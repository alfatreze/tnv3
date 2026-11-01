#![cfg(feature = "c-encoder")]
use std::{
    fs,
    io::Cursor,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tau_video::c_encoder::{FrameTiming, Options, Tools, encode_video};

#[test]
fn adapter_propagates_report_progress_diagnostics_and_failures() {
    let Some(python) = std::env::var_os("TAU_TEST_PYTHON") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "tau wrapper test {} {}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let script = dir.join("fake film.py");
    fs::write(&script, r#"import sys, json, pathlib
out = pathlib.Path(sys.argv[2])
if '--fps-round-down' not in sys.argv or '--fps-eof-pass' not in sys.argv: sys.exit(4)
print('encoded 2 / 3 segments', flush=True)
print('sample diagnostic', file=sys.stderr)
out.write_bytes(b'example video')
out.with_suffix('.json').write_text(json.dumps({'frames':3,'level':1,'pictures_raised_for_level':2,'valid':True}))
"#).unwrap();
    let tools = Tools {
        python: python.into(),
        script: script.clone(),
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
        tvc_encoder: None,
        tnv3_tool: None,
    };
    let options = Options {
        qp: 30,
        jobs: 2,
        timing: FrameTiming::RoundDownEofPass,
    };
    let mut output = Cursor::new(Vec::new());
    let mut progress = Vec::new();
    let report = encode_video(
        &tools,
        Path::new("input with spaces"),
        &mut output,
        options,
        |done, total| progress.push((done, total)),
    )
    .unwrap();
    assert_eq!(output.into_inner(), b"example video");
    assert_eq!((report.frames, report.level, report.reencoded), (3, 1, 2));
    assert!(report.diagnostics.contains("sample diagnostic"));
    assert_eq!(progress, [(2, 3)]);
    fs::write(
        &script,
        "import sys\nprint('broken', file=sys.stderr)\nsys.exit(9)\n",
    )
    .unwrap();
    let mut untouched = Vec::new();
    assert!(
        encode_video(
            &tools,
            Path::new("source"),
            &mut untouched,
            options,
            |_, _| {}
        )
        .unwrap_err()
        .contains("broken")
    );
    assert!(untouched.is_empty());
    assert!(
        encode_video(
            &tools,
            Path::new("source"),
            &mut untouched,
            Options { jobs: 0, ..options },
            |_, _| {}
        )
        .is_err()
    );
    fs::remove_dir_all(dir).unwrap();
}
