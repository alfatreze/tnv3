//! Explicitly scoped TNV3 checks; not a complete conformance checker.
use serde_json::{Value, json};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::Write,
    path::Path,
    process::ExitCode,
};
use tau_video::{
    ContainerPreviewStatus, Error, Tnv3Reader, check_chunk_structure, check_container_preview,
    check_subtitle_structure,
};

fn problem(error: &Error) -> Value {
    match error {
        Error::Invalid {
            code,
            offset,
            message,
        } => json!({"code":code,"offset":offset,"message":message}),
        Error::LimitExceeded { offset, message } => {
            json!({"code":"E-TNV3-LIMIT","offset":offset,"message":message})
        }
        Error::Io(e) => json!({"code":"E-TNV3-IO","offset":null,"message":e.to_string()}),
        _ => json!({"code":"unknown","offset":null,"message":error.to_string()}),
    }
}

fn scoped_header_failure(scope: &str, error: &Error) -> Option<Value> {
    if scope == "container-preview" {
        return Some(preview_error(error));
    }
    if matches!(error, Error::Io(_)) {
        return None;
    }
    let (findings_count, findings, limit) = match error {
        Error::Invalid { .. } => (1, json!([problem(error)]), None),
        Error::LimitExceeded { .. } => (0, json!([]), Some(problem(error))),
        _ => (1, json!([problem(error)]), None),
    };
    let mut report = json!({
        "schema":1,"command":"check","scope":scope,"complete":false,
        "scan_complete":false,"full_conformance_checked":false,
        "findings_count":findings_count,"findings_omitted":0,"findings":findings
    });
    if scope == "cross-frame" || scope == "chunk-structure" {
        report["pictures_attempted"] = Value::Null;
        report["pictures_passed"] = Value::Null;
    }
    if let Some(limit) = limit {
        report["limit"] = limit;
    }
    Some(report)
}

pub fn run(args: &[OsString]) -> ExitCode {
    let json_mode = args.iter().any(|arg| arg == OsStr::new("--json"));
    let result = check(args);
    let (status, value) = match result {
        Ok(value) => (
            if value["status"] == "INVALID" {
                1
            } else if value["status"] == "INDETERMINATE" {
                3
            } else if value.get("limit").is_some() || value["findings_count"] != 0 {
                1
            } else {
                0
            },
            value,
        ),
        Err((code, message)) => (
            code,
            json!({"schema":1,"command":"check","complete":false,"scan_complete":false,"error":{"code":code,"message":message}}),
        ),
    };
    if json_mode {
        if writeln!(std::io::stdout().lock(), "{value}").is_err() {
            eprintln!("Could not write check report");
            return ExitCode::from(3);
        }
    } else if value.get("status").is_some() {
        let line = if value["quick"] == true {
            format!(
                "quick-container: {}. Checked {} of {} attempted sampled picture chunks; TVC/TAC payload decoding and full conformance were not checked.",
                value["status"].as_str().unwrap_or("INDETERMINATE"),
                value["pictures_checked"],
                value["pictures_attempted"]
            )
        } else {
            format!(
                "container-preview: {}. Container structure only; TVC/TAC payload decoding and full conformance were not checked.",
                value["status"].as_str().unwrap_or("INDETERMINATE")
            )
        };
        if super::write_stdout_line(&line).is_err() {
            eprintln!("Could not write check report");
            return ExitCode::from(3);
        }
        for finding in value["findings"].as_array().into_iter().flatten() {
            if writeln!(
                std::io::stderr().lock(),
                "{} at byte {}: {}",
                finding["code"].as_str().unwrap_or("invalid"),
                finding["offset"]
                    .as_u64()
                    .map_or_else(|| "unknown".to_string(), |v| v.to_string()),
                finding["message"]
                    .as_str()
                    .unwrap_or("check did not complete")
            )
            .is_err()
            {
                return ExitCode::from(3);
            }
        }
        if value["status"] == "INDETERMINATE" {
            let detail = value["indeterminate"]["message"]
                .as_str()
                .or_else(|| value["limit"]["message"].as_str())
                .or_else(|| value["findings"][0]["message"].as_str())
                .unwrap_or("check could not complete");
            if writeln!(std::io::stderr().lock(), "INDETERMINATE: {detail}").is_err() {
                return ExitCode::from(3);
            }
        }
    } else if let Some(error) = value.get("error") {
        if writeln!(
            std::io::stderr().lock(),
            "{}",
            error["message"].as_str().unwrap_or("Check failed")
        )
        .is_err()
        {
            return ExitCode::from(3);
        }
    } else if let Some(limit) = value.get("limit") {
        if writeln!(
            std::io::stderr().lock(),
            "{}: {}",
            limit["code"].as_str().unwrap_or("limit"),
            limit["message"].as_str().unwrap_or("Check limit reached")
        )
        .is_err()
        {
            return ExitCode::from(3);
        }
    } else {
        let line = format!(
            "{} check: {} finding(s). Full conformance was not checked.",
            value["scope"].as_str().unwrap_or("Scoped"),
            value["findings_count"]
        );
        if super::write_stdout_line(&line).is_err() {
            eprintln!("Could not write check report");
            return ExitCode::from(3);
        }
        for finding in value["findings"].as_array().into_iter().flatten() {
            if writeln!(
                std::io::stderr().lock(),
                "{} at byte {}: {}",
                finding["code"].as_str().unwrap_or("invalid"),
                finding["offset"],
                finding["message"].as_str().unwrap_or("invalid")
            )
            .is_err()
            {
                return ExitCode::from(3);
            }
        }
    }
    ExitCode::from(status)
}

