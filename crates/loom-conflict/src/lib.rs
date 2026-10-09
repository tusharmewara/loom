//! Conflict Resolution Protocol — detect and resolve merge conflicts.
//!
//! Detects standard Git merge conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`)
//! and provides strategies for programmatic resolution.

use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────

/// A single merge conflict region in a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// File path relative to workspace root.
    pub file: String,
    /// 1-based line number where the conflict starts.
    pub start_line: usize,
    /// Content from "our" branch (above `=======`).
    pub ours: String,
    /// Content from "their" branch (below `=======`).
    pub theirs: String,
    /// The raw conflict marker annotation (e.g. `HEAD` or a branch name).
    pub our_ref: String,
    /// The raw conflict marker annotation for theirs.
    pub their_ref: String,
}

/// Strategy for resolving a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    /// Keep our version only.
    Ours,
    /// Keep their version only.
    Theirs,
    /// Keep both, ours first then theirs.
    BothOursTheirs,
    /// Keep both, theirs first then ours.
    BothTheirsOurs,
}

impl ResolutionStrategy {
    /// Resolve a conflict using this strategy.
    pub fn resolve(&self, conflict: &Conflict) -> String {
        match self {
            ResolutionStrategy::Ours => conflict.ours.clone(),
            ResolutionStrategy::Theirs => conflict.theirs.clone(),
            ResolutionStrategy::BothOursTheirs => {
                format!("{}\n{}", conflict.ours, conflict.theirs)
            }
            ResolutionStrategy::BothTheirsOurs => {
                format!("{}\n{}", conflict.theirs, conflict.ours)
            }
        }
    }
}

// ─── Scanning ────────────────────────────────────────────────────────

/// Scan file content for Git merge conflict markers.
///
/// Returns a list of detected conflicts.
pub fn scan_content(file_path: &str, content: &str) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("<<<<<<< ") {
            let our_ref = line[7..].trim().to_string();
            let start_line = i + 1; // 1-based
            let mut ours_lines = Vec::new();
            let mut theirs_lines = Vec::new();
            let mut in_theirs = false;
            let mut depth = 1;
            i += 1;

            while i < lines.len() && depth > 0 {
                let l = lines[i];
                if l.starts_with("<<<<<<< ") {
                    depth += 1;
                    if in_theirs {
                        theirs_lines.push(l);
                    } else {
                        ours_lines.push(l);
                    }
                } else if l.starts_with("=======") {
                    if depth == 1 {
                        in_theirs = true;
                    } else if in_theirs {
                        theirs_lines.push(l);
                    } else {
                        ours_lines.push(l);
                    }
                } else if l.starts_with(">>>>>>> ") {
                    depth -= 1;
                    if depth == 0 {
                        let their_ref = l[7..].trim().to_string();
                        conflicts.push(Conflict {
                            file: file_path.to_string(),
                            start_line,
                            ours: ours_lines.join("\n"),
                            theirs: theirs_lines.join("\n"),
                            our_ref: our_ref.clone(),
                            their_ref,
                        });
                    } else {
                        if in_theirs {
                            theirs_lines.push(l);
                        } else {
                            ours_lines.push(l);
                        }
                    }
                } else {
                    if in_theirs {
                        theirs_lines.push(l);
                    } else {
                        ours_lines.push(l);
                    }
                }
                i += 1;
            }
        }
        i += 1;
    }

    conflicts
}

/// Scan a file for merge conflicts.
pub fn scan_file(path: &str) -> Result<Vec<Conflict>, ConflictError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ConflictError::Io {
            path: path.to_string(),
            source: e,
        })?;
    Ok(scan_content(path, &content))
}

/// Scan all files in a directory tree for merge conflicts.
///
/// Skips `.git/`, `node_modules/`, `target/`, and binary files.
pub fn scan_directory(dir: &str) -> Result<Vec<Conflict>, ConflictError> {
    let mut all_conflicts = Vec::new();
    let skip_dirs = [".git", "node_modules", "target", ".venv", "vendor", ".build"];

    scan_directory_recursive(dir, &skip_dirs, &mut all_conflicts)?;
    Ok(all_conflicts)
}

fn scan_directory_recursive(
    current: &str,
    skip_dirs: &[&str],
    conflicts: &mut Vec<Conflict>,
) -> Result<(), ConflictError> {
    let dir = std::fs::read_dir(current).map_err(|e| ConflictError::Io {
        path: current.to_string(),
        source: e,
    })?;

    for entry in dir {
        let entry = entry.map_err(|e| ConflictError::Io {
            path: current.to_string(),
            source: e,
        })?;
        let path = entry.path();

        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !skip_dirs.contains(&name.as_ref()) {
                scan_directory_recursive(
                    &path.to_string_lossy(),
                    skip_dirs,
                    conflicts,
                )?;
            }
        } else if path.is_file() {
            // Skip binary-looking files
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                match ext.as_str() {
                    "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "svg"
                    | "woff" | "woff2" | "ttf" | "eot"
                    | "o" | "so" | "dylib" | "dll" | "exe"
                    | "zip" | "tar" | "gz" | "bz2" | "xz" | "7z"
                    | "mp3" | "mp4" | "avi" | "mov" | "mkv"
                    | "pdf" | "doc" | "docx" | "xls" | "xlsx" => continue,
                    _ => {}
                }
            }

            if let Ok(c) = scan_file(&path.to_string_lossy()) {
                conflicts.extend(c);
            }
        }
    }

    Ok(())
}

