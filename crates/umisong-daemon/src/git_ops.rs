use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use umisong_core::{
    DiffHunk, DiffLine, DiffLineKind, DiffScope, FileChangeKind, FileDiff, GitDiffSnapshot,
    ProviderId, parse_unified_diff,
};

fn git_cmd(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd);
    cmd
}

pub fn is_git_repo(path: &Path) -> bool {
    git_cmd(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

pub fn current_branch(path: &Path) -> Option<String> {
    let out = git_cmd(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

pub fn diff_stat(path: &Path) -> String {
    if !is_git_repo(path) {
        return String::new();
    }
    git_cmd(path)
        .args(["diff", "--stat"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

pub fn get_git_diff(path: &Path, scope: DiffScope) -> GitDiffSnapshot {
    let is_repo = is_git_repo(path);
    if !is_repo {
        return GitDiffSnapshot {
            workspace_path: path.to_string_lossy().into_owned(),
            is_git_repo: false,
            branch: None,
            files: Vec::new(),
        };
    }

    let branch = current_branch(path);
    let mut raw = String::new();

    match scope {
        DiffScope::Unstaged => {
            raw.push_str(&run_git_stdout(path, &["diff", "--no-color"]));
        }
        DiffScope::Staged => {
            raw.push_str(&run_git_stdout(path, &["diff", "--cached", "--no-color"]));
        }
        DiffScope::All => {
            // Try diff against HEAD; fallback to unstaged + staged if repo has no commits yet.
            let head_diff = git_cmd(path)
                .args(["diff", "HEAD", "--no-color"])
                .output();
            match head_diff {
                Ok(out) if out.status.success() => {
                    raw.push_str(&String::from_utf8_lossy(&out.stdout));
                }
                _ => {
                    raw.push_str(&run_git_stdout(path, &["diff", "--cached", "--no-color"]));
                    raw.push_str(&run_git_stdout(path, &["diff", "--no-color"]));
                }
            }
        }
    }

    let mut files = parse_unified_diff(&raw);

    // Include untracked text files for Unstaged / All scopes.
    if !matches!(scope, DiffScope::Staged) {
        let untracked = run_git_stdout(path, &["ls-files", "--others", "--exclude-standard"]);
        for rel in untracked.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if files.iter().any(|f| f.path == rel) {
                continue;
            }
            files.push(build_untracked_file_diff(path, rel));
        }
    }

    GitDiffSnapshot {
        workspace_path: path.to_string_lossy().into_owned(),
        is_git_repo: true,
        branch,
        files,
    }
}

fn build_untracked_file_diff(root: &Path, rel: &str) -> FileDiff {
    let full = root.join(rel);
    let bytes = std::fs::read(&full).unwrap_or_default();
    let is_binary = bytes.iter().take(4096).any(|&b| b == 0);
    if is_binary {
        return FileDiff {
            path: rel.to_string(),
            old_path: None,
            kind: FileChangeKind::Untracked,
            additions: 0,
            deletions: 0,
            is_binary: true,
            hunks: Vec::new(),
        };
    }

    let text = String::from_utf8_lossy(&bytes);
    let mut lines = Vec::new();
    for (idx, line) in text.lines().take(500).enumerate() {
        lines.push(DiffLine {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(idx + 1),
            content: line.to_string(),
        });
    }
    let additions = lines.len();
    FileDiff {
        path: rel.to_string(),
        old_path: None,
        kind: FileChangeKind::Untracked,
        additions,
        deletions: 0,
        is_binary: false,
        hunks: if lines.is_empty() {
            Vec::new()
        } else {
            vec![DiffHunk {
                header: format!("@@ -0,0 +1,{additions} @@ (untracked file)"),
                old_start: 0,
                new_start: 1,
                lines,
            }]
        },
    }
}

pub fn revert_file(workspace_path: &Path, file_path: &str) -> Result<()> {
    // Check if tracked first.
    let tracked = git_cmd(workspace_path)
        .args(["ls-files", "--error-unmatch", "--", file_path])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if tracked {
        let out = git_cmd(workspace_path)
            .args(["checkout", "--", file_path])
            .output()
            .context("Failed to execute git checkout")?;
        if !out.status.success() {
            bail!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    } else {
        let target = workspace_path.join(file_path);
        if target.exists() {
            std::fs::remove_file(&target)
                .with_context(|| format!("Failed to remove untracked file {}", target.display()))?;
        }
    }
    Ok(())
}

/// Create an isolated detached git worktree for a Parallel Compare provider column.
pub fn create_compare_worktree(
    repo_path: &Path,
    worktrees_root: &Path,
    compare_id: &str,
    provider: ProviderId,
) -> Result<PathBuf> {
    if !is_git_repo(repo_path) {
        bail!("Workspace is not a git repository; use Read-Only Compare instead.");
    }
    let target_dir = worktrees_root.join(compare_id).join(provider.as_str());
    if let Some(parent) = target_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let out = git_cmd(repo_path)
        .args([
            "worktree",
            "add",
            "--detach",
            &target_dir.to_string_lossy(),
            "HEAD",
        ])
        .output()
        .context("Failed to create git worktree")?;

    if !out.status.success() {
        bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    Ok(target_dir)
}

/// Capture the full patch (tracked + untracked newly added files) from a compare worktree.
pub fn capture_worktree_patch(worktree_path: &Path) -> (String, Vec<FileDiff>) {
    // Stage untracked files with intent-to-add so `git diff` includes newly created files cleanly.
    let _ = git_cmd(worktree_path).args(["add", "-N", "."]).output();
    let patch = run_git_stdout(worktree_path, &["diff", "--no-color"]);
    let diffs = parse_unified_diff(&patch);
    (patch, diffs)
}

/// Apply a unified patch cleanly to the main workspace (pre-checking with `git apply --check`).
pub fn apply_patch_to_workspace(workspace_path: &Path, patch: &str) -> Result<()> {
    if patch.trim().is_empty() {
        return Ok(());
    }
    let tmp = std::env::temp_dir().join(format!("umisong-patch-{}.diff", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, patch)?;

    let check = git_cmd(workspace_path)
        .args(["apply", "--check", &tmp.to_string_lossy()])
        .output()
        .context("Failed to run git apply --check")?;
    if !check.status.success() {
        let _ = std::fs::remove_file(&tmp);
        bail!(
            "Patch could not be applied cleanly (working tree conflict): {}",
            String::from_utf8_lossy(&check.stderr).trim()
        );
    }

    let apply = git_cmd(workspace_path)
        .args(["apply", &tmp.to_string_lossy()])
        .output()
        .context("Failed to run git apply")?;
    let _ = std::fs::remove_file(&tmp);

    if !apply.status.success() {
        bail!(
            "git apply failed: {}",
            String::from_utf8_lossy(&apply.stderr).trim()
        );
    }
    Ok(())
}

pub fn remove_worktree(repo_path: &Path, worktree_path: &Path) {
    let _ = git_cmd(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .output();
    let _ = std::fs::remove_dir_all(worktree_path);
}

fn run_git_stdout(cwd: &Path, args: &[&str]) -> String {
    git_cmd(cwd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert!(git_cmd(p).args(["init"]).status().unwrap().success());
        assert!(git_cmd(p).args(["config", "user.email", "test@umisong.dev"]).status().unwrap().success());
        assert!(git_cmd(p).args(["config", "user.name", "Umisong Test"]).status().unwrap().success());
        fs::write(p.join("hello.txt"), "line 1\nline 2\n").unwrap();
        assert!(git_cmd(p).args(["add", "."]).status().unwrap().success());
        assert!(git_cmd(p).args(["commit", "-m", "init"]).status().unwrap().success());
        dir
    }

    #[test]
    fn worktree_isolation_and_patch_apply() {
        let repo = init_temp_repo();
        let wt_root = tempfile::tempdir().unwrap();

        let wt = create_compare_worktree(
            repo.path(),
            wt_root.path(),
            "cmp-1",
            ProviderId::ClaudeCode,
        )
        .unwrap();

        // Modify a file inside the isolated worktree
        fs::write(wt.join("hello.txt"), "line 1\nline 2 updated\n").unwrap();
        fs::write(wt.join("added.rs"), "pub fn ok() -> bool { true }\n").unwrap();

        let (patch, diffs) = capture_worktree_patch(&wt);
        assert_eq!(diffs.len(), 2);
        assert!(patch.contains("line 2 updated"));
        assert!(patch.contains("added.rs"));

        // Main workspace should still be untouched
        assert_eq!(fs::read_to_string(repo.path().join("hello.txt")).unwrap(), "line 1\nline 2\n");
        assert!(!repo.path().join("added.rs").exists());

        // Apply patch cleanly
        apply_patch_to_workspace(repo.path(), &patch).unwrap();
        assert_eq!(
            fs::read_to_string(repo.path().join("hello.txt")).unwrap(),
            "line 1\nline 2 updated\n"
        );
        assert!(repo.path().join("added.rs").exists());

        // Cleanup worktree
        remove_worktree(repo.path(), &wt);
        assert!(!wt.exists());
    }
}
