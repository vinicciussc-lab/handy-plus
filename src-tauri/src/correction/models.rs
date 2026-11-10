//! Finds the GGUF models the user keeps in a folder, for the correction settings.

use serde::Serialize;
use specta::Type;
use std::path::Path;

#[derive(Serialize, Debug, Clone, Type, PartialEq)]
pub struct LocalModel {
    pub path: String,
    pub label: String,
    pub size_bytes: u64,
}

/// How many folder levels below the models folder are searched (author/model/file).
const MAX_DEPTH: usize = 3;

/// GGUF files up to three levels deep, labeled "author / file" after the first
/// folder under `dir`, sorted by label. Vision projectors (`mmproj*`) are skipped.
pub fn find_local_models(dir: &Path) -> Vec<LocalModel> {
    let mut found = Vec::new();
    collect(dir, None, 1, &mut found);
    found.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.path.cmp(&b.path)));
    found
}

fn collect(dir: &Path, author: Option<&str>, depth: usize, out: &mut Vec<LocalModel>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                let name = entry.file_name().to_string_lossy().into_owned();
                collect(&path, Some(author.unwrap_or(&name)), depth + 1, out);
            }
            continue;
        }
        let is_gguf = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if !is_gguf || stem.to_ascii_lowercase().starts_with("mmproj") {
            continue;
        }
        let label = match author {
            Some(author) => format!("{author} / {stem}"),
            None => stem,
        };
        out.push(LocalModel {
            path: path.to_string_lossy().into_owned(),
            label,
            size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
}

#[tauri::command]
#[specta::specta]
pub fn list_local_models(dir: String) -> Vec<LocalModel> {
    find_local_models(Path::new(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_gguf_files_by_author_and_skips_projectors() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir.join("bartowski/Qwen-GGUF")).unwrap();
        fs::create_dir_all(dir.join("x/y")).unwrap();
        fs::write(dir.join("bartowski/Qwen-GGUF/qwen.Q4.gguf"), b"12345").unwrap();
        fs::write(dir.join("x/y/mmproj-y.gguf"), b"1").unwrap();
        fs::write(dir.join("x/y/notes.txt"), b"1").unwrap();

        let m = find_local_models(dir);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].label, "bartowski / qwen.Q4");
        assert_eq!(m[0].size_bytes, 5);
        assert!(m[0].path.ends_with("qwen.Q4.gguf"));
    }

    #[test]
    fn missing_dir_gives_empty_list() {
        assert!(find_local_models(Path::new(r"Z:\nao\existe")).is_empty());
    }

    #[test]
    fn sorts_by_label_and_ignores_files_deeper_than_three_levels() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir.join("zeta/m")).unwrap();
        fs::create_dir_all(dir.join("alfa/m")).unwrap();
        fs::create_dir_all(dir.join("a/b/c/d")).unwrap();
        fs::write(dir.join("zeta/m/z.gguf"), b"1").unwrap();
        fs::write(dir.join("alfa/m/a.gguf"), b"1").unwrap();
        fs::write(dir.join("a/b/c/d/deep.gguf"), b"1").unwrap();

        let labels: Vec<_> = find_local_models(dir)
            .into_iter()
            .map(|m| m.label)
            .collect();
        assert_eq!(labels, vec!["alfa / a", "zeta / z"]);
    }
}
