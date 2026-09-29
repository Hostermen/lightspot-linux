use crate::model::FileHit;
use std::process::Command;

pub fn plocate_available() -> bool {
    Command::new("plocate")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

pub fn search_files(query: &str, limit: usize) -> Vec<FileHit> {
    let q = query.trim();
    if q.len() < 2 {
        return Vec::new();
    }
    let out = Command::new("plocate")
        .arg("-i")
        .arg("-l")
        .arg(limit.to_string())
        .arg(q)
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| FileHit {
                path: l.to_string(),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_query_returns_empty() {
        assert!(search_files("a", 10).is_empty());
        assert!(search_files("", 10).is_empty());
    }

    #[test]
    fn finds_a_real_file() {
        // plocate index was built in this environment; /etc/passwd always exists.
        let hits = search_files("passwd", 50);
        assert!(plocate_available(), "plocate must be installed for this test");
        assert!(
            hits.iter().any(|h| h.path.ends_with("passwd") || h.path.contains("passwd")),
            "expected to find a path containing 'passwd', got: {:?}",
            hits.iter().map(|h| &h.path).collect::<Vec<_>>()
        );
    }
}