fn check(args: &[OsString]) -> Result<Value, (u8, String)> {
    let usage = || {
        (2, "Usage: tau-codecs-cli check FILE (--quick | --scope chunk-structure|cross-frame|subtitle-structure|container-preview) [--json]; complete conformance checking is not implemented".to_string())
    };
    if args.len() < 3 || args.len() > 5 {
        return Err(usage());
    }
    let input = &args[1];
    if input.to_string_lossy().starts_with('-') {
        return Err(usage());
    }
    let mut scope: Option<&str> = None;
    let mut json_seen = false;
    let mut quick = false;
    let mut at = 2;
    while at < args.len() {
        if args[at] == OsStr::new("--json") && !json_seen {
            json_seen = true;
            at += 1;
        } else if args[at] == OsStr::new("--quick") && !quick {
            quick = true;
            at += 1;
        } else if args[at] == OsStr::new("--scope")
            && scope.is_none()
            && args.get(at + 1).is_some_and(|v| {
                v == OsStr::new("chunk-structure")
                    || v == OsStr::new("cross-frame")
                    || v == OsStr::new("subtitle-structure")
                    || v == OsStr::new("container-preview")
            })
        {
            scope = args.get(at + 1).and_then(|value| value.to_str());
            at += 2;
        } else {
            return Err(usage());
        }
    }
    if quick == scope.is_some() {
        return Err(usage());
    }
    let selected_scope = if quick {
        "quick-container"
    } else {
        scope.unwrap()
    };
    let file = if quick || scope == Some("container-preview") {
        match open_preview_input(Path::new(input)) {
            Ok(file) => file,
            Err(error) if quick => {
                return Ok(quick_failure(error.code, None, error.message, false));
            }
            Err(error) => return Ok(preview_failure(error.code, None, error.message, false)),
        }
    } else {
        match File::open(input) {
            Ok(file) => file,
            Err(e) => return Err((3, format!("Could not open input: {e}"))),
        }
    };
    if quick || scope == Some("container-preview") {
        match file.metadata() {
            Ok(metadata) if metadata.len() < 64 => {
                if quick {
                    return Ok(quick_failure(
                        "E-TNV3-001",
                        Some(0),
                        "File is shorter than the 64-byte TNV3 header.".into(),
                        true,
                    ));
                }
                return Ok(preview_failure(
                    "E-TNV3-001",
                    Some(0),
                    "File is shorter than the 64-byte TNV3 header.".into(),
                    true,
                ));
            }
            Err(e) if quick => {
                return Ok(quick_failure("E-TNV3-IO", None, e.to_string(), false));
            }
            Err(e) => return Ok(preview_failure("E-TNV3-IO", None, e.to_string(), false)),
            _ => {}
        }
    }
    let mut reader = match Tnv3Reader::open(file) {
        Ok(reader) => reader,
        Err(e) => {
            if quick {
                return Ok(quick_failure_from_error(&e));
            }
            if let Some(report) = scoped_header_failure(selected_scope, &e) {
                return Ok(report);
            }
            return Err((
                match &e {
                    Error::Io(io) if io.kind() != std::io::ErrorKind::UnexpectedEof => 3,
                    _ => 1,
                },
                e.to_string(),
            ));
        }
    };
    if quick {
        return quick_container_check(&mut reader);
    }
    if scope == Some("container-preview") {
        let report = check_container_preview(&mut reader);
        let status = match report.status {
            ContainerPreviewStatus::Valid => "VALID",
            ContainerPreviewStatus::Invalid => "INVALID",
            ContainerPreviewStatus::Indeterminate => "INDETERMINATE",
        };
        return Ok(
            json!({"schema":1,"command":"check","scope":"container-preview","status":status,"complete":false,"scan_complete":report.status == ContainerPreviewStatus::Valid,"full_conformance_checked":false,"payload_decoding_checked":false,"input_stability_required":true,"pictures_checked":report.pictures_checked,"tracks_checked":report.tracks_checked,"variants_checked":report.variants_checked,"events_checked":report.events_checked,"glyphs_checked":report.glyphs_checked,"findings_count":report.findings.len(),"findings_omitted":0,"findings":report.findings.iter().map(|f| json!({"code":f.code,"offset":f.offset,"message":f.message})).collect::<Vec<_>>()}),
        );
    }
    if scope == Some("subtitle-structure") {
        return match check_subtitle_structure(&mut reader) {
            Ok(report) => Ok(
                json!({"schema":1,"command":"check","scope":"subtitle-structure","complete":false,"scan_complete":true,"full_conformance_checked":false,"tracks_checked":report.tracks_checked,"variants_checked":report.variants_checked,"events_checked":report.events_checked,"glyphs_checked":report.glyphs_checked,"findings_count":0,"findings_omitted":0,"findings":[]}),
            ),
            Err(error @ Error::Invalid { .. }) => Ok(
                json!({"schema":1,"command":"check","scope":"subtitle-structure","complete":false,"scan_complete":false,"full_conformance_checked":false,"findings_count":1,"findings_omitted":0,"findings":[problem(&error)]}),
            ),
            Err(error @ Error::LimitExceeded { .. }) => Ok(
                json!({"schema":1,"command":"check","scope":"subtitle-structure","complete":false,"scan_complete":false,"full_conformance_checked":false,"findings_count":0,"findings_omitted":0,"findings":[],"limit":problem(&error)}),
            ),
            Err(error) => Err((
                if matches!(error, Error::Io(_)) { 3 } else { 1 },
                error.to_string(),
            )),
        };
    }
    if scope == Some("cross-frame") {
        let frame_count = reader.info().frame_count;
        return match reader.check_cross_frame() {
            Ok(()) => Ok(
                json!({"schema":1,"command":"check","scope":"cross-frame","complete":false,"scan_complete":true,"full_conformance_checked":false,"pictures_attempted":frame_count,"pictures_passed":frame_count,"findings_count":0,"findings_omitted":0,"findings":[]}),
            ),
            Err(error @ Error::Invalid { .. }) => Ok(
                json!({"schema":1,"command":"check","scope":"cross-frame","complete":false,"scan_complete":false,"full_conformance_checked":false,"pictures_attempted":null,"pictures_passed":null,"findings_count":1,"findings_omitted":0,"findings":[problem(&error)]}),
            ),
            Err(error) => Err((
                if matches!(error, Error::Io(_)) { 3 } else { 1 },
                error.to_string(),
            )),
        };
    }
    let report = check_chunk_structure(&mut reader)
        .map_err(|e| (if matches!(e, Error::Io(_)) { 3 } else { 1 }, e.to_string()))?;
    let findings_count = report.findings.len() as u32 + report.findings_omitted;
    let findings: Vec<Value> = report
        .findings
        .iter()
        .map(|f| {
            let mut entry = problem(&f.error);
            entry["picture_index"] = json!(f.picture_index);
            entry
        })
        .collect();
    Ok(
        json!({"schema":1,"command":"check","scope":"chunk-structure","complete":false,"scan_complete":true,"full_conformance_checked":false,"pictures_attempted":report.pictures_attempted,"pictures_passed":report.pictures_checked,"findings_count":findings_count,"findings_omitted":report.findings_omitted,"findings":findings}),
    )
}

