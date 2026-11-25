use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Returns an enriched PATH string combining the current process PATH with common
/// macOS/Unix user and Homebrew binary directories so Node/Python/Rust CLIs work
/// even when launched outside a login shell.
pub fn enriched_path() -> String {
    let mut dirs_vec: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut push_dir = |p: PathBuf| {
        if seen.insert(p.clone()) {
            dirs_vec.push(p);
        }
    };

    if let Some(existing) = env::var_os("PATH") {
        for p in env::split_paths(&existing) {
            push_dir(p);
        }
    }

    for sys in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
    ] {
        push_dir(PathBuf::from(sys));
    }

    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".local/bin",
            ".npm-global/bin",
            ".pnpm-global/bin",
            ".bun/bin",
            ".cargo/bin",
            ".gemini/antigravity/bin",
            ".antigravity/bin",
            ".volta/bin",
            ".asdf/shims",
            ".mise/shims",
        ] {
            push_dir(home.join(rel));
        }

        // Check NVM installed node versions (prefer highest lexicographic version).
        let nvm_dir = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm_dir) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path().join("bin"))
                .filter(|p| p.is_dir())
                .collect();
            versions.sort();
            versions.reverse();
            for v in versions {
                push_dir(v);
            }
        }
    }

    env::join_paths(dirs_vec)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| env::var("PATH").unwrap_or_default())
}

/// Locate a CLI executable by checking:
/// 1. User-specified `override_path`
/// 2. All directories in `enriched_path()`
pub fn find_binary(binary_name: &str, override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = override_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let expanded = expand_tilde(trimmed);
            if is_executable_file(&expanded) {
                return Some(expanded);
            }
        }
    }

    let path_str = enriched_path();
    for dir in env::split_paths(&path_str) {
        let candidate = dir.join(binary_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    } else {
        false
    }
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Probe `<binary> --version` with a short timeout, returning the first non-empty line.
pub async fn probe_version(binary: &Path) -> Option<String> {
    let fut = Command::new(binary)
        .arg("--version")
        .env("PATH", enriched_path())
        .stdin(std::process::Stdio::null())
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(4), fut)
        .await
        .ok()?
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(first.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn finds_override_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("my-cli");
        fs::write(&fake, "#!/bin/sh\necho 1.0.0\n").unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake, perms).unwrap();
        }
        let found = find_binary("nonexistent-cli-xyz", Some(fake.to_str().unwrap()));
        assert_eq!(found, Some(fake));
    }

    #[test]
    fn ignores_non_executable_override() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("not-exec");
        fs::write(&fake, "plain text").unwrap();
        let found = find_binary("nonexistent-cli-xyz", Some(fake.to_str().unwrap()));
        assert_eq!(found, None);
    }
}