// ─── Resolution ──────────────────────────────────────────────────────

/// Apply a resolution strategy to all conflicts in a file.
///
/// Returns the resolved file content.
pub fn resolve_file(content: &str, strategy: ResolutionStrategy) -> String {
    let conflicts = scan_content("", content);
    if conflicts.is_empty() {
        return content.to_string();
    }

    let mut result = String::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut conflict_idx = 0;

    while i < lines.len() {
        if conflict_idx < conflicts.len() {
            let conflict = &conflicts[conflict_idx];
            // Check if this line is the start of a conflict
            let line_num = i + 1; // 1-based
            if line_num == conflict.start_line {
                // Skip past the opening <<<<<<< marker
                let mut depth = 1;
                let mut end_line = i + 1;
                while end_line < lines.len() && depth > 0 {
                    let l = lines[end_line];
                    if l.starts_with("<<<<<<< ") {
                        depth += 1;
                    } else if l.starts_with(">>>>>>> ") {
                        depth -= 1;
                    }
                    end_line += 1;
                }

                // Insert the resolved content
                let resolved = strategy.resolve(conflict);
                result.push_str(&resolved);
                if !resolved.ends_with('\n') {
                    result.push('\n');
                }

                i = end_line;
                conflict_idx += 1;
                continue;
            }
        }
        result.push_str(lines[i]);
        result.push('\n');
        i += 1;
    }

    result
}

/// Resolve all conflicts in a file using the given strategy, writing the result.
pub fn resolve_file_in_place(path: &str, strategy: ResolutionStrategy) -> Result<usize, ConflictError> {
    let content = std::fs::read_to_string(path).map_err(|e| ConflictError::Io {
        path: path.to_string(),
        source: e,
    })?;

    let conflicts = scan_content(path, &content);
    if conflicts.is_empty() {
        return Ok(0);
    }

    let resolved = resolve_file(&content, strategy);
    std::fs::write(path, &resolved).map_err(|e| ConflictError::Io {
        path: path.to_string(),
        source: e,
    })?;

    Ok(conflicts.len())
}

// ─── Bulk resolution ─────────────────────────────────────────────────

/// Resolve all conflicts across multiple files using a single strategy.
/// Returns (file_path, count_of_conflicts_resolved).
pub fn resolve_bulk(
    files: &[String],
    strategy: ResolutionStrategy,
) -> Vec<(String, Result<usize, ConflictError>)> {
    files
        .iter()
        .map(|f| {
            let result = resolve_file_in_place(f, strategy);
            (f.clone(), result)
        })
        .collect()
}

// ─── Error type ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ConflictError {
    Io { path: String, source: std::io::Error },
}

impl std::error::Error for ConflictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { ref source, .. } => Some(source),
        }
    }
}

