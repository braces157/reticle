use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use windows::Win32::System::SystemInformation::GetLocalTime;

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("could not determine a default screenshots directory")]
    NoSaveDirectory,
    #[error("failed to create screenshots folder {path}: {source}")]
    CreateDirFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write screenshot {path}: {source}")]
    WriteFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Resolves the target screenshots directory from config or defaults to `%USERPROFILE%\Pictures\SnipTool`.
pub fn resolve_save_dir(configured: Option<&str>) -> Result<PathBuf, SaveError> {
    if let Some(raw) = configured {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(profile).join("Pictures").join("SnipTool"));
    }

    std::env::current_dir().map_err(|_| SaveError::NoSaveDirectory)
}

fn local_timestamp_stem() -> String {
    // SAFETY: GetLocalTime writes to a valid stack-allocated SYSTEMTIME struct.
    let st = unsafe { GetLocalTime() };
    format!(
        "Snip_{:04}{:02}{:02}_{:02}{:02}{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

fn unique_png_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.png"));
    if !first.exists() {
        return first;
    }
    for idx in 1..=999u32 {
        let candidate = dir.join(format!("{stem}_{idx}.png"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}_overflow.png"))
}

pub fn save_png_bytes(configured_dir: Option<&str>, png_bytes: &[u8]) -> Result<PathBuf, SaveError> {
    let dir = resolve_save_dir(configured_dir)?;
    save_png_bytes_to_dir(&dir, &local_timestamp_stem(), png_bytes)
}

pub fn save_png_bytes_to_dir(
    dir: &Path,
    stem: &str,
    png_bytes: &[u8],
) -> Result<PathBuf, SaveError> {
    fs::create_dir_all(dir).map_err(|source| SaveError::CreateDirFailed {
        path: dir.to_path_buf(),
        source,
    })?;
    let target = unique_png_path(dir, stem);
    fs::write(&target, png_bytes).map_err(|source| SaveError::WriteFailed {
        path: target.clone(),
        source,
    })?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_with_collision_suffix_and_creates_missing_dir() {
        let temp_root = std::env::temp_dir().join(format!(
            "snip_tool_save_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_root);

        let p1 = save_png_bytes_to_dir(&temp_root, "Snip_20261122_233000", b"png1")
            .expect("first save");
        let p2 = save_png_bytes_to_dir(&temp_root, "Snip_20261122_233000", b"png2")
            .expect("second save");

        assert_eq!(p1.file_name().unwrap(), "Snip_20261122_233000.png");
        assert_eq!(p2.file_name().unwrap(), "Snip_20261122_233000_1.png");
        assert_eq!(fs::read(&p1).unwrap(), b"png1");
        assert_eq!(fs::read(&p2).unwrap(), b"png2");

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn resolve_save_dir_uses_configured_override() {
        let dir = resolve_save_dir(Some(r"C:\Temp\MySnips")).expect("configured dir");
        assert_eq!(dir, PathBuf::from(r"C:\Temp\MySnips"));
    }
}
