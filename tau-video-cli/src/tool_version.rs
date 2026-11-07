//! Optional bounded version queries for explicitly selected tools.
//! A reported string is the executable's response, not a compatibility result.

use std::path::Path;

const VERSION_LINE_LIMIT: usize = 512;

pub(crate) fn probe_explicit_tools(tools: &mut serde_json::Value) {
    let Some(map) = tools.as_object_mut() else {
        return;
    };
    for (name, item) in map {
        let Some(path) = item
            .get("selected_path")
            .and_then(serde_json::Value::as_str)
        else {
            item["version"] = serde_json::Value::Null;
            item["version_status"] = "unknown_invalid_path".into();
            continue;
        };
        let (args, available) = match name.as_str() {
            "ffmpeg" | "ffprobe" => (&["-version"][..], true),
            "tvc_enc" | "tnv3" => (&["--version"][..], true),
            _ => (&[][..], false),
        };
        if !available {
            item["version"] = serde_json::Value::Null;
            item["version_status"] = "unknown_unsupported_tool".into();
            continue;
        }
        match query(Path::new(path), args) {
            Ok((Some(0), stdout, stderr)) => {
                let response = first_line(&stdout).or_else(|| first_line(&stderr));
                // A zero exit only says the switch was accepted. The output
                // is arbitrary tool text, not a validated version identity.
                item["version"] = serde_json::Value::Null;
                item["version_response"] =
                    response.clone().map_or(serde_json::Value::Null, Into::into);
                item["version_status"] = if response.is_some() {
                    "response_unvalidated"
                } else {
                    "unknown_empty_output"
                }
                .into();
                item["version_exit_code"] = 0.into();
            }
            Ok((code, stdout, stderr)) => {
                item["version"] = serde_json::Value::Null;
                item["version_status"] = "unknown_nonzero_exit".into();
                item["version_exit_code"] = code.map_or(serde_json::Value::Null, Into::into);
                item["version_diagnostic"] = first_line(&stderr)
                    .or_else(|| first_line(&stdout))
                    .map_or(serde_json::Value::Null, Into::into);
            }
            Err(error) => {
                item["version"] = serde_json::Value::Null;
                item["version_status"] = "unknown_query_failed".into();
                item["version_diagnostic"] = error.into();
            }
        }
    }
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(bytes)
        .lines()
        .next()?
        .trim()
        .to_owned();
    if line.is_empty() {
        return None;
    }
    let clipped: String = line.chars().take(VERSION_LINE_LIMIT).collect();
    Some(clipped)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn query(path: &Path, args: &[&str]) -> Result<(Option<i32>, Vec<u8>, Vec<u8>), String> {
    // `Command::new("tool")` searches PATH. Explicit paths are hashed from
    // the current directory, so resolve relative paths before executing too.
    let resolved;
    let executable = if path.is_relative() {
        resolved = std::env::current_dir()
            .map_err(|e| format!("could not resolve selected tool path: {e}"))?
            .join(path);
        resolved.as_path()
    } else {
        path
    };
    crate::probe_exec::run_version_command(executable, args)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn query(_path: &Path, _args: &[&str]) -> Result<(Option<i32>, Vec<u8>, Vec<u8>), String> {
    Err("bounded version querying is supported only on macOS and Linux".into())
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    fn executable(name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::unique_test_dir("tau-version-probe");
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn reports_successful_version_and_keeps_nonzero_tool_unknown() {
        let ffmpeg = executable("ffmpeg", "echo 'usage: fake tool help text'");
        let tnv3 = executable("tnv3", "echo 'usage: tnv3' >&2; exit 7");
        let mut tools = serde_json::json!({
            "ffmpeg": {"selected_path": ffmpeg},
            "tnv3": {"selected_path": tnv3}
        });
        probe_explicit_tools(&mut tools);
        assert!(tools["ffmpeg"]["version"].is_null());
        assert_eq!(
            tools["ffmpeg"]["version_response"],
            "usage: fake tool help text"
        );
        assert_eq!(tools["ffmpeg"]["version_status"], "response_unvalidated");
        assert!(tools["tnv3"]["version"].is_null());
        assert_eq!(tools["tnv3"]["version_status"], "unknown_nonzero_exit");
        assert_eq!(tools["tnv3"]["version_exit_code"], 7);
    }

    #[test]
    fn rejects_output_over_limit() {
        let noisy = executable("ffmpeg", "yes x | head -c 20000");
        let error = query(&noisy, &["-version"]).unwrap_err();
        assert!(error.contains("output rejected"), "{error}");
    }
}
