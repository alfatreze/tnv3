use std::{
    env,
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc,
    time::Duration,
};
use tau_video::c_encoder::{self, Options, Tools};
use tau_video::experimental_c_orchestrator::{self, Options as RustCOptions, Tools as RustCTools};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn unique_test_dir(prefix: &str) -> PathBuf {
    for _ in 0..100 {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("{prefix}-{}-{id}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!(
                "could not create test directory '{}': {error}",
                path.display()
            ),
        }
    }
    panic!("could not reserve unique test directory for {prefix}")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod publication {
    use super::*;
    use rustix::{
        fd::OwnedFd,
        fs::{self as rfs, AtFlags, Mode, OFlags, RenameFlags},
    };
    use std::fs;

    const PAYLOAD: &str = "payload.tnv3";
    const AUDIO_PAYLOAD: &str = "payload.mp3";

    pub struct ProtectedStage {
        parent: PathBuf,
        parent_fd: OwnedFd,
        stage_fd: OwnedFd,
        stage_name: OsString,
        payload: Option<fs::File>,
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
                            parent: parent.to_path_buf(),
                            parent_fd,
                            stage_fd,
                            stage_name,
                            payload: Some(fs::File::from(payload_fd)),
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

        pub fn audio_path(&self, parent: &Path) -> PathBuf {
            parent.join(&self.stage_name).join(AUDIO_PAYLOAD)
        }

        pub fn sync_audio(&self) -> Result<(), String> {
            let file = rfs::openat(
                &self.stage_fd,
                AUDIO_PAYLOAD,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|e| format!("Could not open staged MP3: {e}"))?;
            rfs::fsync(&file).map_err(|e| format!("Could not sync staged MP3: {e}"))
        }

        pub fn publish_pair(
            mut self,
            video_name: &OsStr,
            audio_name: &OsStr,
        ) -> Result<(u64, u64, Option<String>), String> {
            let video = self.payload.as_mut().expect("staged payload exists");
            video
                .sync_all()
                .map_err(|e| format!("Could not sync staged TNV3: {e}"))?;
            let video_len = video
                .metadata()
                .map_err(|e| format!("Could not inspect staged TNV3: {e}"))?
                .len();
            self.sync_audio()?;
            let audio = rfs::openat(
                &self.stage_fd,
                AUDIO_PAYLOAD,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|e| format!("Could not inspect staged MP3: {e}"))?;
            let audio_len = rfs::fstat(&audio)
                .map_err(|e| format!("Could not inspect staged MP3: {e}"))?
                .st_size as u64;

            let video_path = self.parent_path(video_name);
            let audio_path = self.parent_path(audio_name);
            let (published_video_len, video_copied) = publish_one(
                &self.parent_fd,
                &self.stage_fd,
                PAYLOAD,
                video_name,
                &video_path,
                "TNV3",
                video_len,
            )?;
            let (published_audio_len, audio_copied) = match publish_one(
                &self.parent_fd,
                &self.stage_fd,
                AUDIO_PAYLOAD,
                audio_name,
                &audio_path,
                "MP3",
                audio_len,
            ) {
                Ok(result) => result,
                Err(error) => {
                    return Err(format!(
                        "Audio publish failed after TNV3 was visible at '{}'; pair publication is not atomic. {error}",
                        video_path.display()
                    ));
                }
            };
            let warning = if video_copied || audio_copied {
                Some("This filesystem does not support no-replace rename; outputs were copied into exclusively created final files. Final names may be visible while bytes are being copied, and a failure may leave a partial output. Inspect the named outputs before retrying.".into())
            } else {
                None
            };
            let sync_warning = rfs::fsync(&self.parent_fd)
                .err()
                .map(|e| format!("Published pair, but output-directory sync failed: {e}"));
            let warning = match (warning, sync_warning) {
                (Some(a), Some(b)) => Some(format!("{a} {b}")),
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            };
            Ok((published_video_len, published_audio_len, warning))
        }

        pub fn publish(&mut self, output_name: &OsStr) -> Result<(u64, Option<String>), String> {
            let file = self.payload.as_mut().expect("staged payload exists");
            file.sync_all()
                .map_err(|e| format!("Could not sync staged TNV3: {e}"))?;
            let length = file
                .metadata()
                .map_err(|e| format!("Could not inspect staged TNV3: {e}"))?
                .len();
            let output_path = self.parent_path(output_name);
            let (published_len, copied) = publish_one(
                &self.parent_fd,
                &self.stage_fd,
                PAYLOAD,
                output_name,
                &output_path,
                "TNV3",
                length,
            )?;
            let sync_warning = rfs::fsync(&self.parent_fd)
                .err()
                .map(|e| format!("Published, but output-directory sync failed: {e}"));
            let warning = if copied {
                let fallback = "This filesystem does not support no-replace rename; output was copied into an exclusively created final file. The final name may be visible while bytes are being copied, and a failure may leave a partial output. Inspect it before retrying.";
                Some(
                    sync_warning.map_or_else(|| fallback.to_owned(), |w| format!("{fallback} {w}")),
                )
            } else {
                sync_warning
            };
            Ok((published_len, warning))
        }

        fn parent_path(&self, name: &OsStr) -> PathBuf {
            // This path is diagnostic only; actual publication remains dirfd-relative.
            self.parent.join(name)
        }
    }

    fn unsupported_rename(error: rustix::io::Errno) -> bool {
        error == rustix::io::Errno::NOTSUP || error == rustix::io::Errno::OPNOTSUPP
    }

    fn rename_or_copy<T>(
        rename: impl FnOnce() -> Result<(), rustix::io::Errno>,
        fallback: impl FnOnce() -> Result<T, String>,
        label: &str,
    ) -> Result<(Option<T>, bool), String> {
        match rename() {
            Ok(()) => Ok((None, false)),
            Err(error) if unsupported_rename(error) => fallback().map(|value| (Some(value), true)),
            Err(error) => Err(format!(
                "Could not publish {label} without replacing an existing path: {error}"
            )),
        }
    }

    fn copy_staged(
        parent_fd: &OwnedFd,
        stage_fd: &OwnedFd,
        stage_name: &str,
        output_name: &OsStr,
        output_path: &Path,
        label: &str,
    ) -> Result<u64, String> {
        let source_fd = rfs::openat(
            stage_fd,
            stage_name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|e| format!("Could not open staged {label}: {e}"))?;
        let output_fd = rfs::openat(
            parent_fd,
            output_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_bits_retain(0o666),
        )
        .map_err(|e| {
            format!(
                "Could not create {label} output '{}': {e}",
                output_path.display()
            )
        })?;
        let source = fs::File::from(source_fd);
        copy_to_new(source, output_fd, output_path, label)
    }

    fn copy_to_new(
        mut source: impl std::io::Read,
        output_fd: OwnedFd,
        output_path: &Path,
        label: &str,
    ) -> Result<u64, String> {
        use std::io;
        let mut output = fs::File::from(output_fd);
        let result = io::copy(&mut source, &mut output)
            .map_err(|e| format!("Could not copy {label} output: {e}"))
            .and_then(|n| {
                output
                    .sync_all()
                    .map(|_| n)
                    .map_err(|e| format!("Could not sync {label} output: {e}"))
            });
        result.map_err(|error| format!(
            "{error}. Partial {label} output may remain at '{}'; inspect it before retrying. Final outputs are never removed on error.",
            output_path.display()
        ))
    }

    fn publish_one(
        parent_fd: &OwnedFd,
        stage_fd: &OwnedFd,
        stage_name: &str,
        output_name: &OsStr,
        output_path: &Path,
        label: &str,
        expected_len: u64,
    ) -> Result<(u64, bool), String> {
        publish_one_with_rename(
            parent_fd,
            stage_fd,
            stage_name,
            output_name,
            output_path,
            label,
            expected_len,
            || {
                rfs::renameat_with(
                    stage_fd,
                    stage_name,
                    parent_fd,
                    output_name,
                    RenameFlags::NOREPLACE,
                )
            },
        )
    }

    fn publish_one_with_rename(
        parent_fd: &OwnedFd,
        stage_fd: &OwnedFd,
        stage_name: &str,
        output_name: &OsStr,
        output_path: &Path,
        label: &str,
        expected_len: u64,
        rename: impl FnOnce() -> Result<(), rustix::io::Errno>,
    ) -> Result<(u64, bool), String> {
        let (copied_len, copied) = rename_or_copy(
            rename,
            || {
                copy_staged(
                    parent_fd,
                    stage_fd,
                    stage_name,
                    output_name,
                    output_path,
                    label,
                )
            },
            label,
        )?;
        Ok((copied_len.unwrap_or(expected_len), copied))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, io::Write};
        fn temp_dir() -> PathBuf {
            unique_test_dir("tau-cli-pub")
        }
        #[test]
        fn enotsup_uses_create_new_copy_and_preserves_existing_target() {
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"staged bytes").unwrap();
            let existing = dir.join("existing.tnv3");
            fs::write(&existing, b"keep me").unwrap();
            let existing_name = existing.file_name().unwrap();
            let error = publish_one_with_rename(
                &stage.parent_fd,
                &stage.stage_fd,
                PAYLOAD,
                existing_name,
                &existing,
                "TNV3",
                12,
                || Err(rustix::io::Errno::NOTSUP),
            )
            .unwrap_err();
            assert!(error.contains("Could not create TNV3 output"));
            assert_eq!(fs::read(&existing).unwrap(), b"keep me");
            let target = dir.join("copied.tnv3");
            let (_, copied) = publish_one_with_rename(
                &stage.parent_fd,
                &stage.stage_fd,
                PAYLOAD,
                target.file_name().unwrap(),
                &target,
                "TNV3",
                12,
                || Err(rustix::io::Errno::OPNOTSUPP),
            )
            .unwrap();
            assert!(copied);
            assert_eq!(fs::read(&target).unwrap(), b"staged bytes");
            drop(stage);
            fs::remove_dir_all(dir).unwrap();
        }
        struct FaultReader {
            first: bool,
        }
        impl std::io::Read for FaultReader {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if self.first {
                    self.first = false;
                    out[0] = b'x';
                    Ok(1)
                } else {
                    Err(std::io::Error::other("injected read failure"))
                }
            }
        }
        #[test]
        fn failed_copy_reports_partial_and_does_not_unlink_final_path() {
            let dir = temp_dir();
            let parent = rfs::open(
                &dir,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .unwrap();
            let dest = dir.join("partial.tnv3");
            let fd = rfs::openat(
                &parent,
                "partial.tnv3",
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::from_bits_retain(0o600),
            )
            .unwrap();
            let error = copy_to_new(FaultReader { first: true }, fd, &dest, "TNV3").unwrap_err();
            assert!(error.contains("Partial TNV3 output may remain"));
            assert_eq!(fs::read(&dest).unwrap(), b"x");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    impl Drop for ProtectedStage {
        fn drop(&mut self) {
            // Only remove private staging entries. Final paths are never unlinked,
            // including after a partial pair publish or copy failure.
            let _ = rfs::unlinkat(&self.stage_fd, PAYLOAD, AtFlags::empty());
            let _ = rfs::unlinkat(&self.stage_fd, AUDIO_PAYLOAD, AtFlags::empty());
            let _ = rfs::unlinkat(&self.parent_fd, &self.stage_name, AtFlags::REMOVEDIR);
        }
    }
}

#[cfg(windows)]
mod publication {
    use super::*;
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Seek, SeekFrom},
    };

    const PAYLOAD: &str = "payload.tnv3";
    const AUDIO_PAYLOAD: &str = "payload.mp3";

    pub struct ProtectedStage {
        parent: PathBuf,
        stage_dir: PathBuf,
        payload: Option<File>,
    }

    impl ProtectedStage {
        pub fn create(parent: &Path) -> Result<Self, String> {
            for _ in 0..100 {
                let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
                let stage_dir =
                    env::temp_dir().join(format!("tau-codecs-cli-{}-{id}", std::process::id()));
                match fs::create_dir(&stage_dir) {
                    Ok(()) => {
                        let payload = match OpenOptions::new()
                            .read(true)
                            .write(true)
                            .create_new(true)
                            .open(stage_dir.join(PAYLOAD))
                        {
                            Ok(file) => file,
                            Err(error) => {
                                let _ = fs::remove_dir(&stage_dir);
                                return Err(format!("Could not create staged TNV3: {error}"));
                            }
                        };
                        return Ok(Self {
                            parent: parent.to_path_buf(),
                            stage_dir,
                            payload: Some(payload),
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        return Err(format!("Could not create staging directory: {error}"));
                    }
                }
            }
            Err("Could not reserve a unique staging directory".into())
        }

        pub fn writer(&mut self) -> &mut File {
            self.payload.as_mut().expect("staged payload exists")
        }

        pub fn audio_path(&self, _parent: &Path) -> PathBuf {
            self.stage_dir.join(AUDIO_PAYLOAD)
        }

        pub fn publish_pair(
            mut self,
            video_name: &OsStr,
            audio_name: &OsStr,
        ) -> Result<(u64, u64, Option<String>), String> {
            let audio_source = self.audio_path(&self.parent);
            let mut audio_input = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&audio_source)
                .map_err(|e| format!("Could not open staged MP3 before publication: {e}"))?;
            audio_input
                .sync_all()
                .map_err(|e| format!("Could not sync staged MP3 before publication: {e}"))?;
            let audio_len = audio_input
                .metadata()
                .map_err(|e| format!("Could not inspect staged MP3 before publication: {e}"))?
                .len();
            audio_input
                .seek(SeekFrom::Start(0))
                .map_err(|e| format!("Could not rewind staged MP3 before publication: {e}"))?;

            let video = self.payload.as_mut().expect("staged payload exists");
            video
                .sync_all()
                .map_err(|e| format!("Could not sync staged TNV3: {e}"))?;
            let video_len = video
                .metadata()
                .map_err(|e| format!("Could not inspect staged TNV3: {e}"))?
                .len();
            video
                .seek(SeekFrom::Start(0))
                .map_err(|e| format!("Could not rewind staged TNV3: {e}"))?;

            let video_path = self.parent.join(video_name);
            copy_new(video, &video_path, "TNV3")?;

            let audio_path = self.parent.join(audio_name);
            if let Err(error) = copy_new(&mut audio_input, &audio_path, "MP3") {
                return Err(format!(
                    "{error}. TNV3 remains published at '{}'; the output pair is incomplete and publication is not atomic.",
                    video_path.display()
                ));
            }
            Ok((video_len, audio_len, None))
        }

        pub fn publish(mut self, output_name: &OsStr) -> Result<(u64, Option<String>), String> {
            let video = self.payload.as_mut().expect("staged payload exists");
            video
                .sync_all()
                .map_err(|e| format!("Could not sync staged TNV3: {e}"))?;
            let length = video
                .metadata()
                .map_err(|e| format!("Could not inspect staged TNV3: {e}"))?
                .len();
            video
                .seek(SeekFrom::Start(0))
                .map_err(|e| format!("Could not rewind staged TNV3: {e}"))?;
            let output_path = self.parent.join(output_name);
            copy_new(video, &output_path, "TNV3")?;
            Ok((length, None))
        }
    }

    impl Drop for ProtectedStage {
        fn drop(&mut self) {
            self.payload.take();
            let _ = fs::remove_dir_all(&self.stage_dir);
        }
    }

    fn copy_new(source: &mut File, destination: &Path, label: &str) -> Result<u64, String> {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| {
                format!(
                    "Could not create {label} output '{}': {e}",
                    destination.display()
                )
            })?;
        let result = (|| {
            let copied = io::copy(source, &mut output)
                .map_err(|e| format!("Could not copy {label} output: {e}"))?;
            output
                .sync_all()
                .map_err(|e| format!("Could not sync {label} output: {e}"))?;
            Ok::<u64, String>(copied)
        })();
        drop(output);
        result.map_err(|error| {
            format!(
                "{error}. Partial {label} output may remain at '{}'; inspect it before retrying. Final outputs are never removed on error.",
                destination.display()
            )
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn temp_dir() -> PathBuf {
            super::super::unique_test_dir("tau-cli-win")
        }

        #[test]
        fn staged_audio_is_opened_before_video_publication() {
            use std::io::Write;
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"staged video").unwrap();
            let error = stage
                .publish_pair(OsStr::new("film.tnv3"), OsStr::new("film.mp3"))
                .unwrap_err();
            assert!(error.contains("before publication"));
            assert!(!dir.join("film.tnv3").exists());
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn pair_error_after_video_publish_names_and_keeps_video() {
            use std::io::Write;
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"video").unwrap();
            fs::write(stage.audio_path(&dir), b"staged audio").unwrap();
            fs::write(dir.join("film.mp3"), b"existing audio").unwrap();
            let error = stage
                .publish_pair(OsStr::new("film.tnv3"), OsStr::new("film.mp3"))
                .unwrap_err();
            assert!(error.contains("film.tnv3"));
            assert!(error.contains("incomplete"));
            assert_eq!(fs::read(dir.join("film.tnv3")).unwrap(), b"video");
            assert_eq!(fs::read(dir.join("film.mp3")).unwrap(), b"existing audio");
            fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
    tvc_encoder: Option<PathBuf>,
    tnv3_tool: Option<PathBuf>,
    qp: u8,
    jobs: usize,
    workspace_mib: u64,
    pocket_sidecar: bool,
    rust_c_orchestrator: bool,
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let names = if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        vec![format!("{name}.exe"), name.to_owned()]
    } else {
        vec![name.to_owned()]
    };
    env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| is_executable(p))
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_executable_arg(path: PathBuf) -> PathBuf {
    if path.components().count() == 1 {
        if let Some(found) = path.to_str().and_then(on_path) {
            return found;
        }
    }
    path
}

fn codec_tool(root: Option<&Path>, rel: &str) -> Option<PathBuf> {
    let path = root?.join(rel);
    #[cfg(windows)]
    let path = path.with_extension("exe");
    path.is_file().then_some(path)
}

fn discover_codec_root() -> Option<PathBuf> {
    if let Some(root) = env::var_os("TAU_CODECS_ROOT") {
        let root = PathBuf::from(root);
        if root.join("tools/tnv3_ref/encode_film.py").is_file() {
            return Some(root);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.extend(cwd.ancestors().map(Path::to_path_buf));
    }
    if let Ok(exe) = env::current_exe() {
        candidates.extend(exe.ancestors().map(Path::to_path_buf));
    }
    candidates
        .into_iter()
        .find(|p| p.join("tools/tnv3_ref/encode_film.py").is_file())
}

fn usage() -> &'static str {
    "Usage: tau-codecs-cli encode INPUT OUTPUT.tnv3 [--codec-root PATH] [--script PATH] [--video-only] [--rust-c-orchestrator] [--workspace-mib N] [--qp 0..51] [--jobs N] [--python PATH|NAME] [--ffmpeg PATH|NAME] [--ffprobe PATH|NAME] [--tvc-encoder PATH|NAME] [--tnv3-tool PATH|NAME]\n\nDefault: use Python to write TNV3 plus a matching 48 kHz stereo 128 kbit/s MP3. --rust-c-orchestrator is an experimental macOS-first path: FFmpeg decodes to Y4M stdout, Rust groups frames for the C TVC tools, and level is inferred from the actual scaled Y4M header. It does not require Python or FFprobe. --workspace-mib sets its projected temporary-workspace budget (default 512 MiB). Both outputs stay staged until video and audio succeed. Windows is unverified."
}

fn run_capturing_stderr(
    command: &mut Command,
    action: &str,
) -> Result<(ExitStatus, String), String> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start {action}: {e}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("Could not capture {action} diagnostics."))?;
    let diagnostics = std::thread::spawn(move || {
        const LIMIT: usize = 4096;
        let mut saved = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut truncated = false;
        let mut stderr = stderr;
        loop {
            let count = stderr.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            let take = count.min(LIMIT.saturating_sub(saved.len()));
            saved.extend_from_slice(&buffer[..take]);
            truncated |= take < count;
        }
        let mut message = String::from_utf8_lossy(&saved).trim().to_owned();
        if truncated {
            message.push_str("\n[Further diagnostics omitted.]");
        }
        Ok::<_, String>(message)
    });
    let status = child
        .wait()
        .map_err(|e| format!("Could not wait for {action}: {e}"))?;
    let detail = diagnostics
        .join()
        .map_err(|_| format!("Could not collect {action} diagnostics."))??;
    Ok((status, detail))
}

