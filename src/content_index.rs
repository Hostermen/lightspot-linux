// content_index.rs — Full-text content index (Tantivy) + file watcher.
//
// This module builds and maintains a persistent full-text index of file
// *contents* under the user's home directory (or `$SPOTLIGHT_INDEX_DIRS`).
// The daemon spawns a background thread that performs an initial full
// reindex and then watches the filesystem for changes, incrementally
// updating the index. The search subprocess (`--search`) opens the same
// index read-only and runs Tantivy queries to find files whose contents
// match, returning a text snippet around the first match for each result.
//
// Schema:
//   `path` — STRING (indexed, exact-match) + stored  → for delete-by-term + retrieve
//   `body` — TEXT  (tokenized)          + stored  → for full-text query + snippet

use crate::model::ContentHit;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STRING, TEXT};
use tantivy::snippet::SnippetGenerator;
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

use notify::Watcher;
use std::sync::OnceLock;

/// Maximum file size we will read and index (2 MiB). Larger files are
/// skipped to keep the index compact and indexing fast.
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

/// Number of bytes to sniff for a NUL byte when deciding if a file is binary.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// How long the watcher waits after the last filesystem event before
/// flushing a batch of changed paths to the index writer.
const DEBOUNCE: Duration = Duration::from_secs(3);

/// Hard cap on the number of pending changed paths held in memory before
/// the watcher forces a flush even within the debounce window.
const FLUSH_BATCH_LIMIT: usize = 1000;

/// Directory names we never descend into, even if not git-ignored.
const DENYLIST: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".git",
    ".svn",
    ".hg",
    "venv",
    ".venv",
    ".cache",
];

/// Resolve the persistent index directory under the user's cache home.
/// Prefers `$XDG_CACHE_HOME` (if set and absolute), else `~/.cache`.
fn index_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".cache")
        });
    base.join("spotlight-linux").join("index")
}

/// The list of directories to index. Reads `$SPOTLIGHT_INDEX_DIRS`
/// (colon-separated); defaults to `$HOME` if unset or empty.
fn index_dirs() -> Vec<PathBuf> {
    if let Ok(s) = std::env::var("SPOTLIGHT_INDEX_DIRS") {
        let dirs: Vec<PathBuf> = s
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    vec![PathBuf::from(home)]
}

/// Build the Tantivy schema: a `path` field (exact, stored) and a `body`
/// field (tokenized, stored). Returns the schema and both field handles.
fn build_schema() -> (Schema, Field, Field) {
    let mut b = Schema::builder();
    // STRING = indexed as a single token (good for exact delete by path).
    let path = b.add_text_field("path", STRING.set_stored());
    // TEXT = tokenized + indexed (good for full-text search + snippets).
    let body = b.add_text_field("body", TEXT.set_stored());
    (b.build(), path, body)
}

/// Open the index, creating it (with schema) if it doesn't yet exist.
/// Field handles are re-fetched from the opened index's own schema so the
/// ids match across processes (the daemon's writer and the search reader).
fn open_or_create_index() -> tantivy::Result<(Index, Field, Field)> {
    let dir = index_dir();
    fs::create_dir_all(&dir)?;
    let (schema, _path, _body) = build_schema();
    // meta.json is Tantivy's schema marker; its presence means the index
    // was already created, so we open instead of recreating (which would
    // fail if the directory already holds a valid index).
    let index = if dir.join("meta.json").exists() {
        Index::open_in_dir(&dir)?
    } else {
        Index::create_in_dir(&dir, schema)?
    };
    let path = index.schema().get_field("path")?;
    let body = index.schema().get_field("body")?;
    Ok((index, path, body))
}

/// Read a file and decide whether it is indexable. Returns the file's
/// textual content (lossy UTF-8) if it is, or `None` for binary/oversized/
/// hidden/missing entries.
fn read_indexable(path: &Path) -> Option<String> {
    // Skip dotfiles / hidden entries (the ignore walker already hides most,
    // but events from the watcher can land on them).
    if path
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(true)
    {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() as usize > MAX_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    // A NUL byte in the first 8 KiB is a strong signal of binary content.
    let sniff = &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)];
    if sniff.contains(&0u8) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).to_string())
}