/// Check the first, middle and final picture targets for slow-card triage.
/// `read_picture` may read additional records; this report only names target
/// indices passed to it and makes no claim about all index reads.
fn quick_container_check(reader: &mut Tnv3Reader<File>) -> Result<Value, (u8, String)> {
    let count = reader.info().frame_count;
    let mut indices = Vec::with_capacity(3);
    if count != 0 {
        for index in [0, count / 2, count - 1] {
            if !indices.contains(&index) {
                indices.push(index);
            }
        }
    }
    let mut findings = Vec::new();
    let mut checked = 0u32;
    let mut attempted = Vec::new();
    let mut indeterminate = None;
    for &index in &indices {
        attempted.push(index);
        match reader.read_picture(index) {
            Ok(_) => checked += 1,
            Err(error @ Error::Io(_)) | Err(error @ Error::LimitExceeded { .. }) => {
                indeterminate.get_or_insert_with(|| problem(&error));
            }
            Err(error) => {
                let mut finding = problem(&error);
                finding["picture_index"] = json!(index);
                findings.push(finding);
            }
        }
    }
    let status = if !findings.is_empty() {
        "INVALID"
    } else if indeterminate.is_some() {
        "INDETERMINATE"
    } else {
        "VALID"
    };
    let mut report = json!({"schema":1,"command":"check","scope":"quick-container","quick":true,"status":status,"complete":false,"scan_complete":false,"full_conformance_checked":false,"payload_decoding_checked":false,"input_stability_required":true,"pictures_attempted":attempted.len(),"pictures_checked":checked,"sampled_picture_indices":indices,"attempted_picture_indices":attempted,"findings_count":findings.len(),"findings_omitted":0,"findings":findings});
    if let Some(problem) = indeterminate { report["indeterminate"] = problem; }
    Ok(report)
}