fn drain_bounded_stderr(mut stderr: impl Read) -> Result<String, String> {
    const LIMIT: usize = 4096;
    let mut saved = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut truncated = false;
    loop {
        let count = stderr
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let take = count.min(LIMIT.saturating_sub(saved.len()));
        saved.extend_from_slice(&buffer[..take]);
        truncated |= take < count;
    }
    let mut message = String::from_utf8_lossy(&saved).trim().to_owned();
    if truncated {
        message.push_str("\n[Further diagnostics omitted.]");
    }
    Ok(message)
}

fn diagnostics_warning(label: &str, detail: &str) -> Option<String> {
    (!detail.is_empty()).then(|| format!("{label} diagnostics:\n{detail}"))
}

fn retry_description(rust_c_orchestrator: bool, count: u32) -> String {
    if rust_c_orchestrator {
        format!("{count} joint segment retry(s)")
    } else {
        format!("{count} picture(s) raised for level limits")
    }
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
    let mut workspace_mib = 512u64;
    let mut workspace_mib_explicit = false;
    let mut pocket_sidecar = true;
    let mut rust_c_orchestrator = false;
    let mut codec_root = None;
    while let Some(key) = it.next() {
        let key_text = key
            .to_str()
            .ok_or_else(|| format!("option name must be UTF-8: {}", key.to_string_lossy()))?;
        if key_text == "--video-only" {
            pocket_sidecar = false;
            continue;
        }
        if key_text == "--rust-c-orchestrator" {
            rust_c_orchestrator = true;
            continue;
        }
        if key_text == "--workspace-mib" {
            workspace_mib = it
                .next()
                .ok_or("missing value for --workspace-mib")?
                .to_str()
                .ok_or("workspace size must be UTF-8")?
                .parse()
                .map_err(|_| "workspace size must be a positive integer in MiB")?;
            workspace_mib_explicit = true;
            continue;
        }
        if key_text == "--codec-root" {
            codec_root = Some(PathBuf::from(
                it.next().ok_or("missing value for --codec-root")?,
            ));
            continue;
        }
        if key_text == "--profile" {
            let value = it
                .next()
                .ok_or_else(|| "missing value for --profile".to_owned())?;
            if value != OsStr::new("pocket-sidecar") {
                return Err("Supported profile: pocket-sidecar".into());
            }
            pocket_sidecar = true;
            continue;
        }
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
    if workspace_mib == 0 {
        return Err("workspace size must be a positive integer in MiB".into());
    }
    if workspace_mib.checked_mul(1024 * 1024).is_none() {
        return Err("workspace size in MiB exceeds the supported byte range".into());
    }
    if workspace_mib_explicit && !rust_c_orchestrator {
        return Err("--workspace-mib requires --rust-c-orchestrator".into());
    }
    if pocket_sidecar && output.extension() != Some(OsStr::new("tnv3")) {
        return Err("The pocket-sidecar profile requires an OUTPUT path ending in .tnv3".into());
    }
    // An explicit script makes the checkout optional. Explicit TVC/TNV3 helpers
    // are also sufficient when the encoder script is supplied from a bundle.
    let root = codec_root.or_else(|| {
        if script.is_none() {
            discover_codec_root()
        } else {
            None
        }
    });
    let python = python
        .map(resolve_executable_arg)
        .or_else(|| on_path("python3"))
        .or_else(|| on_path("python"));
    let python = if rust_c_orchestrator {
        python.unwrap_or_default()
    } else {
        python.ok_or("Python 3 not found on PATH; use --python")?
    };
    let ffmpeg = ffmpeg
        .map(resolve_executable_arg)
        .or_else(|| on_path("ffmpeg"))
        .ok_or("FFmpeg not found on PATH; use --ffmpeg")?;
    let ffprobe = ffprobe
        .map(resolve_executable_arg)
        .or_else(|| on_path("ffprobe"));
    let ffprobe = if rust_c_orchestrator {
        ffprobe.unwrap_or_default()
    } else {
        ffprobe.ok_or("FFprobe not found on PATH; use --ffprobe")?
    };
    let script = match script {
        Some(path) => path,
        None if rust_c_orchestrator => root
            .as_ref()
            .map(|root| root.join("tools/tnv3_ref/encode_film.py"))
            .unwrap_or_default(),
        None => root.as_ref().ok_or("Could not find Tau codecs checkout; use --codec-root, --script, or TAU_CODECS_ROOT")?
            .join("tools/tnv3_ref/encode_film.py"),
    };
    Ok(Some(Args {
        input,
        output,
        python,
        script,
        ffmpeg,
        ffprobe,
        tvc_encoder: tvc_encoder
            .map(resolve_executable_arg)
            .or_else(|| codec_tool(root.as_deref(), "tools/tvc_ref/tvc_enc")),
        tnv3_tool: tnv3_tool
            .map(resolve_executable_arg)
            .or_else(|| codec_tool(root.as_deref(), "tools/tnv3_ref/tnv3")),
        qp,
        jobs,
        workspace_mib,
        pocket_sidecar,
        rust_c_orchestrator,
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

fn validate_optional_file(path: Option<&Path>, label: &str) -> Result<(), String> {
    if let Some(path) = path {
        required_file(path, label)?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn run(_args: Args) -> Result<(), String> {
    Err("Atomic no-replace publication is unsupported on this platform in this prototype".into())
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn run(args: Args) -> Result<(), String> {
    required_file(&args.input, "Input")?;
    required_file(&args.ffmpeg, "FFmpeg")?;
    if !args.rust_c_orchestrator {
        required_file(&args.python, "Python")?;
        required_file(&args.script, "Encoder script")?;
        required_file(&args.ffprobe, "FFprobe")?;
    }
    validate_optional_file(args.tvc_encoder.as_deref(), "TVC encoder")?;
    validate_optional_file(args.tnv3_tool.as_deref(), "TNV3 tool")?;
    if args.output.exists() {
        return Err(format!("Output already exists: {}", args.output.display()));
    }
    let audio_output = args.output.with_extension("mp3");
    if args.pocket_sidecar && audio_output.exists() {
        return Err(format!(
            "Audio output already exists: {}",
            audio_output.display()
        ));
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
        tvc_encoder: args.tvc_encoder,
        tnv3_tool: args.tnv3_tool,
    };
    let output_name = args
        .output
        .file_name()
        .ok_or("Output path must include a file name")?;
    let mut staged = publication::ProtectedStage::create(parent)?;
    let (frames, level, retries, encoder_diagnostics) = if args.rust_c_orchestrator {
        let tvc_encoder = tools
            .tvc_encoder
            .as_ref()
            .ok_or("--rust-c-orchestrator requires --tvc-encoder or --codec-root")?;
        let tnv3_tool = tools
            .tnv3_tool
            .as_ref()
            .ok_or("--rust-c-orchestrator requires --tnv3-tool or --codec-root")?;
        let mut ffmpeg = Command::new(&tools.ffmpeg);
        ffmpeg
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&args.input)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-vf",
                "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "yuv4mpegpipe",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = ffmpeg
            .spawn()
            .map_err(|error| format!("Could not start FFmpeg Y4M decode: {error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Could not read FFmpeg Y4M output")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Could not capture FFmpeg diagnostics")?;
        let (diagnostics_tx, diagnostics_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = diagnostics_tx.send(drain_bounded_stderr(stderr));
        });
        let tools = RustCTools {
            tvc_encoder: tvc_encoder.clone(),
            tnv3: tnv3_tool.clone(),
        };
        let encoded = experimental_c_orchestrator::encode_y4m_video(
            &tools,
            std::io::BufReader::new(stdout),
            staged.writer(),
            RustCOptions {
                qp: args.qp,
                jobs: args.jobs.clamp(1, 6),
                // Match encode_film.py's default --seg 480.
                frames_per_segment: 480,
                workspace_limit_bytes: args.workspace_mib * 1024 * 1024,
                ..RustCOptions::default()
            },
        );
        let report = match encoded {
            Ok(report) => report,
            Err(error) => {
                let _ = child.kill();
                let status = child.wait().map_err(|wait_error| {
                    format!("{error}; could not reap FFmpeg: {wait_error}")
                })?;
                let detail = diagnostics_rx
                    .recv_timeout(Duration::from_secs(2))
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_else(|| "[FFmpeg stderr pipe did not close promptly.]".into());
                return Err(format!(
                    "{error}; FFmpeg was stopped after the Y4M stream error ({status}): {detail}"
                ));
            }
        };
        let status = child.wait();
        let detail = match diagnostics_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => result?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("FFmpeg stderr remained open after the process exited; output was not published (diagnostics reader detached).".into());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Could not collect FFmpeg diagnostics".into());
            }
        };
        let status =
            status.map_err(|error| format!("Could not wait for FFmpeg Y4M decode: {error}"))?;
        if !status.success() {
            return Err(format!("FFmpeg Y4M decode failed ({status}): {detail}"));
        }
        (
            report.frames,
            report.level,
            format!(
                "{} segment(s); {}",
                report.segments,
                retry_description(true, report.joint_reencodes)
            ),
            detail,
        )
    } else {
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
        (
            report.frames,
            report.level,
            retry_description(false, report.reencoded),
            report.diagnostics,
        )
    };
    let (byte_size, audio_byte_size, sync_warning, audio_warning) = if args.pocket_sidecar {
        let audio_path = staged.audio_path(parent);
        let duration = format!("{:.9}", f64::from(frames) / 24.0);
        let mut command = Command::new(&tools.ffmpeg);
        command
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&args.input)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-map_metadata",
                "-1",
                "-af",
                "apad",
                "-t",
                &duration,
                "-ar",
                "48000",
                "-ac",
                "2",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "128k",
                "-f",
                "mp3",
            ])
            .arg(&audio_path);
        let (status, detail) = run_capturing_stderr(&mut command, "FFmpeg MP3 encoding")?;
        if !status.success() {
            return Err(format!("MP3 encoding failed ({}): {}", status, detail));
        }
        let audio_name = audio_output
            .file_name()
            .ok_or("Audio output path must include a file name")?;
        let (video_len, audio_len, warning) = staged.publish_pair(output_name, audio_name)?;
        let audio_warning = diagnostics_warning("MP3 encoder", &detail);
        (video_len, audio_len, warning, audio_warning)
    } else {
        let (video_len, warning) = staged.publish(output_name)?;
        (video_len, 0, warning, None)
    };
    println!(
        "Wrote {}TNV3: {} frames, level {}, {}, {} bytes{}",
        if args.pocket_sidecar {
            "TNV3 + MP3 sidecar; "
        } else {
            "video-only "
        },
        frames,
        level,
        retries,
        byte_size,
        if args.pocket_sidecar {
            format!("; MP3 {audio_byte_size} bytes")
        } else {
            String::new()
        }
    );
    println!("Output: {}", args.output.display());
    if !encoder_diagnostics.is_empty() {
        eprintln!("Encoder diagnostics:\n{}", encoder_diagnostics);
    }
    if let Some(warning) = sync_warning {
        eprintln!("Warning: {warning}");
    }
    if let Some(warning) = audio_warning {
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        assert!(args.pocket_sidecar);
        assert_eq!(args.output, PathBuf::from("out.tnv3"));
        assert_eq!(args.script, PathBuf::from("/tools/encode_film.py"));
        assert_eq!(args.tvc_encoder, Some(PathBuf::from("/tools/tvc_enc")));
    }

    #[test]
    fn explicit_bundle_paths_do_not_require_checkout_discovery() {
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        let args = parse_args(argv).unwrap().unwrap();
        assert_eq!(args.script, PathBuf::from("/tools/encode_film.py"));
        assert_eq!(args.tnv3_tool, Some(PathBuf::from("/tools/tnv3")));
    }

    #[test]
    fn invalid_explicit_helper_path_has_a_clear_error() {
        let error =
            validate_optional_file(Some(Path::new("/missing/tvc_enc")), "TVC encoder").unwrap_err();
        assert!(error.contains("TVC encoder does not exist or is not a file"));
    }

    #[test]
    fn bare_explicit_executable_names_resolve_through_path() {
        if let Some(expected) = on_path("ffmpeg") {
            assert_eq!(resolve_executable_arg(PathBuf::from("ffmpeg")), expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn path_discovery_rejects_non_executable_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = env::temp_dir().join(format!("tau-cli-exec-check-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("candidate");
        fs::write(&file, b"tool").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&file));
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&file));
        let _ = fs::remove_file(file);
        let _ = fs::remove_dir(dir);
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

    #[test]
    fn retry_labels_distinguish_joint_segments_from_level_retries() {
        assert_eq!(retry_description(true, 2), "2 joint segment retry(s)");
        assert_eq!(
            retry_description(false, 3),
            "3 picture(s) raised for level limits"
        );
    }

    #[test]
    fn rust_c_flag_does_not_require_python_script_or_ffprobe() {
        let args = parse_args([
            OsString::from("encode"),
            OsString::from("in.mp4"),
            OsString::from("out.tnv3"),
            OsString::from("--rust-c-orchestrator"),
            OsString::from("--ffmpeg"),
            OsString::from("/tools/ffmpeg"),
            OsString::from("--tvc-encoder"),
            OsString::from("/tools/tvc_enc"),
            OsString::from("--tnv3-tool"),
            OsString::from("/tools/tnv3"),
        ])
        .unwrap()
        .unwrap();
        assert!(args.rust_c_orchestrator);
        assert_eq!(args.workspace_mib, 512);
        let args = parse_args([
            OsString::from("encode"),
            OsString::from("in.mp4"),
            OsString::from("out.tnv3"),
            OsString::from("--rust-c-orchestrator"),
            OsString::from("--workspace-mib"),
            OsString::from("1024"),
            OsString::from("--ffmpeg"),
            OsString::from("/tools/ffmpeg"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.workspace_mib, 1024);
        assert!(
            parse_args([
                OsString::from("encode"),
                OsString::from("in.mp4"),
                OsString::from("out.tnv3"),
                OsString::from("--rust-c-orchestrator"),
                OsString::from("--workspace-mib"),
                OsString::from("0"),
                OsString::from("--ffmpeg"),
                OsString::from("/tools/ffmpeg"),
            ])
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn ffmpeg_failure_after_valid_y4m_never_publishes_tnv3() {
        let Some(tvc) = env::var_os("TAU_TEST_TVC").map(PathBuf::from) else {
            return;
        };
        let Some(tnv3) = env::var_os("TAU_TEST_TNV3").map(PathBuf::from) else {
            return;
        };
        if !tvc.is_file() || !tnv3.is_file() {
            return;
        }
        let dir = env::temp_dir().join(format!(
            "tau-rust-ffmpeg-failure-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let y4m_path = dir.join("valid.y4m");
        let input = dir.join("unused-source.mp4");
        let fake_ffmpeg = dir.join("ffmpeg-fails-after-output");
        let output = dir.join("should-not-publish.tnv3");
        fs::write(&input, b"unused by the fake FFmpeg").unwrap();
        let mut y4m = b"YUV4MPEG2 W64 H48 F24:1 C420jpeg\n".to_vec();
        for _ in 0..2 {
            y4m.extend_from_slice(b"FRAME\n");
            y4m.resize(y4m.len() + 64 * 48 * 3 / 2, 0);
        }
        fs::write(&y4m_path, y4m).unwrap();
        let script = format!(
            "#!/bin/sh\ncat '{}'\ndd if=/dev/zero bs=9000 count=1 2>/dev/null | tr '\\000' X >&2\nexit 9\n",
            y4m_path.display()
        );
        fs::write(&fake_ffmpeg, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_ffmpeg, fs::Permissions::from_mode(0o700)).unwrap();

        let error = run(Args {
            input,
            output: output.clone(),
            python: PathBuf::new(),
            script: PathBuf::new(),
            ffmpeg: fake_ffmpeg,
            ffprobe: PathBuf::new(),
            tvc_encoder: Some(tvc),
            tnv3_tool: Some(tnv3),
            qp: 30,
            jobs: 1,
            workspace_mib: 512,
            pocket_sidecar: false,
            rust_c_orchestrator: true,
        })
        .unwrap_err();
        assert!(error.contains("exit status: 9"), "{error}");
        assert!(error.contains("Further diagnostics omitted"), "{error}");
        assert!(!output.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn malformed_y4m_kills_stalled_ffmpeg_without_publication() {
        let dir = env::temp_dir().join(format!(
            "tau-rust-ffmpeg-stall-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let input = dir.join("unused-source.mp4");
        let fake_ffmpeg = dir.join("ffmpeg-stalls");
        let output = dir.join("should-not-publish.tnv3");
        let tvc = dir.join("unused-tvc");
        let tnv3 = dir.join("unused-tnv3");
        fs::write(&input, b"unused by fake FFmpeg").unwrap();
        fs::write(&tvc, b"unused").unwrap();
        fs::write(&tnv3, b"unused").unwrap();
        fs::write(
            &fake_ffmpeg,
            "#!/bin/sh\nprintf 'bad header\\n'\nexec sleep 30\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_ffmpeg, fs::Permissions::from_mode(0o700)).unwrap();

        let started = std::time::Instant::now();
        let error = run(Args {
            input,
            output: output.clone(),
            python: PathBuf::new(),
            script: PathBuf::new(),
            ffmpeg: fake_ffmpeg,
            ffprobe: PathBuf::new(),
            tvc_encoder: Some(tvc),
            tnv3_tool: Some(tnv3),
            qp: 30,
            jobs: 1,
            workspace_mib: 512,
            pocket_sidecar: false,
            rust_c_orchestrator: true,
        })
        .unwrap_err();
        assert!(error.contains("invalid Y4M stream"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!output.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn video_only_flag_and_legacy_profile_are_supported() {
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        argv.push("--video-only".into());
        assert!(!parse_args(argv).unwrap().unwrap().pocket_sidecar);
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        argv.push("--rust-c-orchestrator".into());
        assert!(parse_args(argv).unwrap().unwrap().rust_c_orchestrator);
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        argv.extend(["--profile".into(), "pocket-sidecar".into()]);
        assert!(parse_args(argv).unwrap().unwrap().pocket_sidecar);
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        argv.extend(["--profile".into(), "unsafe".into()]);
        assert!(parse_args(argv).is_err());
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.mp3".into()];
        argv.extend(required_options());
        argv.extend(["--profile".into(), "pocket-sidecar".into()]);
        assert!(matches!(parse_args(argv), Err(error) if error.contains("ending in .tnv3")));
    }

    #[cfg(unix)]
    #[test]
    fn successful_child_stderr_is_retained_as_a_warning() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf 'fixture warning\\n' >&2; printf ignored_stdout",
        ]);
        let (status, detail) = run_capturing_stderr(&mut command, "diagnostic fixture").unwrap();
        assert!(status.success());
        assert_eq!(
            diagnostics_warning("MP3 encoder", &detail).as_deref(),
            Some("MP3 encoder diagnostics:\nfixture warning")
        );
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
        unique_test_dir("tau-publish-test")
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn tauv_mount_publication_probe_when_requested() {
        use std::io::Write;
        let Some(root) = env::var_os("TAU_CARD_TEST_ROOT").map(PathBuf::from) else {
            return;
        };
        let dir = root.join(format!(
            ".tau-cli-card-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let result = (|| {
            let mut video_stage = publication::ProtectedStage::create(&dir).unwrap();
            video_stage
                .writer()
                .write_all(b"generated video-only TNV3")
                .unwrap();
            let (n, warning) = video_stage
                .publish(OsStr::new("generated-video-only.tnv3"))
                .unwrap();
            assert_eq!(n, b"generated video-only TNV3".len() as u64);
            assert_eq!(
                fs::read(dir.join("generated-video-only.tnv3")).unwrap(),
                b"generated video-only TNV3"
            );
            if let Some(w) = warning {
                println!("video-only warning: {w}");
            }

            let mut pair_stage = publication::ProtectedStage::create(&dir).unwrap();
            pair_stage
                .writer()
                .write_all(b"generated paired TNV3")
                .unwrap();
            fs::write(pair_stage.audio_path(&dir), b"generated paired MP3").unwrap();
            let (vlen, alen, warning) = pair_stage
                .publish_pair(
                    OsStr::new("generated-pair.tnv3"),
                    OsStr::new("generated-pair.mp3"),
                )
                .unwrap();
            assert_eq!(vlen, b"generated paired TNV3".len() as u64);
            assert_eq!(alen, b"generated paired MP3".len() as u64);
            assert_eq!(
                fs::read(dir.join("generated-pair.tnv3")).unwrap(),
                b"generated paired TNV3"
            );
            assert_eq!(
                fs::read(dir.join("generated-pair.mp3")).unwrap(),
                b"generated paired MP3"
            );
            if let Some(w) = warning {
                println!("pair warning: {w}");
            }

            let existing = dir.join("preserve.mp3");
            fs::write(&existing, b"preexisting bytes").unwrap();
            let mut fail_stage = publication::ProtectedStage::create(&dir).unwrap();
            fail_stage.writer().write_all(b"owned video").unwrap();
            fs::write(fail_stage.audio_path(&dir), b"staged audio").unwrap();
            let err = fail_stage
                .publish_pair(OsStr::new("partial-video.tnv3"), OsStr::new("preserve.mp3"))
                .unwrap_err();
            assert!(err.contains("partial-video.tnv3"));
            assert_eq!(
                fs::read(dir.join("partial-video.tnv3")).unwrap(),
                b"owned video"
            );
            assert_eq!(fs::read(existing).unwrap(), b"preexisting bytes");
            println!("pair collision result: {err}");
            Ok::<(), String>(())
        })();
        let cleanup = fs::remove_dir_all(&dir);
        println!(
            "TauV generated-output test cleanup: {}",
            if cleanup.is_ok() { "ok" } else { "failed" }
        );
        result.unwrap();
        cleanup.unwrap();
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
    fn pair_publish_reports_and_keeps_video_when_audio_target_exists() {
        use std::io::Write;
        let dir = private_test_dir();
        let video = dir.join("pair.tnv3");
        let audio = dir.join("pair.mp3");
        fs::write(&audio, b"pre-existing audio").unwrap();
        let mut stage = publication::ProtectedStage::create(&dir).unwrap();
        stage.writer().write_all(b"staged video").unwrap();
        fs::write(stage.audio_path(&dir), b"staged audio").unwrap();
        let result = stage.publish_pair(OsStr::new("pair.tnv3"), OsStr::new("pair.mp3"));
        assert!(result.is_err());
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("pair publication is not atomic")
        );
        assert!(video.exists());
        assert_eq!(fs::read(&video).unwrap(), b"staged video");
        assert_eq!(fs::read(&audio).unwrap(), b"pre-existing audio");
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

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    #[test]
    fn unsupported_platform_publication_fails_explicitly() {
        let error = publication::ProtectedStage::create(Path::new("."))
            .err()
            .expect("unsupported platforms must fail closed");
        assert!(error.contains("unsupported on this platform"));
    }
}
