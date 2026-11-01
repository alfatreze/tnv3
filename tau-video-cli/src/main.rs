use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
};
use tau_video::c_encoder::{self, Options, Tools};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod publication {
    use super::*;
    use rustix::{
        fd::OwnedFd,
        fs::{self as rfs, AtFlags, Mode, OFlags, RenameFlags},
    };
    use std::fs;

    const PAYLOAD: &str = "payload.tnv3";

    pub struct ProtectedStage {
        parent_fd: OwnedFd,
        stage_fd: OwnedFd,
        stage_name: OsString,
        payload: Option<fs::File>,
        published: bool,
    }

    impl ProtectedStage {
        pub fn create(parent: &Path) -> Result<Self, String> {
            let parent_fd = rfs::open(
                parent,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| format!("Could not open output directory safely: {e}"))?;
            let parent_stat = rfs::fstat(&parent_fd)
                .map_err(|e| format!("Could not inspect output directory: {e}"))?;
            let parent_mode = parent_stat.st_mode & 0o7777;
            if parent_mode & 0o022 != 0 {
                return Err(
                    "Refusing publication in a group/world-writable output directory; choose a trusted directory that is not writable by other users"
                        .into(),
                );
            }

            for _ in 0..100 {
                let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
                let stage_name =
                    OsString::from(format!(".tau-publish-{}-{id}", std::process::id()));
                match rfs::mkdirat(&parent_fd, &stage_name, Mode::from_bits_retain(0o700)) {
                    Ok(()) => {
                        let stage_fd = match rfs::openat(
                            &parent_fd,
                            &stage_name,
                            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                            Mode::empty(),
                        ) {
                            Ok(fd) => fd,
                            Err(error) => {
                                let _ = rfs::unlinkat(&parent_fd, &stage_name, AtFlags::REMOVEDIR);
                                return Err(format!(
                                    "Could not open private staging directory: {error}"
                                ));
                            }
                        };
                        let stage_stat = match rfs::fstat(&stage_fd) {
                            Ok(stat) => stat,
                            Err(error) => {
                                let _ = rfs::unlinkat(&parent_fd, &stage_name, AtFlags::REMOVEDIR);
                                return Err(format!(
                                    "Could not inspect staging directory: {error}"
                                ));
                            }
                        };
                        let stage_mode = stage_stat.st_mode & 0o777;
                        if stage_mode & 0o077 != 0 || stage_mode & 0o700 != 0o700 {
                            let _ = rfs::unlinkat(&parent_fd, &stage_name, AtFlags::REMOVEDIR);
                            return Err(format!(
                                "Filesystem did not create a private staging directory (mode {stage_mode:o})"
                            ));
                        }
                        let payload_fd = match rfs::openat(
                            &stage_fd,
                            PAYLOAD,
                            OFlags::RDWR
                                | OFlags::CREATE
                                | OFlags::EXCL
                                | OFlags::CLOEXEC
                                | OFlags::NOFOLLOW,
                            Mode::from_bits_retain(0o600),
                        ) {
                            Ok(fd) => fd,
                            Err(error) => {
                                let _ = rfs::unlinkat(&parent_fd, &stage_name, AtFlags::REMOVEDIR);
                                return Err(format!("Could not create staged output: {error}"));
                            }
                        };
                        return Ok(Self {
                            parent_fd,
                            stage_fd,
                            stage_name,
                            payload: Some(fs::File::from(payload_fd)),
                            published: false,
                        });
                    }
                    Err(error) if error == rustix::io::Errno::EXIST => continue,
                    Err(error) => {
                        return Err(format!("Could not create staging directory: {error}"));
                    }
                }
            }
            Err("Could not reserve a unique staging directory".into())
        }

        pub fn writer(&mut self) -> &mut fs::File {
            self.payload.as_mut().expect("staged payload exists")
        }

        pub fn publish(mut self, output_name: &OsStr) -> Result<(u64, Option<String>), String> {
            let file = self.payload.as_mut().expect("staged payload exists");
            file.sync_all()
                .map_err(|e| format!("Could not sync staged TNV3: {e}"))?;
            let length = file
                .metadata()
                .map_err(|e| format!("Could not inspect staged TNV3: {e}"))?
                .len();
            rfs::renameat_with(
                &self.stage_fd,
                PAYLOAD,
                &self.parent_fd,
                output_name,
                RenameFlags::NOREPLACE,
            )
            .map_err(|e| {
                format!("Atomic no-replace publish failed; no output was replaced: {e}")
            })?;
            self.published = true;
            let warning = rfs::fsync(&self.parent_fd)
                .err()
                .map(|e| format!("Published, but output-directory sync failed: {e}"));
            Ok((length, warning))
        }
    }

    impl Drop for ProtectedStage {
        fn drop(&mut self) {
            if !self.published {
                let _ = rfs::unlinkat(&self.stage_fd, PAYLOAD, AtFlags::empty());
            }
            let _ = rfs::unlinkat(&self.parent_fd, &self.stage_name, AtFlags::REMOVEDIR);
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod publication {
    use super::*;
    pub struct ProtectedStage;
    impl ProtectedStage {
        pub fn create(_parent: &Path) -> Result<Self, String> {
            Err(
                "Atomic no-replace publication is unsupported on this platform in this prototype"
                    .into(),
            )
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    python: PathBuf,
    script: PathBuf,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    tvc_encoder: PathBuf,
    tnv3_tool: PathBuf,
    qp: u8,
    jobs: usize,
}

fn usage() -> &'static str {
    "Usage: tau-codecs-cli encode INPUT OUTPUT --python PATH --script PATH --ffmpeg PATH --ffprobe PATH --tvc-encoder PATH --tnv3-tool PATH [--qp 0..51] [--jobs N]\n\nEncodes video-only TNV3 using the existing C-reference helper pipeline. It does not create an MP3 sidecar or an audio/video pair. OUTPUT must not already exist."
}

fn parse_args<I>(iter: I) -> Result<Option<Args>, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut it = iter.into_iter();
    let Some(command) = it.next() else {
        return Err(usage().into());
    };
    if command == OsStr::new("--help") || command == OsStr::new("-h") {
        return Ok(None);
    }
    if command != OsStr::new("encode") {
        return Err(usage().into());
    }
    let input = PathBuf::from(it.next().ok_or_else(|| usage().to_owned())?);
    let output = PathBuf::from(it.next().ok_or_else(|| usage().to_owned())?);
    let mut python = None;
    let mut script = None;
    let mut ffmpeg = None;
    let mut ffprobe = None;
    let mut tvc_encoder = None;
    let mut tnv3_tool = None;
    let mut qp = 30;
    let mut jobs = 1;
    while let Some(key) = it.next() {
        let key_text = key
            .to_str()
            .ok_or_else(|| format!("option name must be UTF-8: {}", key.to_string_lossy()))?;
        let value = it
            .next()
            .ok_or_else(|| format!("missing value for {key_text}"))?;
        match key_text {
            "--python" => python = Some(PathBuf::from(value)),
            "--script" => script = Some(PathBuf::from(value)),
            "--ffmpeg" => ffmpeg = Some(PathBuf::from(value)),
            "--ffprobe" => ffprobe = Some(PathBuf::from(value)),
            "--tvc-encoder" => tvc_encoder = Some(PathBuf::from(value)),
            "--tnv3-tool" => tnv3_tool = Some(PathBuf::from(value)),
            "--qp" => {
                qp = value
                    .to_str()
                    .ok_or("QP must be UTF-8")?
                    .parse()
                    .map_err(|_| "QP must be an integer from 0 to 51")?
            }
            "--jobs" => {
                jobs = value
                    .to_str()
                    .ok_or("jobs must be UTF-8")?
                    .parse()
                    .map_err(|_| "jobs must be a positive integer")?
            }
            _ => return Err(format!("unknown option: {key_text}\n{}", usage())),
        }
    }
    if qp > 51 || jobs == 0 {
        return Err("QP must be 0–51 and jobs must be positive".into());
    }
    Ok(Some(Args {
        input,
        output,
        python: python.ok_or("--python is required")?,
        script: script.ok_or("--script is required")?,
        ffmpeg: ffmpeg.ok_or("--ffmpeg is required")?,
        ffprobe: ffprobe.ok_or("--ffprobe is required")?,
        tvc_encoder: tvc_encoder.ok_or("--tvc-encoder is required")?,
        tnv3_tool: tnv3_tool.ok_or("--tnv3-tool is required")?,
        qp,
        jobs,
    }))
}

fn required_file(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "{label} does not exist or is not a file: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn run(_args: Args) -> Result<(), String> {
    Err("Atomic no-replace publication is unsupported on this platform in this prototype".into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run(args: Args) -> Result<(), String> {
    required_file(&args.input, "Input")?;
    for (path, label) in [
        (&args.python, "Python"),
        (&args.script, "Encoder script"),
        (&args.ffmpeg, "FFmpeg"),
        (&args.ffprobe, "FFprobe"),
        (&args.tvc_encoder, "TVC encoder"),
        (&args.tnv3_tool, "TNV3 tool"),
    ] {
        required_file(path, label)?;
    }
    if args.output.exists() {
        return Err(format!("Output already exists: {}", args.output.display()));
    }
    let parent = args
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "Output directory does not exist: {}",
            parent.display()
        ));
    }

    let tools = Tools {
        python: args.python,
        script: args.script,
        ffmpeg: args.ffmpeg,
        ffprobe: args.ffprobe,
        tvc_encoder: Some(args.tvc_encoder),
        tnv3_tool: Some(args.tnv3_tool),
    };
    let output_name = args
        .output
        .file_name()
        .ok_or("Output path must include a file name")?;
    let mut staged = publication::ProtectedStage::create(parent)?;
    let report = c_encoder::encode_video(
        &tools,
        &args.input,
        staged.writer(),
        Options {
            qp: args.qp,
            jobs: args.jobs,
            ..Options::default()
        },
        |done, total| eprintln!("segments: {done}/{total}"),
    )?;
    let (byte_size, sync_warning) = staged.publish(output_name)?;
    println!(
        "Wrote video-only TNV3: {} frames, level {}, {} picture(s) raised for level limits, {} bytes",
        report.frames, report.level, report.reencoded, byte_size
    );
    println!("Output: {}", args.output.display());
    if !report.diagnostics.is_empty() {
        eprintln!("Encoder diagnostics:\n{}", report.diagnostics);
    }
    if let Some(warning) = sync_warning {
        eprintln!("Warning: {warning}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match parse_args(env::args_os().skip(1)) {
        Ok(None) => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
        Ok(Some(args)) => match run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn required_options() -> Vec<OsString> {
        [
            "--python",
            "/tools/python",
            "--script",
            "/tools/encode_film.py",
            "--ffmpeg",
            "/tools/ffmpeg",
            "--ffprobe",
            "/tools/ffprobe",
            "--tvc-encoder",
            "/tools/tvc_enc",
            "--tnv3-tool",
            "/tools/tnv3",
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }

    #[test]
    fn parses_required_dependencies_and_defaults() {
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        let args = parse_args(argv).unwrap().unwrap();
        assert_eq!(args.qp, 30);
        assert_eq!(args.jobs, 1);
        assert_eq!(args.output, PathBuf::from("out.tnv3"));
    }

    #[test]
    fn rejects_invalid_qp_jobs_and_unknown_options() {
        for (option, value) in [("--qp", "52"), ("--jobs", "0"), ("--strange", "x")] {
            let mut argv = vec!["encode".into(), "in".into(), "out".into()];
            argv.extend(required_options());
            argv.extend([option.into(), value.into()]);
            assert!(parse_args(argv).is_err());
        }
    }

    #[test]
    fn help_does_not_require_dependencies() {
        assert_eq!(parse_args([OsString::from("--help")]).unwrap(), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn preserves_non_utf8_positional_paths() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let input = OsString::from_vec(b"in-\xff.mp4".to_vec());
        let output = OsString::from_vec(b"out-\xfe.tnv3".to_vec());
        let mut argv = vec![OsString::from("encode"), input, output];
        argv.extend(required_options());
        let args = parse_args(argv).unwrap().unwrap();
        assert_eq!(args.input.as_os_str().as_bytes(), b"in-\xff.mp4");
        assert_eq!(args.output.as_os_str().as_bytes(), b"out-\xfe.tnv3");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn private_test_dir() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("tau-publish-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn parent_directory_must_not_be_group_or_world_writable_even_with_sticky_bit() {
        use std::os::unix::fs::PermissionsExt;

        for mode in [0o0777, 0o1777] {
            let dir = private_test_dir();
            fs::set_permissions(&dir, fs::Permissions::from_mode(mode)).unwrap();
            let result = publication::ProtectedStage::create(&dir);
            assert!(matches!(result, Err(error) if error.contains("group/world-writable")));
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
            fs::remove_dir(dir).unwrap();
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn private_parent_directory_is_accepted() {
        let dir = private_test_dir();
        let stage = publication::ProtectedStage::create(&dir).unwrap();
        drop(stage);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir(dir).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn atomic_publish_does_not_clobber_existing_output() {
        use std::io::Write;
        let dir = private_test_dir();
        let output = dir.join("existing.tnv3");
        fs::write(&output, b"old output").unwrap();
        let mut stage = publication::ProtectedStage::create(&dir).unwrap();
        stage.writer().write_all(b"new output").unwrap();
        assert!(stage.publish(OsStr::new("existing.tnv3")).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"old output");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn atomic_publish_does_not_replace_symlink_or_target() {
        use std::io::Write;
        use std::os::unix::fs::symlink;
        let dir = private_test_dir();
        let target = dir.join("target.bin");
        let output = dir.join("linked.tnv3");
        fs::write(&target, b"keep me").unwrap();
        symlink(&target, &output).unwrap();
        let mut stage = publication::ProtectedStage::create(&dir).unwrap();
        stage.writer().write_all(b"replacement").unwrap();
        assert!(stage.publish(OsStr::new("linked.tnv3")).is_err());
        assert!(
            fs::symlink_metadata(&output)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), b"keep me");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn failed_before_publish_leaves_no_partial_final_file() {
        use std::io::Write;
        let dir = private_test_dir();
        let output = dir.join("never-published.tnv3");
        let mut stage = publication::ProtectedStage::create(&dir).unwrap();
        stage.writer().write_all(b"partial staging bytes").unwrap();
        drop(stage);
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir(dir).unwrap();
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[test]
    fn unsupported_platform_publication_fails_explicitly() {
        let error = publication::ProtectedStage::create(Path::new("."))
            .err()
            .expect("unsupported platforms must fail closed");
        assert!(error.contains("unsupported on this platform"));
    }
}
