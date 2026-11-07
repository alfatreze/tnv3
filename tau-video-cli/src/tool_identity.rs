#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Read;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_IDENTITY_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn hash_identity_file(path: &Path) -> Result<String, String> {
    hash_identity_file_after_open(path, || {})
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn hash_identity_file_after_open<F>(path: &Path, after_open: F) -> Result<String, String>
where
    F: FnOnce(),
{
    use rustix::fs::{self as rfs, Mode, OFlags};
    use sha2::{Digest, Sha256};

    let fd = rfs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| format!("Could not safely open '{}': {error}", path.display()))?;
    let stat = rfs::fstat(&fd)
        .map_err(|error| format!("Could not inspect '{}': {error}", path.display()))?;
    if stat.st_mode & 0o170000 != 0o100000 {
        return Err(format!("'{}' is not a regular file", path.display()));
    }
    if stat.st_size < 0 || stat.st_size as u64 > MAX_IDENTITY_FILE_BYTES {
        return Err(format!(
            "'{}' exceeds the {} byte identity limit",
            path.display(),
            MAX_IDENTITY_FILE_BYTES
        ));
    }
    let initial_size = stat.st_size;
    let initial_mtime = stat.st_mtime;
    let initial_mtime_nsec = stat.st_mtime_nsec;
    let initial_ctime = stat.st_ctime;
    let initial_ctime_nsec = stat.st_ctime_nsec;
    let mut file = std::fs::File::from(fd);
    after_open();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_IDENTITY_FILE_BYTES {
            return Err(format!(
                "'{}' exceeded the {} byte identity limit while reading",
                path.display(),
                MAX_IDENTITY_FILE_BYTES
            ));
        }
        hasher.update(&buffer[..count]);
    }
    let after = rfs::fstat(&file)
        .map_err(|error| format!("Could not re-inspect '{}': {error}", path.display()))?;
    if after.st_size != initial_size
        || after.st_mtime != initial_mtime
        || after.st_mtime_nsec != initial_mtime_nsec
        || after.st_ctime != initial_ctime
        || after.st_ctime_nsec != initial_ctime_nsec
        || total != initial_size as u64
    {
        return Err(format!(
            "'{}' changed while its identity was being read; retry with a stable file",
            path.display()
        ));
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn hash_identity_file(path: &Path) -> Result<String, String> {
    Err(format!(
        "tool identity is unsupported on this platform; refusing to hash '{}'",
        path.display()
    ))
}

pub(crate) fn tool_identity_command(
    argv: &[OsString],
) -> Result<Option<serde_json::Value>, (u8, String)> {
    if argv
        .first()
        .is_none_or(|arg| arg != OsStr::new("tool-identity"))
    {
        return Ok(None);
    }
    if argv.get(1).is_none_or(|arg| arg != OsStr::new("--json")) {
        return Err((
            2,
            "tool-identity requires --json immediately after the command".into(),
        ));
    }
    let mut tools = serde_json::Map::new();
    let mut selected = Vec::new();
    let mut it = argv[2..].iter();
    while let Some(option) = it.next() {
        let name = match option.to_str() {
            Some("--ffmpeg") => "ffmpeg",
            Some("--ffprobe") => "ffprobe",
            Some("--tvc-encoder") => "tvc_enc",
            Some("--tnv3-tool") => "tnv3",
            _ => {
                return Err((
                    2,
                    format!("unknown tool-identity option: {}", option.to_string_lossy()),
                ));
            }
        };
        if selected
            .iter()
            .any(|(selected_name, _)| *selected_name == name)
        {
            return Err((2, format!("duplicate tool path for {name}")));
        }
        let Some(path) = it.next() else {
            return Err((
                2,
                format!(
                    "missing path after --{}",
                    option.to_string_lossy().trim_start_matches("--")
                ),
            ));
        };
        let Some(path_text) = path.to_str() else {
            return Err((2, "tool-identity paths must be UTF-8".into()));
        };
        if path_text.starts_with("--") {
            return Err((
                2,
                "tool-identity path values may not start with '--'".into(),
            ));
        }
        selected.push((name, PathBuf::from(path_text)));
    }
    if selected.is_empty() {
        return Err((
            2,
            "tool-identity requires at least one explicit tool path".into(),
        ));
    }
    for (name, path) in selected {
        let sha256 = hash_identity_file(&path).map_err(|message| (3, message))?;
        tools.insert(
            name.to_owned(),
            serde_json::json!({
                "selected_path": path.to_string_lossy(),
                "sha256": sha256,
                "version": null,
                "version_status": "not_exposed"
            }),
        );
    }
    Ok(Some(serde_json::json!({
        "schema": 1,
        "identity": "sha256_file_contents",
        "scope": "metadata-stability-checked bytes read from explicitly selected file; this is not an immutable snapshot and does not bind a later execution",
        "tools": tools
    })))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    #[test]
    fn rejects_mutation_during_read() {
        let dir = crate::unique_test_dir("tau-identity-mutation");
        let file = dir.join("tool");
        std::fs::write(&file, b"before!").unwrap();
        let result = hash_identity_file_after_open(&file, || {
            std::fs::write(&file, b"changed").unwrap();
        });
        assert!(
            result
                .unwrap_err()
                .contains("changed while its identity was being read")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
