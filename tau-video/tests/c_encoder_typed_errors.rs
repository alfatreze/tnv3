#![cfg(feature = "c-encoder")]

use std::{io::Cursor, path::PathBuf};
use tau_video::c_encoder::{EncodeError, Options, Tools, encode_video_typed};

fn tools(python: PathBuf) -> Tools {
    Tools {
        python,
        script: "unused.py".into(),
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
        tvc_encoder: None,
        tnv3_tool: None,
    }
}

#[test]
fn typed_api_distinguishes_invalid_input_from_spawn_failure() {
    let mut out = Cursor::new(Vec::new());
    let invalid = encode_video_typed(
        &tools("unused-python".into()),
        PathBuf::from("source").as_path(),
        &mut out,
        Options {
            jobs: 0,
            ..Options::default()
        },
        |_, _| {},
    )
    .unwrap_err();
    assert!(matches!(invalid, EncodeError::InvalidInput(_)));

    let spawn = encode_video_typed(
        &tools("/definitely/missing/tau-python".into()),
        PathBuf::from("source").as_path(),
        &mut out,
        Options::default(),
        |_, _| {},
    )
    .unwrap_err();
    assert!(matches!(spawn, EncodeError::Spawn(_)));
}

#[test]
fn typed_api_classifies_nonzero_codec_exit() {
    #[cfg(unix)]
    {
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            time::{SystemTime, UNIX_EPOCH},
        };
        let dir = std::env::temp_dir().join(format!(
            "tau-typed-error-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let python = dir.join("fake-python");
        fs::write(&python, "#!/bin/sh\necho codec-broke >&2\nexit 9\n").unwrap();
        let mut perms = fs::metadata(&python).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&python, perms).unwrap();
        let result = encode_video_typed(
            &tools(python),
            PathBuf::from("source").as_path(),
            &mut Vec::new(),
            Options::default(),
            |_, _| {},
        );
        assert!(matches!(result, Err(EncodeError::ProcessFailure { .. })));
        assert!(result.unwrap_err().to_string().contains("codec-broke"));
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn typed_api_preserves_writer_io_failure() {
    use std::{
        fs,
        io::{self, Write},
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture write failure",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = std::env::temp_dir().join(format!(
        "tau-typed-writer-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let python = dir.join("fake-python");
    fs::write(&python, "#!/bin/sh\nprintf data > \"$3\"\nprintf '{\"valid\":true,\"frames\":1,\"level\":1,\"pictures_raised_for_level\":0}' > \"${3%.tnv3}.json\"\n").unwrap();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o700)).unwrap();
    let error = encode_video_typed(
        &tools(python),
        PathBuf::from("source").as_path(),
        &mut FailingWriter,
        Options::default(),
        |_, _| {},
    )
    .unwrap_err();
    match error {
        EncodeError::Io { operation, source } => {
            assert_eq!(operation, "Could not copy encoded video");
            assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
        }
        other => panic!("unexpected typed failure: {other}"),
    }
    fs::remove_dir_all(dir).unwrap();
}