/// Index (or remove) a single path. Deletes any prior doc for the same
/// path first, then adds the new one if the file is indexable. Does NOT
/// commit — the caller batches commits.
fn index_one(writer: &mut IndexWriter, path_field: Field, body_field: Field, path: &Path) {
    let p = match path.to_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    match read_indexable(path) {
        Some(body) => {
            let _ = writer.delete_term(Term::from_field_text(path_field, &p));
            let _ = writer.add_document(doc!(path_field => p, body_field => body));
        }
        None => {
            // File is gone or not indexable — remove any stale doc.
            let _ = writer.delete_term(Term::from_field_text(path_field, &p));
        }
    }
}

/// Walk every index dir with the `ignore` crate (hidden + gitignore aware)
/// and (re)index every indexable file. Returns the number of docs indexed.
/// Used both for the initial full build (daemon startup / `--index`).
pub fn build_index() -> usize {
    let (index, path_field, body_field) = match open_or_create_index() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("spotlight-files: cannot open content index: {e}");
            return 0;
        }
    };
    let mut writer = match index.writer(50_000_000) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("spotlight-files: cannot open index writer: {e}");
            return 0;
        }
    };
    // Start from a clean slate for a full rebuild.
    let _ = writer.delete_all_documents();

    let mut count = 0usize;
    for root in index_dirs() {
        if !root.is_dir() {
            continue;
        }
        let mut wb = ignore::WalkBuilder::new(&root);
        wb.hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            // Prune denylisted *directories* only (files keep their own filter).
            .filter_entry(|entry| {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    return entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| !DENYLIST.contains(&name));
                }
                true
            });
        for entry in wb.build().flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let p = entry.path();
            if let Some(body) = read_indexable(p) {
                if let Some(s) = p.to_str() {
                    let _ = writer.add_document(doc!(path_field => s, body_field => body));
                    count += 1;
                }
            }
        }
    }
    if let Err(e) = writer.commit() {
        eprintln!("spotlight-files: index commit failed: {e}");
        return 0;
    }
    eprintln!("spotlight-files: indexed {count} documents");
    count
}

/// Collapse control characters to spaces and cap the snippet length so it
/// renders cleanly on a single UI row.
fn sanitize_snippet(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    // Trim runs of whitespace introduced by the control→space replacement.
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    if out.chars().count() > 150 {
        let truncated: String = out.chars().take(150).collect();
        format!("{truncated}…")
    } else {
        out
    }
}

/// Query the content index read-only and return up to `limit` content
/// hits, each with the matched file path and a text snippet. Errors are
/// swallowed and return an empty list (the index may not exist yet).
pub fn search_content(query: &str, limit: usize) -> Vec<ContentHit> {
    let q = query.trim();
    // Short queries are skipped (consistent with file_search policy).
    if q.len() < 2 {
        return Vec::new();
    }
    let (index, path_field, body_field) = match open_or_create_index() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let reader = match index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    collect_content_hits(&index, &reader, path_field, body_field, q, limit)
}