impl std::fmt::Display for ConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "I/O error for {path}: {source}")
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_conflict() -> &'static str {
        concat!(
            "fn main() {\n",
            "<<<<<<< HEAD\n",
            "    println!(\"hello\");\n",
            "=======\n",
            "    println!(\"world\");\n",
            ">>>>>>> feature-branch\n",
            "}\n",
        )
    }

    fn sample_nested_conflict() -> &'static str {
        concat!(
            "outer start\n",
            "<<<<<<< HEAD\n",
            "outer ours\n",
            "<<<<<<< HEAD\n",
            "inner ours\n",
            "=======\n",
            "inner theirs\n",
            ">>>>>>> feature\n",
            "=======\n",
            "outer theirs\n",
            ">>>>>>> feature-branch\n",
            "outer end\n",
        )
    }

    #[test]
    fn test_detect_single_conflict() {
        let content = sample_conflict();
        let conflicts = scan_content("test.rs", content);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].file, "test.rs");
        assert_eq!(conflicts[0].start_line, 2);
        assert_eq!(conflicts[0].our_ref, "HEAD");
        assert_eq!(conflicts[0].their_ref, "feature-branch");
    }

    #[test]
    fn test_ours_resolution() {
        let content = sample_conflict();
        let conflicts = scan_content("", content);
        assert_eq!(conflicts.len(), 1);

        let resolved = ResolutionStrategy::Ours.resolve(&conflicts[0]);
        assert_eq!(resolved, "    println!(\"hello\");");
    }

    #[test]
    fn test_theirs_resolution() {
        let content = sample_conflict();
        let conflicts = scan_content("", content);
        assert_eq!(conflicts.len(), 1);

        let resolved = ResolutionStrategy::Theirs.resolve(&conflicts[0]);
        assert_eq!(resolved, "    println!(\"world\");");
    }

    #[test]
    fn test_both_ours_theirs() {
        let content = sample_conflict();
        let conflicts = scan_content("", content);

        let resolved = ResolutionStrategy::BothOursTheirs.resolve(&conflicts[0]);
        assert!(resolved.contains(r#"println!("hello");"#));
        assert!(resolved.contains(r#"println!("world");"#));
    }

    #[test]
    fn test_resolve_file_ours() {
        let content = sample_conflict();
        let resolved = resolve_file(content, ResolutionStrategy::Ours);
        assert_eq!(
            resolved,
            "fn main() {\n    println!(\"hello\");\n}\n"
        );
    }

    #[test]
    fn test_resolve_file_theirs() {
        let content = sample_conflict();
        let resolved = resolve_file(content, ResolutionStrategy::Theirs);
        assert_eq!(
            resolved,
            "fn main() {\n    println!(\"world\");\n}\n"
        );
    }

    #[test]
    fn test_no_conflicts_returns_original() {
        let content = "clean file\nno conflicts\n";
        let resolved = resolve_file(content, ResolutionStrategy::Ours);
        assert_eq!(resolved, content);
    }

    #[test]
    fn test_multiple_conflicts() {
        let content = "line1\n\
                       <<<<<<< HEAD\n\
                       ours1\n=======\n\
                       theirs1\n>>>>>>> b\n\
                       line2\n\
                       <<<<<<< HEAD\n\
                       ours2\n=======\n\
                       theirs2\n>>>>>>> b\n\
                       line3\n";
        let conflicts = scan_content("", content);
        assert_eq!(conflicts.len(), 2);

        let resolved = resolve_file(content, ResolutionStrategy::Ours);
        assert_eq!(resolved, "line1\nours1\nline2\nours2\nline3\n");
    }

    #[test]
    fn test_nested_conflicts_scanning() {
        let content = sample_nested_conflict();
        let conflicts = scan_content("", content);
        assert_eq!(conflicts.len(), 1);
        // The inner conflict is nested within the outer's "ours" section
        assert!(conflicts[0].ours.contains("inner ours"));
        assert!(conflicts[0].ours.contains("inner theirs"));
        assert!(!conflicts[0].theirs.contains("inner theirs"));
        assert!(conflicts[0].theirs.contains("outer theirs"));
    }

    #[test]
    fn test_resolve_file_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conflict.rs");
        std::fs::write(&path, sample_conflict()).unwrap();

        let count = resolve_file_in_place(
            &path.to_string_lossy(),
            ResolutionStrategy::Theirs,
        )
        .unwrap();
        assert_eq!(count, 1);

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "fn main() {\n    println!(\"world\");\n}\n");
    }

    #[test]
    fn test_scan_nonexistent_file_error() {
        let result = scan_file("/nonexistent/path.rs");
        assert!(result.is_err());
    }

    #[test]
    fn test_bulk_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("f1.rs");
        let p2 = dir.path().join("f2.rs");
        std::fs::write(&p1, sample_conflict()).unwrap();
        std::fs::write(&p2, "clean file").unwrap();

        let files = vec![p1.to_string_lossy().to_string(), p2.to_string_lossy().to_string()];
        let results = resolve_bulk(&files, ResolutionStrategy::Ours);

        assert_eq!(results.len(), 2);
        assert!(results[0].1.is_ok());
        assert_eq!(results[0].1.as_ref().unwrap(), &1);
        assert!(results[1].1.is_ok());
        assert_eq!(results[1].1.as_ref().unwrap(), &0);
    }

    #[test]
    fn test_scan_directory_no_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("clean.rs");
        std::fs::write(&f, "fn main() {}").unwrap();

        let conflicts = scan_directory(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(conflicts.len(), 0);
    }

    #[test]
    fn test_resolution_strategy_roundtrip() {
        use ResolutionStrategy::*;
        let strategies = [Ours, Theirs, BothOursTheirs, BothTheirsOurs];
        for strategy in &strategies {
            let content = sample_conflict();
            let resolved = resolve_file(content, *strategy);
            // The resolved file should not contain conflict markers
            assert!(!resolved.contains("<<<<<<<"));
            assert!(!resolved.contains("======="));
            assert!(!resolved.contains(">>>>>>>"));
        }
    }

    #[test]
    fn test_conflict_ref_parsing() {
        let content = "<<<<<<< HEAD\nfoo\n=======\nbar\n>>>>>>> feature/my-branch\n";
        let conflicts = scan_content("", content);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].our_ref, "HEAD");
        assert_eq!(conflicts[0].their_ref, "feature/my-branch");
    }
}
