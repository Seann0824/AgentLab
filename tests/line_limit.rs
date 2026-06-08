use std::fs;
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 500;

#[test]
fn handwritten_source_and_docs_stay_under_500_lines() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();

    collect_matching(&root.join("src"), &["rs"], &mut files);
    collect_matching(&root.join("docs"), &["md"], &mut files);
    collect_root_markdown(&root, &mut files);

    let mut failures = Vec::new();
    for file in files {
        let line_count = count_lines(&file);
        if line_count > MAX_LINES {
            failures.push(format!(
                "{} has {} lines",
                file.strip_prefix(&root).unwrap_or(&file).display(),
                line_count
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "files exceed {MAX_LINES} lines:\n{}",
        failures.join("\n")
    );
}

fn collect_root_markdown(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
}

fn collect_matching(dir: &Path, extensions: &[&str], files: &mut Vec<PathBuf>) {
    if should_skip_dir(dir) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_matching(&path, extensions, files);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
        {
            files.push(path);
        }
    }
}

fn should_skip_dir(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git" | "target" | ".agent" | ".sessions" | "gen"
        )
    })
}

fn count_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|content| content.lines().count())
        .unwrap_or(0)
}
