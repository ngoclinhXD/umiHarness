use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffScope {
    Unstaged,
    Staged,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

impl FileChangeKind {
    pub fn badge(self) -> &'static str {
        match self {
            FileChangeKind::Modified => "M",
            FileChangeKind::Added => "A",
            FileChangeKind::Deleted => "D",
            FileChangeKind::Renamed => "R",
            FileChangeKind::Untracked => "?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    Meta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: FileChangeKind,
    pub additions: usize,
    pub deletions: usize,
    pub is_binary: bool,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffSnapshot {
    pub workspace_path: String,
    pub is_git_repo: bool,
    pub branch: Option<String>,
    pub files: Vec<FileDiff>,
}

/// Pure-Rust parser for standard git unified diffs (`diff --git ...`).
pub fn parse_unified_diff(raw: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line_no: usize = 0;
    let mut new_line_no: usize = 0;

    let flush_hunk = |file: &mut Option<FileDiff>, hunk: &mut Option<DiffHunk>| {
        if let (Some(f), Some(h)) = (file.as_mut(), hunk.take()) {
            f.hunks.push(h);
        }
    };

    let flush_file = |files: &mut Vec<FileDiff>,
                      file: &mut Option<FileDiff>,
                      hunk: &mut Option<DiffHunk>| {
        flush_hunk(file, hunk);
        if let Some(f) = file.take() {
            files.push(f);
        }
    };

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush_file(&mut files, &mut current_file, &mut current_hunk);
            let (old_p, new_p) = parse_diff_git_paths(rest);
            current_file = Some(FileDiff {
                path: new_p.clone(),
                old_path: if old_p != new_p { Some(old_p) } else { None },
                kind: FileChangeKind::Modified,
                additions: 0,
                deletions: 0,
                is_binary: false,
                hunks: Vec::new(),
            });
            continue;
        }

        let Some(file) = current_file.as_mut() else {
            continue;
        };

        if line.starts_with("new file mode") {
            file.kind = FileChangeKind::Added;
        } else if line.starts_with("deleted file mode") {
            file.kind = FileChangeKind::Deleted;
        } else if line.starts_with("rename from ") {
            file.kind = FileChangeKind::Renamed;
            file.old_path = Some(line.trim_start_matches("rename from ").to_string());
        } else if line.starts_with("rename to ") {
            file.kind = FileChangeKind::Renamed;
            file.path = line.trim_start_matches("rename to ").to_string();
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.is_binary = true;
        } else if line.starts_with("@@ ") {
            flush_hunk(&mut current_file, &mut current_hunk);
            let (old_start, new_start) = parse_hunk_header(line);
            old_line_no = old_start;
            new_line_no = new_start;
            current_hunk = Some(DiffHunk {
                header: line.to_string(),
                old_start,
                new_start,
                lines: Vec::new(),
            });
        } else if let Some(hunk) = current_hunk.as_mut() {
            if let Some(content) = line.strip_prefix('+') {
                file.additions += 1;
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    old_line: None,
                    new_line: Some(new_line_no),
                    content: content.to_string(),
                });
                new_line_no += 1;
            } else if let Some(content) = line.strip_prefix('-') {
                file.deletions += 1;
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    old_line: Some(old_line_no),
                    new_line: None,
                    content: content.to_string(),
                });
                old_line_no += 1;
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    old_line: Some(old_line_no),
                    new_line: Some(new_line_no),
                    content: content.to_string(),
                });
                old_line_no += 1;
                new_line_no += 1;
            } else if line.starts_with('\\') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Meta,
                    old_line: None,
                    new_line: None,
                    content: line.to_string(),
                });
            }
        }
    }

    flush_file(&mut files, &mut current_file, &mut current_hunk);
    files
}

fn parse_diff_git_paths(rest: &str) -> (String, String) {
    // Handle `a/path/with spaces b/path/with spaces` or standard `a/foo b/foo`.
    if let Some(idx) = rest.rfind(" b/") {
        let left = rest[..idx].trim().trim_matches('"');
        let right = rest[idx + 1..].trim().trim_matches('"');
        let old_p = left.strip_prefix("a/").unwrap_or(left).to_string();
        let new_p = right.strip_prefix("b/").unwrap_or(right).to_string();
        return (old_p, new_p);
    }
    let mut parts = rest.split_whitespace();
    let old_p = parts
        .next()
        .unwrap_or("")
        .strip_prefix("a/")
        .unwrap_or("")
        .to_string();
    let new_p = parts
        .next()
        .unwrap_or("")
        .strip_prefix("b/")
        .unwrap_or("")
        .to_string();
    (old_p, new_p)
}

fn parse_hunk_header(header: &str) -> (usize, usize) {
    // Format: @@ -12,7 +14,9 @@ optional context
    let mut old_start = 1usize;
    let mut new_start = 1usize;
    for token in header.split_whitespace() {
        if let Some(rest) = token.strip_prefix('-') {
            if let Some(num) = rest.split(',').next() {
                if let Ok(v) = num.parse::<usize>() {
                    old_start = v;
                }
            }
        } else if let Some(rest) = token.strip_prefix('+') {
            if let Some(num) = rest.split(',').next() {
                if let Ok(v) = num.parse::<usize>() {
                    new_start = v;
                }
            }
        }
    }
    (old_start, new_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_file_unified_diff() {
        let sample = r#"diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,4 +1,5 @@
 fn main() {
-    println!("old");
+    println!("new");
+    println!("extra");
 }
diff --git a/assets/logo.png b/assets/logo.png
new file mode 100644
Binary files /dev/null and b/assets/logo.png differ
diff --git a/old_name.txt b/old_name.txt
deleted file mode 100644
--- a/old_name.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-goodbye
"#;

        let files = parse_unified_diff(sample);
        assert_eq!(files.len(), 3);

        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].kind, FileChangeKind::Modified);
        assert_eq!(files[0].additions, 2);
        assert_eq!(files[0].deletions, 1);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].lines.len(), 5);
        assert_eq!(files[0].hunks[0].lines[1].old_line, Some(2));
        assert_eq!(files[0].hunks[0].lines[1].kind, DiffLineKind::Removed);
        assert_eq!(files[0].hunks[0].lines[2].new_line, Some(2));
        assert_eq!(files[0].hunks[0].lines[2].kind, DiffLineKind::Added);

        assert_eq!(files[1].path, "assets/logo.png");
        assert_eq!(files[1].kind, FileChangeKind::Added);
        assert!(files[1].is_binary);

        assert_eq!(files[2].path, "old_name.txt");
        assert_eq!(files[2].kind, FileChangeKind::Deleted);
        assert_eq!(files[2].deletions, 1);
    }
}
