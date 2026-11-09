//! One-time import of the user's Handy settings and speech models into the
//! Handy Plus data directory.

use std::fs;
use std::io;
use std::path::Path;

const SETTINGS_FILE: &str = "settings_store.json";
const MODELS_DIR: &str = "models";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Imported,
    AlreadyHasSettings,
    NoHandyData,
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies `settings_store.json` and the `models/` folder from the Handy data
/// directory (`src_dir`) into the Handy Plus one (`dst_dir`). History, logs and
/// webview data are never copied. Does nothing if the destination already has
/// settings or the source has none.
pub fn import_from_handy(src_dir: &Path, dst_dir: &Path) -> io::Result<ImportOutcome> {
    if dst_dir.join(SETTINGS_FILE).exists() {
        return Ok(ImportOutcome::AlreadyHasSettings);
    }
    let src_settings = src_dir.join(SETTINGS_FILE);
    if !src_settings.is_file() {
        return Ok(ImportOutcome::NoHandyData);
    }

    fs::create_dir_all(dst_dir)?;
    let src_models = src_dir.join(MODELS_DIR);
    if src_models.is_dir() {
        copy_dir_recursive(&src_models, &dst_dir.join(MODELS_DIR))?;
    }
    // Settings last: its presence marks the import as complete, so a failed
    // model copy is retried on the next start.
    fs::copy(&src_settings, dst_dir.join(SETTINGS_FILE))?;
    Ok(ImportOutcome::Imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_settings_and_models_when_destination_is_empty() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join(SETTINGS_FILE), r#"{"a":1}"#).unwrap();
        fs::create_dir_all(src.path().join("models/parakeet")).unwrap();
        fs::write(src.path().join("models/parakeet/x.bin"), b"bin").unwrap();

        let out = import_from_handy(src.path(), dst.path()).unwrap();

        assert_eq!(out, ImportOutcome::Imported);
        assert_eq!(
            fs::read_to_string(dst.path().join(SETTINGS_FILE)).unwrap(),
            r#"{"a":1}"#
        );
        assert!(dst.path().join("models/parakeet/x.bin").exists());
    }

    #[test]
    fn keeps_existing_destination_settings() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join(SETTINGS_FILE), "from-handy").unwrap();
        fs::write(dst.path().join(SETTINGS_FILE), "mine").unwrap();

        let out = import_from_handy(src.path(), dst.path()).unwrap();

        assert_eq!(out, ImportOutcome::AlreadyHasSettings);
        assert_eq!(
            fs::read_to_string(dst.path().join(SETTINGS_FILE)).unwrap(),
            "mine"
        );
    }

    #[test]
    fn does_nothing_without_handy_data() {
        let base = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        let out = import_from_handy(&base.path().join("missing"), dst.path()).unwrap();

        assert_eq!(out, ImportOutcome::NoHandyData);
        assert_eq!(fs::read_dir(dst.path()).unwrap().count(), 0);
    }
}