fn quick_failure(code: &str, offset: Option<u64>, message: String, invalid: bool) -> Value {
    json!({"schema":1,"command":"check","scope":"quick-container","quick":true,"status":if invalid {"INVALID"} else {"INDETERMINATE"},"complete":false,"scan_complete":false,"full_conformance_checked":false,"payload_decoding_checked":false,"input_stability_required":true,"pictures_attempted":0,"pictures_checked":0,"sampled_picture_indices":[],"attempted_picture_indices":[],"findings_count":if invalid {1} else {0},"findings_omitted":0,"findings":if invalid {json!([{"code":code,"offset":offset,"message":message}])} else {json!([])},"indeterminate":if invalid {Value::Null} else {json!({"code":code,"offset":offset,"message":message})}})
}

fn quick_failure_from_error(error: &Error) -> Value {
    match error {
        Error::Invalid { code, offset, message } => quick_failure(code, Some(*offset), message.to_string(), true),
        Error::Io(_) | Error::LimitExceeded { .. } => {
            let detail = problem(error);
            quick_failure(
                detail["code"].as_str().unwrap_or("E-TNV3-LIMIT"),
                detail["offset"].as_u64(),
                detail["message"].as_str().unwrap_or("Check could not complete.").to_string(),
                false,
            )
        }
        _ => quick_failure("E-TNV3-INVALID", None, error.to_string(), true),
    }
}

struct PreviewInputError {
    code: &'static str,
    message: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_preview_input(path: &Path) -> Result<File, PreviewInputError> {
    use rustix::{
        fd::OwnedFd,
        fs::{self as rfs, FileType, Mode, OFlags},
    };

    let fd: OwnedFd = rfs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|e| PreviewInputError {
        code: "E-TNV3-IO",
        message: format!("Could not safely open input: {e}"),
    })?;
    let stat = rfs::fstat(&fd).map_err(|e| PreviewInputError {
        code: "E-TNV3-IO",
        message: format!("Could not inspect opened input: {e}"),
    })?;
    if !FileType::from_raw_mode(stat.st_mode).is_file() {
        return Err(PreviewInputError {
            code: "E-TNV3-NONREGULAR",
            message:
                "Container preview requires a regular file; non-regular input is indeterminate."
                    .into(),
        });
    }
    Ok(File::from(fd))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_preview_input(_path: &Path) -> Result<File, PreviewInputError> {
    Err(PreviewInputError {
        code: "E-TNV3-UNSUPPORTED-OPEN",
        message: "Safe nonblocking regular-file opening is unavailable on this target.".into(),
    })
}

fn preview_failure(code: &str, offset: Option<u64>, message: String, invalid: bool) -> Value {
    json!({"schema":1,"command":"check","scope":"container-preview","status":if invalid {"INVALID"} else {"INDETERMINATE"},"complete":false,"scan_complete":false,"full_conformance_checked":false,"payload_decoding_checked":false,"input_stability_required":true,"pictures_checked":0,"tracks_checked":0,"variants_checked":0,"events_checked":0,"glyphs_checked":0,"findings_count":1,"findings_omitted":0,"findings":[{"code":code,"offset":offset,"message":message}]})
}

fn preview_error(error: &Error) -> Value {
    match error {
        Error::Invalid {
            code,
            offset,
            message,
        } => preview_failure(code, Some(*offset), (*message).into(), true),
        Error::LimitExceeded { offset, message } => {
            preview_failure("E-TNV3-LIMIT", Some(*offset), (*message).into(), false)
        }
        Error::Io(e) => preview_failure("E-TNV3-IO", None, e.to_string(), false),
        other => preview_failure("E-TNV3-UNKNOWN", None, other.to_string(), false),
    }
}