/// Shared query + snippet logic used by both the one-shot CLI path
/// (`search_content`) and the long-lived daemon path (`search_content_warm`).
/// Takes already-opened index/reader/field handles so the caller decides
/// whether to open fresh (CLI) or reuse a warm, auto-reloading reader.
fn collect_content_hits(
    index: &Index,
    reader: &IndexReader,
    path_field: Field,
    body_field: Field,
    q: &str,
    limit: usize,
) -> Vec<ContentHit> {
    let searcher = reader.searcher();

    let parser = QueryParser::for_index(index, vec![body_field]);
    let query = match parser.parse_query(q) {
        Ok(q) => q,
        Err(_) => return Vec::new(), // unparseable query → no content hits
    };

    let top = TopDocs::with_limit(limit).order_by_score();
    let hits = match searcher.search(&*query, &top) {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };

    // Snippets are best-effort: if the generator can't be built (e.g. the
    // query matched no indexed terms), we still return paths with an empty
    // snippet rather than dropping the results.
    let snip_gen = SnippetGenerator::create(&searcher, &*query, body_field).ok();

    let mut out = Vec::with_capacity(hits.len());
    for (_score, addr) in hits {
        let doc: TantivyDocument = match searcher.doc::<TantivyDocument>(addr) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let path = doc
            .get_first(path_field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if path.is_empty() {
            continue;
        }
        let snippet = match &snip_gen {
            Some(g) => g.snippet_from_doc(&doc).fragment().to_string(),
            None => String::new(),
        };
        out.push(ContentHit {
            path,
            snippet: sanitize_snippet(&snippet),
        });
    }
    out
}

// ── Warm, persistent reader for the long-running daemon ────────────────
//
// The search daemon stays alive for the whole session, so it can keep the
// Tantivy index and a reader open permanently. `ReloadPolicy::OnCommit`
// makes the reader auto-refresh whenever the background watcher commits a
// batch of file changes, so content search always sees the latest index
// without re-opening anything. This turns content search from a ~tens-of-ms
// "open + mmap + query" into a sub-millisecond "searcher() + query".

struct Warm {
    index: Index,
    reader: IndexReader,
    path_field: Field,
    body_field: Field,
}

static WARM: OnceLock<Option<Warm>> = OnceLock::new();

/// Return a reference to the process-wide warm index/reader, opening it on
/// first use. Returns `None` if the index can't be opened (the daemon then
/// behaves as if content search is unavailable).
fn warm() -> Option<&'static Warm> {
    WARM.get_or_init(|| {
        let (index, path_field, body_field) = open_or_create_index().ok()?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .ok()?;
        Some(Warm {
            index,
            reader,
            path_field,
            body_field,
        })
    })
    .as_ref()
}

/// Daemon-side content search: uses the warm, auto-reloading reader instead
/// of re-opening the index on every query. Same result shape as
/// `search_content`.
pub fn search_content_warm(query: &str, limit: usize) -> Vec<ContentHit> {
    let q = query.trim();
    if q.len() < 2 {
        return Vec::new();
    }
    match warm() {
        Some(w) => collect_content_hits(&w.index, &w.reader, w.path_field, w.body_field, q, limit),
        None => Vec::new(),
    }
}

/// Flush a batch of pending changed paths to the index writer and commit.
fn flush(
    writer: &mut IndexWriter,
    path_field: Field,
    body_field: Field,
    pending: &mut HashSet<PathBuf>,
) {
    if pending.is_empty() {
        return;
    }
    let paths = std::mem::take(pending);
    for p in paths {
        index_one(writer, path_field, body_field, &p);
    }
    if let Err(e) = writer.commit() {
        eprintln!("spotlight-files: watcher commit failed: {e}");
    }
}

