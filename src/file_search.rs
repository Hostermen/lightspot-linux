// file_search.rs — File search backed by `plocate`.
//
// `plocate` reads a prebuilt, compressed filename index (refreshed daily by
// `updatedb.plocate`). A query is a single read-only scan — no filesystem
// walk and no running daemon — so it answers in milliseconds with near-zero
// idle cost. This module is a thin wrapper that shells out to the `plocate`
// binary and turns its newline-delimited output into `FileHit` values.

use crate::model::FileHit;
use std::process::Command;

/// Returns true if the `plocate` binary is installed and runnable.
/// The UI uses this to warn the user when file search is unavailable.
pub fn plocate_available() -> bool {
    Command::new("plocate")
        .arg("--version") // cheapest invocation that exists
        .stdout(std::process::Stdio::null()) // discard output
        .stderr(std::process::Stdio::null()) // and errors
        .status() // returns Ok only if it launched
        .is_ok()
}

/// Run `plocate -i -l <limit> <query>` and return each output line as a FileHit.
pub fn search_files(query: &str, limit: usize) -> Vec<FileHit> {
    let q = query.trim();
    // Short queries are skipped to avoid huge, useless result sets.
    if q.len() < 2 {
        return Vec::new();
    }
    // Spawn plocate: -i = case-insensitive, -l N = cap number of results.
    let out = Command::new("plocate")
        .arg("-i")
        .arg("-l")
        .arg(limit.to_string())
        .arg(q)
        .output();
    match out {
        // Success → one FileHit per non-empty line of stdout.
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| FileHit {
                path: l.to_string(),
            })
            .collect(),
        // plocate missing or failed → empty list (file search silently disabled).
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_query_returns_empty() {
        // 0- and 1-char queries never reach plocate.
        assert!(search_files("a", 10).is_empty());
        assert!(search_files("", 10).is_empty());
    }

    #[test]
    fn finds_a_real_file() {
        // plocate index was built in this environment; /etc/passwd always exists.
        let hits = search_files("passwd", 50);
        assert!(
            plocate_available(),
            "plocate must be installed for this test"
        );
        assert!(
            hits.iter()
                .any(|h| h.path.ends_with("passwd") || h.path.contains("passwd")),
            "expected to find a path containing 'passwd', got: {:?}",
            hits.iter().map(|h| &h.path).collect::<Vec<_>>()
        );
    }
}