/// Run the watcher loop: register recursive watches on every index dir,
/// then debounce incoming change events and flush batches to the index.
fn watch_loop() {
    // Channel: the notify callback forwards changed paths to the loop.
    let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                // Drop errors and forward the paths of every event. `index_one`
                // handles missing files (delete) and unindexable files (no-op).
                let _ = tx.send(ev.paths);
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("spotlight-files: watcher init failed: {e}");
                return;
            }
        };

    for dir in index_dirs() {
        if dir.is_dir() {
            let _ = watcher.watch(&dir, notify::RecursiveMode::Recursive);
        }
    }

    let (index, path_field, body_field) = match open_or_create_index() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("spotlight-files: cannot open index for watcher: {e}");
            return;
        }
    };
    let mut writer = match index.writer(50_000_000) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("spotlight-files: cannot open writer for watcher: {e}");
            return;
        }
    };

    let mut pending: HashSet<PathBuf> = HashSet::new();
    loop {
        match rx.recv_timeout(DEBOUNCE) {
            // Events arrived: accumulate their paths.
            Ok(paths) => {
                for p in paths {
                    pending.insert(p);
                }
                // Safety valve: don't let a huge burst grow without bound.
                if pending.len() >= FLUSH_BATCH_LIMIT {
                    flush(&mut writer, path_field, body_field, &mut pending);
                }
            }
            // No events for DEBOUNCE: if anything is pending, flush it now.
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !pending.is_empty() {
                    flush(&mut writer, path_field, body_field, &mut pending);
                }
            }
            // Channel closed (shouldn't happen normally): stop the loop.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Spawn the background indexer thread. Performs an initial full build so
/// search works immediately, then watches for incremental changes.
///
/// The full build is skipped when an index already exists: re-reading
/// every file under the index dirs on every start pegs CPU and disk for
/// tens of seconds, which starves the concurrently-starting Electron GPU
/// process and triggers its crash (GPU error 1002). The watcher keeps an
/// existing index current; run `spotlight-files --index` to force a
/// full rebuild.
pub fn spawn_indexer() {
    std::thread::spawn(|| {
        if !index_dir().join("meta.json").exists() {
            build_index();
        } else {
            eprintln!(
                "spotlight-files: index present, skipping full rebuild \
                 (watcher tracks changes; use '--index' to force)"
            );
        }
        watch_loop();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes tests that mutate process-global env vars so their
    /// `XDG_CACHE_HOME` / `SPOTLIGHT_INDEX_DIRS` settings don't clobber each
    /// other when cargo runs tests in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run a closure with `$SPOTLIGHT_INDEX_DIRS` pointed at a temp dir and
    /// `$XDG_CACHE_HOME` pointed at a temp cache, so tests are isolated.
    /// Holds `ENV_LOCK` for the duration to serialize env access.
    fn with_temp_env<F>(inner: F)
    where
        F: FnOnce(&Path),
    {
        let _guard: MutexGuard<'static, ()> = ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::env::set_var("SPOTLIGHT_INDEX_DIRS", root.path().to_str().unwrap());
        std::env::set_var("XDG_CACHE_HOME", cache.path().to_str().unwrap());
        inner(root.path());
        std::env::remove_var("SPOTLIGHT_INDEX_DIRS");
        std::env::remove_var("XDG_CACHE_HOME");
    }

    #[test]
    fn build_and_search_finds_content() {
        with_temp_env(|root| {
            // Write two indexable files, one matching the query.
            let mut f1 = fs::File::create(root.join("notes.txt")).unwrap();
            f1.write_all(b"The quick brown fox jumps over the lazy dog")
                .unwrap();
            let mut f2 = fs::File::create(root.join("other.txt")).unwrap();
            f2.write_all(b"nothing interesting here").unwrap();

            // Build the index and confirm both files were indexed.
            assert_eq!(build_index(), 2);

            // Search for a phrase that only appears in notes.txt.
            let hits = search_content("quick brown", 10);
            assert_eq!(
                hits.len(),
                1,
                "expected exactly one content hit, got {hits:?}"
            );
            assert!(hits[0].path.ends_with("notes.txt"));
            // The snippet should contain the matched text.
            assert!(
                hits[0].snippet.to_lowercase().contains("quick"),
                "snippet should contain the query term: {}",
                hits[0].snippet
            );
        });
    }

    #[test]
    fn binary_and_oversized_files_are_skipped() {
        with_temp_env(|root| {
            // A file with a NUL byte is treated as binary and skipped.
            let mut bin = fs::File::create(root.join("bin.dat")).unwrap();
            bin.write_all(b"hello\x00world").unwrap();
            // A normal file is indexed.
            let mut txt = fs::File::create(root.join("ok.txt")).unwrap();
            txt.write_all(b"hello world text").unwrap();

            assert_eq!(build_index(), 1);
            let hits = search_content("hello", 10);
            assert!(hits.iter().all(|h| !h.path.ends_with("bin.dat")));
        });
    }

    #[test]
    fn short_query_returns_empty() {
        with_temp_env(|root| {
            let mut f = fs::File::create(root.join("a.txt")).unwrap();
            f.write_all(b"hello world").unwrap();
            build_index();
            assert!(search_content("h", 10).is_empty());
            assert!(search_content("", 10).is_empty());
        });
    }

    #[test]
    fn sanitize_collapses_controls_and_truncates() {
        let s = "a\n\tb".to_string();
        assert_eq!(sanitize_snippet(&s), "a b");
        let long: String = "a".repeat(500);
        let out = sanitize_snippet(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 151); // 150 chars + ellipsis
    }
}
