mod check;
mod decode_audio;
mod encode_audio;
mod mux_command;
mod child_supervisor;
mod info;
mod probe;
mod probe_command;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod probe_exec;
mod tool_identity;
mod tool_version;

use std::{
    env,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    sync::mpsc,
    time::Duration,
};
use tau_video::c_encoder::{self, Options, Tools};
use tau_video::experimental_c_orchestrator::{self, Options as RustCOptions, Tools as RustCTools};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Convert terminal SIGINT into the same cooperative cancellation path as
/// control-stdin EOF. The handler itself is owned by signal-hook; work happens
/// on this listener thread, where it is safe to lock/send the active child stop.
#[cfg(unix)]
struct SigintCancellation {
    handle: signal_hook::iterator::Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl SigintCancellation {
    fn install(
        state: std::sync::Arc<AtomicU8>,
        stop: std::sync::Arc<AtomicBool>,
        active_stop: std::sync::Arc<std::sync::Mutex<Option<mpsc::Sender<()>>>>,
    ) -> Result<Self, RunError> {
        let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGINT])
            .map_err(|error| {
                RunError::io(format!("Could not install SIGINT cancellation: {error}"))
            })?;
        let handle = signals.handle();
        let thread = std::thread::Builder::new()
            .name("tau-sigint-cancel".into())
            .spawn(move || {
                for _signal in signals.forever() {
                    if state
                        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        stop.store(true, Ordering::Release);
                        if let Ok(slot) = active_stop.lock() {
                            if let Some(sender) = slot.as_ref() {
                                let _ = sender.send(());
                            }
                        }
                    }
                }
            })
            .map_err(|error| RunError::io(format!("Could not start SIGINT listener: {error}")))?;
        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }
}

#[cfg(unix)]
impl Drop for SigintCancellation {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

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
        retained: bool,
        allow_nonatomic_copy: bool,
    }

    impl ProtectedStage {
        #[cfg(test)]
        pub fn create(parent: &Path) -> Result<Self, String> {
            Self::create_with_policy(parent, false)
        }

        pub fn create_with_policy(
            parent: &Path,
            allow_nonatomic_copy: bool,
        ) -> Result<Self, String> {
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
                            retained: false,
                            allow_nonatomic_copy,
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

        pub fn retain(&mut self) -> PathBuf {
            self.retained = true;
            self.parent.join(&self.stage_name)
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
            let prep = (|| {
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
                Ok::<_, String>((video_len, audio_len))
            })();
            let (video_len, audio_len) = match prep {
                Ok(value) => value,
                Err(error) => return Err(self.retain_error(error)),
            };

            let video_path = self.parent_path(video_name);
            let audio_path = self.parent_path(audio_name);
            let (published_video_len, video_copied) = match publish_one(
                &self.parent_fd,
                &self.stage_fd,
                PAYLOAD,
                video_name,
                &video_path,
                "TNV3",
                video_len,
                self.allow_nonatomic_copy,
            ) {
                Ok(result) => result,
                Err(error) => {
                    return Err(self.retain_error(error));
                }
            };
            let (published_audio_len, audio_copied) = match publish_one(
                &self.parent_fd,
                &self.stage_fd,
                AUDIO_PAYLOAD,
                audio_name,
                &audio_path,
                "MP3",
                audio_len,
                self.allow_nonatomic_copy,
            ) {
                Ok(result) => result,
                Err(error) => {
                    let path = self.retain();
                    return Err(format!(
                        "Audio publish failed after TNV3 was visible at '{}'; pair publication is not atomic. {error}; staging retained at '{}' (the video may have moved; inspect remaining files)",
                        video_path.display(),
                        path.display()
                    ));
                }
            };
            let mut warning = rfs::fsync(&self.parent_fd)
                .err()
                .map(|e| format!("Published pair, but output-directory sync failed: {e}"));
            if video_copied || audio_copied {
                let message = "Explicit --allow-nonatomic-copy fallback used: final names may be visible while bytes are copied; failure or interruption can leave partial outputs. Pair publication is not atomic; inspect outputs before retrying.";
                warning =
                    Some(warning.map_or_else(|| message.to_owned(), |w| format!("{message} {w}")));
            }
            Ok((published_video_len, published_audio_len, warning))
        }

        pub fn publish(&mut self, output_name: &OsStr) -> Result<(u64, Option<String>), String> {
            let prep = (|| {
                let file = self.payload.as_mut().expect("staged payload exists");
                file.sync_all()
                    .map_err(|e| format!("Could not sync staged TNV3: {e}"))?;
                file.metadata()
                    .map(|meta| meta.len())
                    .map_err(|e| format!("Could not inspect staged TNV3: {e}"))
            })();
            let length = match prep {
                Ok(value) => value,
                Err(error) => return Err(self.retain_error(error)),
            };
            let output_path = self.parent_path(output_name);
            let (published_len, copied) = match publish_one(
                &self.parent_fd,
                &self.stage_fd,
                PAYLOAD,
                output_name,
                &output_path,
                "TNV3",
                length,
                self.allow_nonatomic_copy,
            ) {
                Ok(result) => result,
                Err(error) => {
                    return Err(self.retain_error(error));
                }
            };
            let mut sync_warning = rfs::fsync(&self.parent_fd)
                .err()
                .map(|e| format!("Published, but output-directory sync failed: {e}"));
            if copied {
                let message = "Explicit --allow-nonatomic-copy fallback used: final name may be visible while bytes are copied; failure or interruption can leave a partial output. Inspect it before retrying.";
                sync_warning = Some(
                    sync_warning.map_or_else(|| message.to_owned(), |w| format!("{message} {w}")),
                );
            }
            Ok((published_len, sync_warning))
        }

        fn retain_error(&mut self, error: String) -> String {
            let path = self.retain();
            format!(
                "{error}; staging retained at '{}' and may be incomplete",
                path.display()
            )
        }

        fn parent_path(&self, name: &OsStr) -> PathBuf {
            // This path is diagnostic only; actual publication remains dirfd-relative.
            self.parent.join(name)
        }
    }

    fn unsupported_rename(error: rustix::io::Errno) -> bool {
        error == rustix::io::Errno::NOTSUP
            || error == rustix::io::Errno::OPNOTSUPP
            || error == rustix::io::Errno::NOSYS
    }

    // Both operations atomically publish the already-synced staged file without
    // replacing an existing name. If neither is available, fail closed rather
    // than expose a partially copied output.
    #[cfg(test)]
    fn rename_or_link(
        rename: impl FnOnce() -> Result<(), rustix::io::Errno>,
        link: impl FnOnce() -> Result<(), rustix::io::Errno>,
        label: &str,
    ) -> Result<(), String> {
        match rename() {
            Ok(()) => Ok(()),
            Err(error) if unsupported_rename(error) => link().map_err(|error| format!(
                "Could not atomically publish {label} without replacing an existing path: {error}; this filesystem may not support no-replace rename or hard links"
            )),
            Err(error) => Err(format!("Could not publish {label} without replacing an existing path: {error}")),
        }
    }

    fn publish_one(
        parent_fd: &OwnedFd,
        stage_fd: &OwnedFd,
        stage_name: &str,
        output_name: &OsStr,
        output_path: &Path,
        label: &str,
        expected_len: u64,
        allow_nonatomic_copy: bool,
    ) -> Result<(u64, bool), String> {
        publish_one_with_policy(
            parent_fd,
            stage_fd,
            stage_name,
            output_name,
            output_path,
            label,
            expected_len,
            allow_nonatomic_copy,
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

    #[cfg(test)]
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
        publish_one_with_policy(
            parent_fd,
            stage_fd,
            stage_name,
            output_name,
            output_path,
            label,
            expected_len,
            false,
            rename,
        )
    }

    fn publish_one_with_policy(
        parent_fd: &OwnedFd,
        stage_fd: &OwnedFd,
        stage_name: &str,
        output_name: &OsStr,
        output_path: &Path,
        label: &str,
        expected_len: u64,
        allow_nonatomic_copy: bool,
        rename: impl FnOnce() -> Result<(), rustix::io::Errno>,
    ) -> Result<(u64, bool), String> {
        publish_with_operations(
            expected_len,
            allow_nonatomic_copy,
            rename,
            || {
                rfs::linkat(
                    stage_fd,
                    stage_name,
                    parent_fd,
                    output_name,
                    AtFlags::empty(),
                )
            },
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
        )
    }

    fn publish_with_operations(
        expected_len: u64,
        allow_nonatomic_copy: bool,
        rename: impl FnOnce() -> Result<(), rustix::io::Errno>,
        link: impl FnOnce() -> Result<(), rustix::io::Errno>,
        copy: impl FnOnce() -> Result<u64, String>,
        label: &str,
    ) -> Result<(u64, bool), String> {
        match rename() {
            Ok(()) => Ok((expected_len, false)),
            Err(error) if unsupported_rename(error) => match link() {
                Ok(()) => Ok((expected_len, false)),
                Err(link_error)
                    if allow_nonatomic_copy && link_error != rustix::io::Errno::EXIST =>
                {
                    let copied = copy()?;
                    Ok((copied, true))
                }
                Err(link_error) => Err(format!(
                    "Could not atomically publish {label} without replacing an existing path: rename returned {error}, hard link returned {link_error}; this filesystem may not support either mechanism"
                )),
            },
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
                "Could not exclusively create {label} output '{}': {e}",
                output_path.display()
            )
        })?;
        copy_to_exclusive(fs::File::from(source_fd), output_fd, output_path, label)
    }

    fn copy_to_exclusive(
        mut source: impl std::io::Read,
        output_fd: OwnedFd,
        output_path: &Path,
        label: &str,
    ) -> Result<u64, String> {
        let mut output = fs::File::from(output_fd);
        std::io::copy(&mut source, &mut output)
            .map_err(|e| format!("Could not copy {label}: {e}. A partial output may remain at '{}'; staging will be retained.", output_path.display()))
            .and_then(|bytes| output.sync_all().map(|_| bytes)
                .map_err(|e| format!("Could not sync {label}: {e}. A partial output may remain at '{}'; staging will be retained.", output_path.display())))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{fs, io::Write};
        fn temp_dir() -> PathBuf {
            unique_test_dir("tau-cli-pub")
        }
        #[test]
        fn unsupported_rename_uses_atomic_link_and_never_overwrites() {
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"staged bytes").unwrap();
            let existing = dir.join("existing.tnv3");
            fs::write(&existing, b"keep me").unwrap();
            let error = publish_one_with_rename(
                &stage.parent_fd,
                &stage.stage_fd,
                PAYLOAD,
                existing.file_name().unwrap(),
                &existing,
                "TNV3",
                12,
                || Err(rustix::io::Errno::NOTSUP),
            )
            .unwrap_err();
            assert!(error.contains("Could not atomically publish"));
            assert_eq!(fs::read(&existing).unwrap(), b"keep me");
            let target = dir.join("linked.tnv3");
            publish_one_with_rename(
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
            assert_eq!(fs::read(&target).unwrap(), b"staged bytes");
            drop(stage);
            assert_eq!(fs::read(&target).unwrap(), b"staged bytes");
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn audio_preparation_failure_retains_staging() {
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"new video").unwrap();
            let staged = dir.join(&stage.stage_name);
            let error = stage
                .publish_pair(OsStr::new("film.tnv3"), OsStr::new("film.mp3"))
                .unwrap_err();
            assert!(error.contains("staging retained at"));
            assert_eq!(fs::read(staged.join(PAYLOAD)).unwrap(), b"new video");
            assert!(!dir.join("film.tnv3").exists());
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn publication_conflicts_retain_staged_data_and_never_replace_outputs() {
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"new video").unwrap();
            let staged = dir.join(&stage.stage_name);
            fs::write(dir.join("existing.tnv3"), b"old video").unwrap();
            let error = stage.publish(OsStr::new("existing.tnv3")).unwrap_err();
            assert!(error.contains("staging retained at"));
            drop(stage);
            assert_eq!(fs::read(dir.join("existing.tnv3")).unwrap(), b"old video");
            assert_eq!(fs::read(staged.join(PAYLOAD)).unwrap(), b"new video");
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn audio_conflict_keeps_published_video_and_retains_staged_audio() {
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"new video").unwrap();
            let staged = dir.join(&stage.stage_name);
            fs::write(stage.audio_path(&dir), b"new audio").unwrap();
            fs::write(dir.join("film.mp3"), b"old audio").unwrap();
            let error = stage
                .publish_pair(OsStr::new("film.tnv3"), OsStr::new("film.mp3"))
                .unwrap_err();
            assert!(error.contains("pair publication is not atomic"));
            assert!(error.contains("staging retained at"));
            assert_eq!(fs::read(dir.join("film.tnv3")).unwrap(), b"new video");
            assert_eq!(fs::read(dir.join("film.mp3")).unwrap(), b"old audio");
            assert_eq!(fs::read(staged.join(AUDIO_PAYLOAD)).unwrap(), b"new audio");
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn unsupported_atomic_operations_fail_without_creating_output() {
            let dir = temp_dir();
            let target = dir.join("untouched.tnv3");
            let error = rename_or_link(
                || Err(rustix::io::Errno::NOTSUP),
                || Err(rustix::io::Errno::OPNOTSUPP),
                "TNV3",
            )
            .unwrap_err();
            assert!(error.contains("filesystem may not support"));
            assert!(!target.exists());
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn explicit_copy_policy_only_copies_after_both_atomic_methods_are_unsupported() {
            let dir = temp_dir();
            let mut stage = ProtectedStage::create(&dir).unwrap();
            stage.writer().write_all(b"complete").unwrap();
            let target = dir.join("explicit.tnv3");
            let strict = publish_with_operations(
                8,
                false,
                || Err(rustix::io::Errno::NOTSUP),
                || Err(rustix::io::Errno::PERM),
                || panic!("default policy must not call copy"),
                "TNV3",
            )
            .unwrap_err();
            assert!(strict.contains("filesystem may not support"));
            assert!(!target.exists());

            let (len, copied) = publish_with_operations(
                8,
                true,
                || Err(rustix::io::Errno::NOTSUP),
                || Err(rustix::io::Errno::PERM),
                || {
                    copy_staged(
                        &stage.parent_fd,
                        &stage.stage_fd,
                        PAYLOAD,
                        target.file_name().unwrap(),
                        &target,
                        "TNV3",
                    )
                },
                "TNV3",
            )
            .unwrap();
            assert_eq!(len, 8);
            assert!(copied);
            assert_eq!(fs::read(&target).unwrap(), b"complete");
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn explicit_copy_failure_reports_partial_risk_and_preserves_user_file() {
            let dir = temp_dir();
            let target = dir.join("existing.tnv3");
            fs::write(&target, b"user data").unwrap();
            let result = publish_with_operations(
                4,
                true,
                || Err(rustix::io::Errno::NOTSUP),
                || Err(rustix::io::Errno::OPNOTSUPP),
                || Err("partial output may remain after write failure".into()),
                "TNV3",
            )
            .unwrap_err();
            assert!(result.contains("partial output may remain"));
            assert_eq!(fs::read(&target).unwrap(), b"user data");
            fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn copy_staged_uses_exclusive_create_on_collision() {
            let dir = temp_dir();
            let stage = ProtectedStage::create(&dir).unwrap();
            let target = dir.join("existing.tnv3");
            fs::write(&target, b"user data").unwrap();
            let error = copy_staged(
                &stage.parent_fd,
                &stage.stage_fd,
                PAYLOAD,
                target.file_name().unwrap(),
                &target,
                "TNV3",
            )
            .unwrap_err();
            assert!(error.contains("Could not exclusively create"));
            assert_eq!(fs::read(&target).unwrap(), b"user data");
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
                    Err(std::io::Error::other("injected copy failure"))
                }
            }
        }

        #[test]
        fn failed_exclusive_copy_reports_and_leaves_partial_for_inspection() {
            let dir = temp_dir();
            let parent = rfs::open(
                &dir,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .unwrap();
            let target = dir.join("partial.tnv3");
            let fd = rfs::openat(
                &parent,
                "partial.tnv3",
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::from_bits_retain(0o600),
            )
            .unwrap();
            let error =
                copy_to_exclusive(FaultReader { first: true }, fd, &target, "TNV3").unwrap_err();
            assert!(error.contains("A partial output may remain"));
            assert_eq!(fs::read(&target).unwrap(), b"x");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    impl Drop for ProtectedStage {
        fn drop(&mut self) {
            if self.retained {
                return;
            }
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
        retained: bool,
        allow_nonatomic_copy: bool,
    }

    impl ProtectedStage {
        #[cfg(test)]
        pub fn create(parent: &Path) -> Result<Self, String> {
            Self::create_with_policy(parent, false)
        }
        pub fn create_with_policy(
            parent: &Path,
            allow_nonatomic_copy: bool,
        ) -> Result<Self, String> {
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
                            retained: false,
                            allow_nonatomic_copy,
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

        pub fn retain(&mut self) -> PathBuf {
            self.retained = true;
            self.stage_dir.clone()
        }

        pub fn audio_path(&self, _parent: &Path) -> PathBuf {
            self.stage_dir.join(AUDIO_PAYLOAD)
        }

        pub fn publish_pair(
            mut self,
            video_name: &OsStr,
            audio_name: &OsStr,
        ) -> Result<(u64, u64, Option<String>), String> {
            if !self.allow_nonatomic_copy {
                return Err(self.retain_error("Windows no-replace atomic publication is not implemented; non-atomic copy is disabled".into()));
            }
            let prep = (|| -> Result<(File, u64, u64), String> {
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
                Ok((audio_input, audio_len, video_len))
            })();
            let (mut audio_input, audio_len, video_len) = match prep {
                Ok(value) => value,
                Err(error) => return Err(self.retain_error(error)),
            };

            let video = self.payload.as_mut().expect("staged payload exists");
            let video_path = self.parent.join(video_name);
            if let Err(error) = copy_new(video, &video_path, "TNV3") {
                let stage = self.retain();
                return Err(format!(
                    "{error}; staging retained at '{}'",
                    stage.display()
                ));
            }

            let audio_path = self.parent.join(audio_name);
            if let Err(error) = copy_new(&mut audio_input, &audio_path, "MP3") {
                let stage = self.retain();
                return Err(format!(
                    "{error}. TNV3 remains published at '{}'; pair publication is not atomic. Staging retained at '{}'.",
                    video_path.display(),
                    stage.display()
                ));
            }
            Ok((video_len, audio_len, Some("Explicit --allow-nonatomic-copy enabled: final names may expose partial bytes on failure, and pair publication is not atomic. Windows runtime is unverified.".into())))
        }

        pub fn publish(mut self, output_name: &OsStr) -> Result<(u64, Option<String>), String> {
            if !self.allow_nonatomic_copy {
                return Err(self.retain_error("Windows no-replace atomic publication is not implemented; non-atomic copy is disabled".into()));
            }
            let prep = (|| -> Result<u64, String> {
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
                Ok(length)
            })();
            let length = match prep {
                Ok(value) => value,
                Err(error) => return Err(self.retain_error(error)),
            };
            let video = self.payload.as_mut().expect("staged payload exists");
            let output_path = self.parent.join(output_name);
            if let Err(error) = copy_new(video, &output_path, "TNV3") {
                let stage = self.retain();
                return Err(format!(
                    "{error}; staging retained at '{}'",
                    stage.display()
                ));
            }
            Ok((length, Some("Explicit --allow-nonatomic-copy enabled: final name may expose partial bytes on failure. Windows runtime is unverified.".into())))
        }

        fn retain_error(&mut self, error: String) -> String {
            let staging = self.retain();
            format!(
                "{error}; staging retained at '{}' and may be incomplete",
                staging.display()
            )
        }
    }

    impl Drop for ProtectedStage {
        fn drop(&mut self) {
            if self.retained {
                return;
            }
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
            Self::create_with_policy(_parent, false)
        }
        pub fn create_with_policy(
            _parent: &Path,
            _allow_nonatomic_copy: bool,
        ) -> Result<Self, String> {
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
    /// Maximum output width in pixels (default 400); height is 9/10 of it, even.
    width: u32,
    /// TVC level 1..=3, or 0 for the smallest level that fits.
    level: u8,
    jobs: usize,
    workspace_mib: u64,
    pocket_sidecar: bool,
    rust_c_orchestrator: bool,
    json_events: bool,
    control_stdin: bool,
    allow_nonatomic_copy: bool,
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

// Only executable files in the binary's own directory are candidates. Explicit
// paths always take precedence, and no tools are downloaded.
fn executable_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let path = dir.join(filename);
    is_executable(&path).then_some(path)
}

fn adjacent_executable(name: &str) -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    executable_in_dir(exe.parent()?, name)
}

fn codec_tool(root: Option<&Path>, rel: &str) -> Option<PathBuf> {
    let path = root?.join(rel);
    #[cfg(windows)]
    let path = path.with_extension("exe");
    path.is_file().then_some(path)
}

fn select_codec_tool(
    root: Option<&Path>,
    explicit_root: bool,
    name: &str,
    rel: &str,
) -> Option<PathBuf> {
    if explicit_root {
        // Retain the explicit path even when missing so validation fails instead
        // of silently combining tools from different installations.
        return root.map(|root| {
            let path = root.join(rel);
            if cfg!(windows) {
                path.with_extension("exe")
            } else {
                path
            }
        });
    }
    adjacent_executable(name).or_else(|| codec_tool(root, rel))
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
    "Usage: tau-codecs-cli encode INPUT OUTPUT.tnv3 [--codec-root PATH] [--script PATH] [--profile pocket-sidecar@1] [--video-only] [--json-events] [--rust-c-orchestrator] [--control-stdin] [--allow-nonatomic-copy] [--workspace-mib N] [--qp 0..51] [--width 16..800] [--level 0..3] [--jobs N] [--python PATH|NAME] [--ffmpeg PATH|NAME] [--ffprobe PATH|NAME] [--tvc-encoder PATH|NAME] [--tnv3-tool PATH|NAME]\n       tau-codecs-cli --version [--json]\n       tau-codecs-cli handshake --json [--probe-versions] [explicit tool paths]\n       tau-codecs-cli tool-identity --json [explicit tool paths]\n       tau-codecs-cli info FILE [--json]\n       tau-codecs-cli probe INPUT [--ffmpeg PATH] [--ffprobe PATH] [--sample-estimate [--tvc-encoder PATH]] [--json]\n       tau-codecs-cli decode-audio INPUT.tnv3 OUTPUT.wav [--model integer|reference] [--codec-root PATH] [--tnv3-tool PATH] [--tac-tool PATH] [--json]\n       tau-codecs-cli encode-audio INPUT OUTPUT.tac [--bitrate 16..512] [--channels 1|2] [--codec-root PATH] [--tac-tool PATH] [--ffmpeg PATH|NAME] [--json]\n       tau-codecs-cli mux VIDEO.tvc OUTPUT.tnv3 [--audio FILE.tac] [--subtitles AREA.bin] [--crc] [--level 1..3] [--rate N/D] [--trim-audio] [--audio-shift N] [--codec-root PATH] [--tnv3-tool PATH] [--json]\n       tau-codecs-cli check FILE (--quick | --scope chunk-structure|cross-frame|subtitle-structure|container-preview) [--json]\n\nDefault publication requires atomic no-replace rename or hard-link support. --allow-nonatomic-copy explicitly permits create-new copying when both are unavailable; final names may expose partial output. Pair publication is not atomic. pocket-sidecar@1 selects the current TNV3+MP3 pairing contract (legacy alias: pocket-sidecar). --json-events writes schema-1 JSON Lines progress on stdout; diagnostics go to stderr. On Unix, --rust-c-orchestrator handles SIGINT cooperatively; --control-stdin also enables cancellation on stdin EOF. Windows supports stdin EOF cancellation only. Only supervised directly owned tools are stopped; descendants may outlive them. Resume and whole-process-tree cancellation are not provided. --workspace-mib sets its projected temporary-workspace budget (default 512 MiB). Windows is unverified."
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncoderProfile {
    PocketSidecarV1,
}

impl EncoderProfile {
    fn resolve(value: &OsStr) -> Result<Self, String> {
        match value.to_str() {
            Some("pocket-sidecar@1") | Some("pocket-sidecar") => Ok(Self::PocketSidecarV1),
            _ => Err("Supported profiles: pocket-sidecar@1 (legacy alias: pocket-sidecar)".into()),
        }
    }
}

fn write_stdout_line(line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    output.write_all(line.as_bytes())?;
    output.write_all(b"\n")?;
    output.flush()
}

fn run_capturing_stderr(
    command: &mut Command,
    action: &str,
) -> Result<(ExitStatus, String), RunError> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RunError::io(format!("Could not start {action}: {e}")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RunError::io(format!("Could not capture {action} diagnostics.")))?;
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
        .map_err(|e| RunError::io(format!("Could not wait for {action}: {e}")))?;
    let detail = diagnostics
        .join()
        .map_err(|_| RunError::io(format!("Could not collect {action} diagnostics.")))?
        .map_err(RunError::io)?;
    Ok((status, detail))
}

fn run_capturing_stderr_controlled(
    command: &mut Command,
    action: &str,
    stop: &AtomicBool,
    active_stop: &std::sync::Arc<std::sync::Mutex<Option<mpsc::Sender<()>>>>,
) -> Result<(ExitStatus, String), RunError> {
    if stop.load(Ordering::Acquire) {
        return Err(RunError::cancelled(
            "encoding stopped before starting FFmpeg MP3 encoding",
        ));
    }
    let child = child_supervisor::spawn_source(command).map_err(|error| {
        RunError::unverified(format!(
            "Could not start/supervise {action}; cleanup unverified: {error}"
        ))
    })?;
    let sender = child.stop.clone();
    if let Ok(mut active) = active_stop.lock() {
        *active = Some(sender.clone());
    }
    if stop.load(Ordering::Acquire) {
        let _ = sender.send(());
    }
    let stdout = child.stdout;
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let stdout_reader = std::thread::Builder::new()
        .name("tau-audio-stdout".into())
        .spawn(move || {
            let result = std::io::copy(&mut std::io::BufReader::new(stdout), &mut std::io::sink());
            let _ = stdout_tx.send(result);
        });
    if let Err(error) = stdout_reader {
        let _ = sender.send(());
        return match child.finished.join() {
            Ok(Ok(_)) => Err(RunError::io(format!(
                "Could not start {action} stdout reader: {error}"
            ))),
            result => Err(RunError::unverified(format!(
                "Could not start {action} stdout reader: {error}; cleanup unverified ({result:?})"
            ))),
        };
    }
    let stderr = child.stderr;
    let (diagnostics_tx, diagnostics_rx) = mpsc::channel();
    let stderr_reader = std::thread::Builder::new()
        .name("tau-audio-stderr".into())
        .spawn(move || {
            let _ = diagnostics_tx.send(drain_bounded_stderr(stderr));
        });
    if let Err(error) = stderr_reader {
        let _ = sender.send(());
        return match child.finished.join() {
            Ok(Ok(_)) => Err(RunError::io(format!(
                "Could not start {action} diagnostic reader: {error}"
            ))),
            result => Err(RunError::unverified(format!(
                "Could not start {action} diagnostic reader: {error}; cleanup unverified ({result:?})"
            ))),
        };
    }
    let status = child
        .finished
        .join()
        .map_err(|_| {
            RunError::unverified(format!("{action} supervisor panicked; cleanup unverified"))
        })?
        .map_err(|error| {
            RunError::unverified(format!(
                "Could not wait for {action}; cleanup unverified: {error}"
            ))
        })?;
    if let Ok(mut active) = active_stop.lock() {
        *active = None;
    }
    let detail = diagnostics_rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| RunError::unverified(format!("{action} diagnostics pipe remained open after direct-child exit; cleanup unverified")))?
        .map_err(RunError::io)?;
    stdout_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| {
            RunError::unverified(format!(
                "{action} stdout pipe remained open after direct-child exit; cleanup unverified"
            ))
        })?
        .map_err(|error| RunError::io(format!("Could not drain {action} stdout: {error}")))?;
    if stop.load(Ordering::Acquire) {
        return Err(RunError::cancelled(
            "encoding stopped during FFmpeg MP3 encoding",
        ));
    }
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

fn emit_event(value: serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let mut line = serde_json::to_vec(&value)
        .map_err(|error| format!("could not serialize JSON event: {error}"))?;
    line.push(b'\n');
    stdout
        .write_all(&line)
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("could not write JSON event: {error}"))
}

#[derive(Clone, Copy, Debug)]
enum RunErrorKind {
    Unclassified,
    InvalidInput,
    Io,
    Dependency,
    Cancelled,
}

#[derive(Debug)]
struct RunError {
    kind: RunErrorKind,
    message: String,
    cleanup_unverified: bool,
}

impl RunError {
    fn io(message: impl Into<String>) -> Self {
        Self {
            kind: RunErrorKind::Io,
            message: message.into(),
            cleanup_unverified: false,
        }
    }

    fn dependency(message: impl Into<String>) -> Self {
        Self {
            kind: RunErrorKind::Dependency,
            message: message.into(),
            cleanup_unverified: false,
        }
    }

    fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: RunErrorKind::InvalidInput,
            message: message.into(),
            cleanup_unverified: false,
        }
    }

    fn unclassified(message: impl Into<String>) -> Self {
        Self {
            kind: RunErrorKind::Unclassified,
            message: message.into(),
            cleanup_unverified: false,
        }
    }

    fn cancelled(message: impl Into<String>) -> Self {
        Self {
            kind: RunErrorKind::Cancelled,
            message: message.into(),
            cleanup_unverified: false,
        }
    }

    fn unverified(message: impl Into<String>) -> Self {
        Self {
            kind: RunErrorKind::Io,
            message: message.into(),
            cleanup_unverified: true,
        }
    }

    fn code(&self) -> u8 {
        match self.kind {
            RunErrorKind::Unclassified => 1,
            RunErrorKind::InvalidInput => 2,
            RunErrorKind::Io | RunErrorKind::Dependency => 3,
            RunErrorKind::Cancelled => 4,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self.kind {
            RunErrorKind::Unclassified => "unclassified",
            RunErrorKind::InvalidInput => "invalid_input",
            RunErrorKind::Io => "io",
            RunErrorKind::Dependency => "dependency",
            RunErrorKind::Cancelled => "cancelled",
        }
    }
}

impl From<String> for RunError {
    fn from(message: String) -> Self {
        Self::unclassified(message)
    }
}

impl From<&str> for RunError {
    fn from(message: &str) -> Self {
        Self::unclassified(message)
    }
}

/// Maximum output height for a requested width: 9/10 of it, rounded down to even
/// (400 -> 360, 800 -> 720, the TVC level 3 maximum).
fn max_height_for_width(width: u32) -> u32 {
    (width * 9 / 10) & !1
}

fn profile_filter(width: u32) -> String {
    format!(
        "scale={width}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24",
        max_height_for_width(width)
    )
}

/// Frames per segment: 480 at 400x360, fewer for larger pictures (as in the C-encoder path).
fn segment_frames(width: u32) -> usize {
    let h = max_height_for_width(width);
    ((480u32 * 400 * 360 / (width * h).max(1)).clamp(1, 480)) as usize
}

fn classify_c_encoder_error(error: c_encoder::EncodeError) -> RunError {
    match error {
        c_encoder::EncodeError::InvalidInput(message) => RunError::invalid_input(message),
        c_encoder::EncodeError::Spawn(_)
        | c_encoder::EncodeError::Io { .. }
        | c_encoder::EncodeError::MissingReport(_)
        | c_encoder::EncodeError::MissingVideo(_)
        | c_encoder::EncodeError::PipeUnavailable(_) => RunError::io(error.to_string()),
        _ => RunError::unclassified(error.to_string()),
    }
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|error| {
        format!(
            "Could not open output for hashing '{}': {error}",
            path.display()
        )
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not hash output '{}': {error}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

// Describes settings passed to the selected workflow, not proof that a
// caller-supplied external script or tool honors them.
fn effective_settings(args: &Args) -> serde_json::Value {
    serde_json::json!({
        "interpretation": "requested-workflow-settings-not-measured-output",
        "external_tools_may_ignore_settings": true,
        "publication": if args.allow_nonatomic_copy {"atomic-preferred-nonatomic-copy-opt-in"} else {"atomic-no-replace-required"},
        "video": {
            "requested_qp": args.qp,
            "qp_may_be_raised_for_level": true,
            "max_width": args.width,
            "max_height": max_height_for_width(args.width),
            "scaling": "aspect-fit-even",
            "fps_num": 24,
            "fps_den": 1,
            "level": if args.level == 0 { serde_json::json!("automatic") } else { serde_json::json!(args.level) },
            "frames_per_segment": segment_frames(args.width),
            "requested_jobs": args.jobs,
            "worker_jobs_limit": if args.rust_c_orchestrator {args.jobs.clamp(1, 6)} else {args.jobs}
        },
        "audio": if args.pocket_sidecar {Some(serde_json::json!({
            "format": "mp3-sidecar", "sample_rate": 48000,
            "channels": 2, "bit_rate": 128000
        }))} else {None},
        "projected_workspace_budget_bytes": if args.rust_c_orchestrator {Some(args.workspace_mib * 1024 * 1024)} else {None}
    })
}

fn retry_description(rust_c_orchestrator: bool, count: u32) -> String {
    if rust_c_orchestrator {
        format!("{count} joint segment retry(s)")
    } else {
        format!("{count} picture(s) raised for level limits")
    }
}

#[derive(Debug)]
enum ArgError {
    Usage(String),
    Dependency(String),
}

impl ArgError {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
            Self::Dependency(_) => "dependency",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Usage(message) => message,
            Self::Dependency(message) => message,
        }
    }

    fn code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Dependency(_) => 3,
        }
    }

    #[cfg(test)]
    fn contains(&self, needle: &str) -> bool {
        self.message().contains(needle)
    }
}

impl From<String> for ArgError {
    fn from(message: String) -> Self {
        Self::Usage(message)
    }
}

impl From<&str> for ArgError {
    fn from(message: &str) -> Self {
        Self::Usage(message.to_owned())
    }
}

impl std::fmt::Display for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

fn parse_args<I>(iter: I) -> Result<Option<Args>, ArgError>
where
    I: IntoIterator<Item = OsString>,
{
    parse_args_inner(iter)
}

fn parse_args_inner<I>(iter: I) -> Result<Option<Args>, ArgError>
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
    let mut width = 400u32;
    let mut level = 0u8;
    let mut jobs = 1;
    let mut workspace_mib = 512u64;
    let mut workspace_mib_explicit = false;
    let mut pocket_sidecar = true;
    let mut rust_c_orchestrator = false;
    let mut json_events = false;
    let mut control_stdin = false;
    let mut allow_nonatomic_copy = false;
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
        if key_text == "--json-events" {
            json_events = true;
            continue;
        }
        if key_text == "--control-stdin" {
            control_stdin = true;
            continue;
        }
        if key_text == "--allow-nonatomic-copy" {
            allow_nonatomic_copy = true;
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
            match EncoderProfile::resolve(&value)? {
                EncoderProfile::PocketSidecarV1 => pocket_sidecar = true,
            }
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
            "--width" => {
                width = value
                    .to_str()
                    .ok_or("width must be UTF-8")?
                    .parse()
                    .map_err(|_| "width must be an even integer from 16 to 800")?
            }
            "--level" => {
                level = value
                    .to_str()
                    .ok_or("level must be UTF-8")?
                    .parse()
                    .map_err(|_| "level must be 0 (automatic), 1, 2, or 3")?
            }
            "--jobs" => {
                jobs = value
                    .to_str()
                    .ok_or("jobs must be UTF-8")?
                    .parse()
                    .map_err(|_| "jobs must be a positive integer")?
            }
            _ => return Err(format!("unknown option: {key_text}\n{}", usage()).into()),
        }
    }
    if qp > 51 || jobs == 0 {
        return Err("QP must be 0–51 and jobs must be positive".into());
    }
    if !(16..=800).contains(&width) || width % 2 != 0 || level > 3 {
        return Err("width must be an even integer from 16 to 800 and level 0 (automatic), 1, 2, or 3".into());
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
    if control_stdin && !rust_c_orchestrator {
        return Err("--control-stdin requires --rust-c-orchestrator".into());
    }
    if pocket_sidecar && output.extension() != Some(OsStr::new("tnv3")) {
        return Err("The pocket-sidecar profile requires an OUTPUT path ending in .tnv3".into());
    }
    // An explicit script makes the checkout optional. Explicit TVC/TNV3 helpers
    // are also sufficient when the encoder script is supplied from a bundle.
    let explicit_root = codec_root.is_some();
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
        python.ok_or_else(|| {
            ArgError::Dependency("Python 3 not found on PATH; use --python".into())
        })?
    };
    let ffmpeg = ffmpeg
        .map(resolve_executable_arg)
        .or_else(|| adjacent_executable("ffmpeg"))
        .or_else(|| on_path("ffmpeg"))
        .ok_or_else(|| ArgError::Dependency("FFmpeg not found on PATH; use --ffmpeg".into()))?;
    let ffprobe = ffprobe
        .map(resolve_executable_arg)
        .or_else(|| adjacent_executable("ffprobe"))
        .or_else(|| on_path("ffprobe"));
    let ffprobe = if rust_c_orchestrator {
        ffprobe.unwrap_or_default()
    } else {
        ffprobe.ok_or_else(|| {
            ArgError::Dependency("FFprobe not found on PATH; use --ffprobe".into())
        })?
    };
    let script = match script {
        Some(path) => path,
        None if rust_c_orchestrator => root
            .as_ref()
            .map(|root| root.join("tools/tnv3_ref/encode_film.py"))
            .unwrap_or_default(),
        None => root.as_ref().ok_or_else(|| ArgError::Dependency("Could not find Tau codecs checkout; use --codec-root, --script, or TAU_CODECS_ROOT".into()))?
            .join("tools/tnv3_ref/encode_film.py"),
    };
    Ok(Some(Args {
        input,
        output,
        python,
        script,
        ffmpeg,
        ffprobe,
        tvc_encoder: tvc_encoder.map(resolve_executable_arg).or_else(|| {
            select_codec_tool(
                root.as_deref(),
                explicit_root,
                "tvc_enc",
                "tools/tvc_ref/tvc_enc",
            )
        }),
        tnv3_tool: tnv3_tool.map(resolve_executable_arg).or_else(|| {
            select_codec_tool(
                root.as_deref(),
                explicit_root,
                "tnv3",
                "tools/tnv3_ref/tnv3",
            )
        }),
        qp,
        width,
        level,
        jobs,
        workspace_mib,
        pocket_sidecar,
        rust_c_orchestrator,
        json_events,
        control_stdin,
        allow_nonatomic_copy,
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
fn run(_args: Args) -> Result<(), RunError> {
    Err(RunError::unclassified(
        "Atomic no-replace publication is unsupported on this platform in this prototype",
    ))
}

fn claim_publication(state: &AtomicU8) -> Result<(), RunError> {
    state
        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| RunError::cancelled("encoding stopped before output publication"))
}

fn cancellation_plan(args: &Args) -> &'static str {
    if !args.rust_c_orchestrator {
        "unsupported"
    } else if args.control_stdin {
        if cfg!(unix) {
            "sigint-or-stdin-eof-direct-leaf"
        } else {
            "stdin-eof-direct-leaf"
        }
    } else if cfg!(unix) {
        "sigint-direct-leaf"
    } else {
        "unsupported"
    }
}

fn register_active_stop(
    state: &AtomicU8,
    active_stop: &std::sync::Mutex<Option<mpsc::Sender<()>>>,
    sender: mpsc::Sender<()>,
) {
    if let Ok(mut slot) = active_stop.lock() {
        *slot = Some(sender.clone());
        // SIGINT can win before the child has been registered. Recheck while
        // holding the same mutex used by the signal listener.
        if state.load(Ordering::Acquire) == 1 {
            let _ = sender.send(());
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn run(args: Args) -> Result<(), RunError> {
    #[cfg(windows)]
    if !args.allow_nonatomic_copy {
        return Err(RunError::io(
            "Windows no-replace atomic publication is not implemented; refusing to encode without explicit --allow-nonatomic-copy. Windows runtime and temp-directory ACL behavior are unverified.",
        ));
    }
    let started = std::time::Instant::now();
    // 0 = running, 1 = cancellation won, 2 = publication won. This atomic
    // transition is the cancellation/publication linearization point.
    let control_state = std::sync::Arc::new(AtomicU8::new(0));
    let stop_token = std::sync::Arc::new(AtomicBool::new(false));
    let active_stop = std::sync::Arc::new(std::sync::Mutex::new(None::<mpsc::Sender<()>>));
    #[cfg(unix)]
    let _sigint = if args.rust_c_orchestrator {
        Some(SigintCancellation::install(
            std::sync::Arc::clone(&control_state),
            std::sync::Arc::clone(&stop_token),
            std::sync::Arc::clone(&active_stop),
        )?)
    } else {
        None
    };
    #[cfg(windows)]
    let _sigint: Option<()> = None;
    required_file(&args.input, "Input").map_err(RunError::io)?;
    required_file(&args.ffmpeg, "FFmpeg").map_err(RunError::dependency)?;
    if !args.rust_c_orchestrator {
        required_file(&args.python, "Python").map_err(RunError::dependency)?;
        required_file(&args.script, "Encoder script").map_err(RunError::dependency)?;
        required_file(&args.ffprobe, "FFprobe").map_err(RunError::dependency)?;
    }
    validate_optional_file(args.tvc_encoder.as_deref(), "TVC encoder")
        .map_err(RunError::dependency)?;
    validate_optional_file(args.tnv3_tool.as_deref(), "TNV3 tool").map_err(RunError::dependency)?;
    if args.output.exists() {
        return Err(RunError::io(format!(
            "Output already exists: {}",
            args.output.display()
        )));
    }
    let audio_output = args.output.with_extension("mp3");
    if args.pocket_sidecar && audio_output.exists() {
        return Err(RunError::io(format!(
            "Audio output already exists: {}",
            audio_output.display()
        )));
    }
    let parent = args
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(RunError::io(format!(
            "Output directory does not exist: {}",
            parent.display()
        )));
    }

    if args.json_events {
        emit_event(serde_json::json!({
            "schema": 1, "event": "plan", "profile": {"id": if args.pocket_sidecar {"pocket-sidecar"} else {"video-only"}, "version": 1},
            "input": args.input.display().to_string(), "files": if args.pocket_sidecar {2} else {1},
            "qp": args.qp, "jobs": args.jobs,
            "effective_settings": effective_settings(&args),
            "backend": if args.rust_c_orchestrator {"experimental-rust-c-orchestrator"} else {"python-c"},
            "resume": false, "cancellation": cancellation_plan(&args),
            "publication": if args.allow_nonatomic_copy {"atomic-preferred-nonatomic-copy-opt-in"} else {"atomic-no-replace-required"},
            "publication_warning": if args.allow_nonatomic_copy {Some("If no-replace rename and hard-link publication fail, exclusive-create copy may expose a partial output on write failure or interruption; the output pair is not atomic.")} else {None}
        })).map_err(RunError::io)?;
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
    let mut staged =
        publication::ProtectedStage::create_with_policy(parent, args.allow_nonatomic_copy)
            .map_err(RunError::io)?;
    let progress_error = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let (frames, level, retries, reencoded, reencoded_kind, encoder_diagnostics) = if args
        .rust_c_orchestrator
    {
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
                profile_filter(args.width).as_str(),
                "-pix_fmt",
                "yuv420p",
                "-f",
                "yuv4mpegpipe",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let source = match child_supervisor::spawn_source(&mut ffmpeg) {
            Ok(source) => source,
            Err(error) => {
                let path = staged.retain();
                return Err(RunError::unverified(format!(
                    "Could not start/supervise FFmpeg Y4M decode; cleanup unverified: {error}; staging retained at {}",
                    path.display()
                )));
            }
        };
        let stop_sender = source.stop.clone();
        register_active_stop(&control_state, &active_stop, stop_sender.clone());
        if args.control_stdin {
            let state = std::sync::Arc::clone(&control_state);
            let token = std::sync::Arc::clone(&stop_token);
            let active = std::sync::Arc::clone(&active_stop);
            let watcher = std::thread::Builder::new().name("tau-control-stdin".into()).spawn(move || {
                let stdin = std::io::stdin();
                let mut input = stdin.lock();
                let mut buffer = [0u8; 256];
                loop {
                    match input.read(&mut buffer) {
                        Ok(0) => {
                            if state.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                                token.store(true, Ordering::Release);
                                if let Ok(slot) = active.lock() { if let Some(stop) = slot.as_ref() { let _ = stop.send(()); } }
                            }
                            break;
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            if state.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                                token.store(true, Ordering::Release);
                                if let Ok(slot) = active.lock() { if let Some(stop) = slot.as_ref() { let _ = stop.send(()); } }
                            }
                            let _ = writeln!(std::io::stderr().lock(), "control stdin read failed; treating it as cancellation: {error}");
                            break;
                        }
                    }
                }
            });
            if let Err(error) = watcher {
                let _ = source.stop.send(());
                return match source.finished.join() {
                    Ok(Ok(_)) => Err(RunError::io(format!(
                        "Could not start control-stdin reader: {error}"
                    ))),
                    result => {
                        let path = staged.retain();
                        Err(RunError::unverified(format!(
                            "Could not start control-stdin reader: {error}; source cleanup unverified ({result:?}); staging retained at {}",
                            path.display()
                        )))
                    }
                };
            }
        }
        let stdout = source.stdout;
        let stderr = source.stderr;
        let (diagnostics_tx, diagnostics_rx) = mpsc::channel();
        let diagnostic_thread = std::thread::Builder::new()
            .name("tau-source-diagnostics".into())
            .spawn(move || {
                let _ = diagnostics_tx.send(drain_bounded_stderr(stderr));
            });
        if let Err(error) = diagnostic_thread {
            let _ = source.stop.send(());
            return match source.finished.join() {
                Ok(Ok(_)) => Err(RunError::io(format!(
                    "Could not start source diagnostic reader: {error}"
                ))),
                result => {
                    let path = staged.retain();
                    Err(RunError::unverified(format!(
                        "Could not start source diagnostic reader: {error}; source cleanup unverified ({result:?}); staging retained at {}",
                        path.display()
                    )))
                }
            };
        }
        let tools = RustCTools {
            tvc_encoder: tvc_encoder.clone(),
            tnv3: tnv3_tool.clone(),
        };
        let progress_error_slot = std::sync::Arc::clone(&progress_error);
        let encoded = experimental_c_orchestrator::encode_y4m_video_with_control_and_quiet_children(
            &tools,
            std::io::BufReader::new(stdout),
            staged.writer(),
            RustCOptions {
                qp: args.qp,
                jobs: args.jobs.clamp(1, 6),
                // 480 at the default size, scaled down for larger pictures.
                frames_per_segment: segment_frames(args.width),
                level: args.level,
                workspace_limit_bytes: args.workspace_mib * 1024 * 1024,
                ..RustCOptions::default()
            },
            Some(&stop_token),
            args.json_events,
            |frames| {
                if args.json_events {
                    if let Ok(mut slot) = progress_error_slot.lock() {
                        if slot.is_none() {
                            if let Err(error) = emit_event(serde_json::json!({
                                "schema": 1, "event": "progress", "frames": frames, "total": null,
                                "segment": frames.div_ceil(segment_frames(args.width) as u64), "bytes": null, "elapsed": started.elapsed().as_secs_f64()
                            })) {
                                *slot = Some(error);
                            }
                        }
                    }
                }
            },
        );
        let report = match encoded {
            Ok(report) => report,
            Err(error) => {
                let _ = source.stop.send(());
                let status = match source.finished.join() {
                    Ok(Ok(status)) => status,
                    Ok(Err(wait_error)) => {
                        let path = staged.retain();
                        return Err(RunError::unverified(format!(
                            "{error}; could not reap FFmpeg: {wait_error}; staging retained at {}",
                            path.display()
                        )));
                    }
                    Err(_) => {
                        let path = staged.retain();
                        return Err(RunError::unverified(format!(
                            "{error}; FFmpeg supervisor panicked; staging retained at {}",
                            path.display()
                        )));
                    }
                };
                if error.contains("cleanup is unverified") {
                    let path = staged.retain();
                    return Err(RunError::unverified(format!(
                        "{error}; staging retained at {}",
                        path.display()
                    )));
                }
                let detail = match diagnostics_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(Ok(detail)) => detail,
                    Ok(Err(read_error)) => {
                        return Err(RunError::io(format!(
                            "{error}; diagnostic read failed: {read_error}"
                        )));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let path = staged.retain();
                        return Err(RunError::unverified(format!(
                            "{error}; FFmpeg stderr remained open; cleanup unverified, staging retained at {}",
                            path.display()
                        )));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(RunError::io(format!("{error}; diagnostic reader failed")));
                    }
                };
                if control_state.load(Ordering::Acquire) == 1 {
                    return Err(RunError::cancelled(format!(
                        "encoding stopped before output publication: {error}; FFmpeg exited ({status}): {detail}"
                    )));
                }
                return Err(RunError::io(format!(
                    "{error}; FFmpeg was stopped after the Y4M stream error ({status}): {detail}"
                )));
            }
        };
        let status = match source.finished.join() {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => {
                let path = staged.retain();
                return Err(RunError::unverified(format!(
                    "Could not reap FFmpeg: {error}; staging retained at {}",
                    path.display()
                )));
            }
            Err(_) => {
                let path = staged.retain();
                return Err(RunError::unverified(format!(
                    "FFmpeg supervisor panicked; staging retained at {}",
                    path.display()
                )));
            }
        };
        let detail = match diagnostics_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => result.map_err(RunError::io)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let path = staged.retain();
                return Err(RunError::unverified(format!(
                    "FFmpeg stderr remained open after exit; cleanup unverified, staging retained at {}",
                    path.display()
                )));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(RunError::io("Could not collect FFmpeg diagnostics"));
            }
        };
        if let Ok(mut slot) = active_stop.lock() {
            *slot = None;
        }
        if control_state.load(Ordering::Acquire) == 1 {
            return Err(RunError::cancelled(
                "encoding stopped before output publication",
            ));
        }
        if !status.success() {
            return Err(RunError::io(format!(
                "FFmpeg Y4M decode failed ({status}): {detail}"
            )));
        }
        if let Some(error) = progress_error.lock().ok().and_then(|slot| slot.clone()) {
            return Err(RunError::io(format!(
                "JSON progress reporting failed after encoder children were reaped; staged output was discarded: {error}"
            )));
        }
        (
            report.frames,
            report.level,
            format!(
                "{} segment(s); {}",
                report.segments,
                retry_description(true, report.joint_reencodes)
            ),
            report.joint_reencodes,
            "joint_segments",
            detail,
        )
    } else {
        let progress_error_slot = std::sync::Arc::clone(&progress_error);
        let report = c_encoder::encode_video_with_geometry_typed(
            &tools,
            &args.input,
            staged.writer(),
            Options {
                qp: args.qp,
                jobs: args.jobs,
                ..Options::default()
            },
            args.width,
            max_height_for_width(args.width),
            args.level,
            |done, total| {
                if args.json_events {
                    if let Ok(mut slot) = progress_error_slot.lock() {
                        if slot.is_none() {
                            if let Err(error) = emit_event(
                                serde_json::json!({"schema": 1, "event": "progress", "frames": null, "total": null, "segment": done, "segments": total, "bytes": null, "elapsed": started.elapsed().as_secs_f64()}),
                            ) {
                                *slot = Some(error);
                            }
                        }
                    }
                } else {
                    eprintln!("segments: {done}/{total}");
                }
            },
        )
        .map_err(classify_c_encoder_error)?;
        if let Some(error) = progress_error.lock().ok().and_then(|slot| slot.clone()) {
            return Err(RunError::io(format!(
                "JSON progress reporting failed after encoder process was reaped; staged output was discarded: {error}"
            )));
        }
        (
            report.frames,
            report.level,
            retry_description(false, report.reencoded),
            report.reencoded,
            "pictures_raised_for_level",
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
        let audio_result = if args.rust_c_orchestrator {
            run_capturing_stderr_controlled(
                &mut command,
                "FFmpeg MP3 encoding",
                &stop_token,
                &active_stop,
            )
        } else {
            run_capturing_stderr(&mut command, "FFmpeg MP3 encoding")
        };
        let (status, detail) = match audio_result {
            Ok(result) => result,
            Err(mut error) => {
                if error.cleanup_unverified {
                    let path = staged.retain();
                    error
                        .message
                        .push_str(&format!("; staging retained at {}", path.display()));
                }
                return Err(error);
            }
        };
        if !status.success() {
            return Err(RunError::io(format!(
                "MP3 encoding failed ({}): {}",
                status, detail
            )));
        }
        let audio_name = audio_output
            .file_name()
            .ok_or("Audio output path must include a file name")?;
        if args.rust_c_orchestrator {
            claim_publication(&control_state)?;
        }
        let (video_len, audio_len, warning) = staged
            .publish_pair(output_name, audio_name)
            .map_err(RunError::io)?;
        let audio_warning = diagnostics_warning("MP3 encoder", &detail);
        (video_len, audio_len, warning, audio_warning)
    } else {
        if args.rust_c_orchestrator {
            claim_publication(&control_state)?;
        }
        let (video_len, warning) = staged.publish(output_name).map_err(RunError::io)?;
        (video_len, 0, warning, None)
    };
    let published_files = if args.pocket_sidecar {
        vec![args.output.clone(), audio_output.clone()]
    } else {
        vec![args.output.clone()]
    };
    if !args.json_events {
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
    }
    if !encoder_diagnostics.is_empty() {
        eprintln!("Encoder diagnostics:\n{}", encoder_diagnostics);
    }
    if let Some(warning) = sync_warning {
        if args.json_events {
            emit_event(serde_json::json!({"schema": 1, "event": "warning", "message": warning}))
                .map_err(|error| {
                    RunError::io(format!(
                        "outputs were published at {}; warning event could not be written: {error}",
                        display_paths(&published_files)
                    ))
                })?;
        } else {
            eprintln!("Warning: {warning}");
        }
    }
    if let Some(warning) = audio_warning {
        if args.json_events {
            emit_event(serde_json::json!({"schema": 1, "event": "warning", "message": warning}))
                .map_err(|error| {
                    RunError::io(format!(
                        "outputs were published at {}; warning event could not be written: {error}",
                        display_paths(&published_files)
                    ))
                })?;
        } else {
            eprintln!("Warning: {warning}");
        }
    }
    if args.json_events {
        let hashes = published_files
            .iter()
            .map(|path| sha256_file(path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                RunError::io(format!(
                    "outputs were published at {}; {error}",
                    display_paths(&published_files)
                ))
            })?;
        let files = published_files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        emit_event(serde_json::json!({
            "schema": 1, "event": "done", "files": files,
            "sizes": if args.pocket_sidecar {vec![byte_size, audio_byte_size]} else {vec![byte_size]},
            "sha256": hashes, "level": level,
            "reencoded": reencoded, "reencoded_kind": reencoded_kind,
            "elapsed": started.elapsed().as_secs_f64()
        })).map_err(|error| RunError::io(format!("outputs were published at {}; done event could not be written: {error}", display_paths(&published_files))))?;
    }
    Ok(())
}

fn run_info(args: &[OsString]) -> ExitCode {
    let json = args.iter().any(|a| a == OsStr::new("--json"));
    let json_last = args.last().is_some_and(|a| a == OsStr::new("--json"));
    let result: Result<String, (u8, String)> = (|| {
        if !(args.len() == 2 || (args.len() == 3 && json_last)) || args[1] == OsStr::new("--json") {
            return Err((2, "Usage: tau-codecs-cli info FILE [--json]".into()));
        }
        let file =
            std::fs::File::open(&args[1]).map_err(|e| (3, format!("Could not open input: {e}")))?;
        let report = info::inspect(file).map_err(|e| {
            let code = match &e {
                tau_video::Error::Io(io) if io.kind() != std::io::ErrorKind::UnexpectedEof => 3,
                _ => 1,
            };
            (code, e.to_string())
        })?;
        Ok(if json {
            report.to_json().to_string()
        } else {
            report.to_human()
        })
    })();
    match result {
        Ok(line) => match write_stdout_line(&line) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Could not write info output: {e}");
                ExitCode::from(3)
            }
        },
        Err((code, message)) => {
            if json {
                let kind = match code {
                    1 => "format",
                    2 => "usage",
                    3 => "io",
                    _ => "unclassified",
                };
                let value = serde_json::json!({"schema": 1, "command": "info", "error": {"code": code, "kind": kind, "message": message}});
                if let Err(e) = write_stdout_line(&value.to_string()) {
                    eprintln!("Could not write info error: {e}");
                    return ExitCode::from(3);
                }
            } else {
                eprintln!("{message}");
            }
            ExitCode::from(code)
        }
    }
}

fn main() -> ExitCode {
    let argv: Vec<OsString> = env::args_os().skip(1).collect();
    if argv
        .first()
        .is_some_and(|arg| arg == OsStr::new("handshake"))
    {
        return handshake::run(&argv);
    }
    match tool_identity::tool_identity_command(&argv) {
        Ok(Some(report)) => {
            return match write_stdout_line(&report.to_string()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "Could not write tool identity output: {error}"
                    );
                    ExitCode::from(3)
                }
            };
        }
        Ok(None) => {}
        Err((code, message)) => {
            let report =
                serde_json::json!({"schema":1,"event":"error","code":code,"message":message});
            if let Err(error) = write_stdout_line(&report.to_string()) {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "Could not write tool identity error: {error}"
                );
                return ExitCode::from(3);
            }
            return ExitCode::from(code);
        }
    }
    if argv.first().is_some_and(|arg| arg == OsStr::new("probe")) {
        return probe_command::run(&argv);
    }
    if argv.first().is_some_and(|arg| arg == OsStr::new("check")) {
        return check::run(&argv);
    }
    if argv
        .first()
        .is_some_and(|arg| arg == OsStr::new("decode-audio"))
    {
        return decode_audio::run(&argv);
    }
    if argv.first().is_some_and(|arg| arg == OsStr::new("encode-audio")) {
        return encode_audio::run(&argv);
    }
    if argv.first().is_some_and(|arg| arg == OsStr::new("mux")) {
        return mux_command::run(&argv);
    }
    if argv.first().is_some_and(|arg| arg == OsStr::new("info")) {
        return run_info(&argv);
    }
    if argv
        .first()
        .is_some_and(|arg| arg == OsStr::new("--version"))
    {
        let version_line = match argv.as_slice() {
            [version] if version == OsStr::new("--version") => {
                Some(format!("tau-codecs-cli {}", env!("CARGO_PKG_VERSION")))
            }
            [version, json]
                if version == OsStr::new("--version") && json == OsStr::new("--json") =>
            {
                Some(version_json().to_string())
            }
            _ => None,
        };
        if let Some(line) = version_line {
            return match write_stdout_line(&line) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Could not write version output: {error}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    match parse_args(argv) {
        Ok(None) => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Err(error) => {
            if std::env::args_os().any(|arg| arg == OsStr::new("--json-events")) {
                let status = error.code();
                if let Err(write_error) = emit_event(
                    serde_json::json!({"schema": 1, "event": "error", "code": status, "kind": error.kind_name(), "message": error.message()}),
                ) {
                    eprintln!("{write_error}");
                }
            } else {
                eprintln!("{error}");
            }
            ExitCode::from(error.code())
        }
        Ok(Some(args)) => match run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                let status = error.code();
                if std::env::args_os().any(|arg| arg == OsStr::new("--json-events")) {
                    if let Err(write_error) = emit_event(
                        serde_json::json!({"schema": 1, "event": "error", "code": status, "kind": error.kind_name(), "message": error.message}),
                    ) {
                        eprintln!("{}; {write_error}", error.message);
                    }
                } else {
                    eprintln!("{}", error.message);
                }
                ExitCode::from(status)
            }
        },
    }
}

/// Keep the cross-process contract separate from `--version`: callers may
/// explicitly identify selected executables. Version switches run only when
/// `--probe-versions` is supplied; their output is retained as unvalidated
/// response text, not treated as a compatibility or version claim.
mod handshake {
    use super::*;

    pub fn run(argv: &[OsString]) -> ExitCode {
        if argv.get(1).is_none_or(|arg| arg != OsStr::new("--json")) {
            return fail(2, "handshake requires --json immediately after the command");
        }

        // Reuse the existing strict, explicit-path identity implementation.
        // No options means no tools have been selected or inspected.
        let probe_versions = argv
            .iter()
            .skip(2)
            .any(|arg| arg == OsStr::new("--probe-versions"));
        let path_args: Vec<OsString> = argv
            .iter()
            .skip(2)
            .filter(|arg| *arg != OsStr::new("--probe-versions"))
            .cloned()
            .collect();
        let mut tools = if path_args.is_empty() {
            serde_json::json!({})
        } else {
            let mut identity_argv = vec![OsString::from("tool-identity"), OsString::from("--json")];
            identity_argv.extend(path_args);
            match tool_identity::tool_identity_command(&identity_argv) {
                Ok(Some(report)) => report["tools"].clone(),
                Ok(None) => unreachable!("tool-identity command must be recognized"),
                Err((code, message)) => return fail(code, &message),
            }
        };

        if probe_versions {
            if tools.as_object().is_none_or(|map| map.is_empty()) {
                return fail(
                    2,
                    "--probe-versions requires at least one explicit tool path",
                );
            }
            tool_version::probe_explicit_tools(&mut tools);
        }

        let report = serde_json::json!({
            "schema": 1,
            "command": "handshake",
            "cli": {"name": "tau-codecs-cli", "version": env!("CARGO_PKG_VERSION")},
            "formats": {
                "tnv3_container": {
                    "version": tau_video::TNV3_VERSION,
                    "read_scope": "header_crc_and_declared_index_layout; selected commands inspect further bounded structures",
                    "write_scope": "video_only_from_TVC1; audio_and_subtitle_muxing_are_not_implemented_by_the_Rust_crate"
                },
                "tvc_elementary": {
                    "read_magics": ["TVC0", "TVC1"],
                    "write_magics": ["TVC1"],
                    "read_scope": "bounded_decode_to_Y4M; not_full_conformance"
                },
                "tac_elementary": {"read": false, "write": false},
                "subtitle_structure": {"read_scope": "bounded_TNV3_subtitle_structure_check_only", "write": false}
            },
            "tools": tools,
            "tools_status": if tools.as_object().is_some_and(|v| v.is_empty()) { "not_selected" } else if probe_versions { "explicit_files_hashed_and_optional_version_queries_attempted" } else { "explicit_files_hashed_versions_not_exposed" },
            "version_query_scope": if probe_versions { "bounded_version_switch_response_only; no compatibility claim; query is separate from and does not bind a later execution" } else { "not_requested" },
            "tool_identity_scope": "selected file bytes hashed with metadata stability checks; the hash does not bind a later execution"
        });
        match write_stdout_line(&report.to_string()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Could not write handshake output: {error}");
                ExitCode::from(3)
            }
        }
    }

    fn fail(code: u8, message: &str) -> ExitCode {
        let report = serde_json::json!({
            "schema": 1,
            "command": "handshake",
            "error": {"code": code, "message": message}
        });
        if let Err(error) = write_stdout_line(&report.to_string()) {
            eprintln!("Could not write handshake error: {error}");
            return ExitCode::from(3);
        }
        ExitCode::from(code)
    }
}

/// Version facts available without probing the host's external toolchain.
/// Null tool versions are intentional: this command describes the CLI build,
/// not whichever executables may later be selected by PATH or explicit flags.
fn version_json() -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "cli": { "name": "tau-codecs-cli", "version": env!("CARGO_PKG_VERSION") },
        "formats": { "tnv3_container": tau_video::TNV3_VERSION },
        "tools": {
            "tvc_enc": null,
            "tnv3": null,
            "ffmpeg": null,
            "ffprobe": null
        },
        "tools_status": "not_probed"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_already_requested_before_child_registration_is_delivered() {
        let state = AtomicU8::new(1);
        let slot = std::sync::Mutex::new(None);
        let (sender, receiver) = mpsc::channel();
        register_active_stop(&state, &slot, sender);
        receiver.recv_timeout(Duration::from_millis(50)).unwrap();
    }

    #[test]
    fn c_encoder_invalid_input_has_stable_cli_classification() {
        use super::*;

        let error = classify_c_encoder_error(c_encoder::EncodeError::InvalidInput(
            "QP must be in range".into(),
        ));

        assert_eq!(error.kind_name(), "invalid_input");
        assert_eq!(error.code(), 2);
        assert_eq!(error.message, "QP must be in range");
        assert!(!error.cleanup_unverified);
    }

    #[test]
    fn explicit_codec_root_does_not_fallback_to_another_installation() {
        let root = Path::new("/explicit-selected-root");
        let got = select_codec_tool(Some(root), true, "tvc_enc", "tools/tvc_ref/tvc_enc").unwrap();
        let expected = root.join("tools/tvc_ref/tvc_enc");
        let expected = if cfg!(windows) {
            expected.with_extension("exe")
        } else {
            expected
        };
        assert_eq!(got, expected);
    }
    #[cfg(unix)]
    #[test]
    fn adjacent_tool_requires_an_executable_regular_file() {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "tau-adjacent-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let tool = dir.join("tvc_enc");
        std::fs::write(&tool, b"test").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(executable_in_dir(&dir, "tvc_enc").is_none());
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(executable_in_dir(&dir, "tvc_enc"), Some(tool));
        assert!(executable_in_dir(&dir, "tnv3").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn unverified_cleanup_keeps_staging_for_recovery() {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "tau-retain-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut stage = publication::ProtectedStage::create(&root).unwrap();
        std::io::Write::write_all(stage.writer(), b"retained").unwrap();
        let path = stage.retain();
        drop(stage);
        assert_eq!(
            std::fs::read(path.join("payload.tnv3")).unwrap(),
            b"retained"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn width_and_level_options_are_parsed_validated_and_reported() {
        let parse = |extra: &[&str]| {
            let mut argv = vec!["encode", "in.mp4", "out.tnv3"];
            argv.extend_from_slice(extra);
            parse_args(argv.into_iter().map(OsString::from))
        };
        let default = parse(&[]).unwrap().unwrap();
        assert_eq!((default.width, default.level), (400, 0));
        let settings = effective_settings(&default);
        assert_eq!(settings["video"]["max_width"], 400);
        assert_eq!(settings["video"]["max_height"], 360);
        assert_eq!(settings["video"]["level"], "automatic");
        assert_eq!(segment_frames(400), 480);

        let wide = parse(&["--width", "800", "--level", "2"]).unwrap().unwrap();
        let settings = effective_settings(&wide);
        assert_eq!(settings["video"]["max_width"], 800);
        assert_eq!(settings["video"]["max_height"], 720);
        assert_eq!(settings["video"]["level"], 2);
        assert_eq!(segment_frames(800), 120);
        assert_eq!(
            profile_filter(800),
            "scale=800:720:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24"
        );
        for bad in [["--width", "802"], ["--width", "15"], ["--width", "x"], ["--level", "4"]] {
            assert!(parse(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn effective_plan_distinguishes_requested_and_clamped_jobs() {
        let argv = [
            "encode",
            "input.mp4",
            "out.tnv3",
            "--rust-c-orchestrator",
            "--ffmpeg",
            "/ffmpeg",
            "--jobs",
            "20",
        ]
        .map(OsString::from);
        let args = parse_args(argv).unwrap().unwrap();
        assert_eq!(
            cancellation_plan(&args),
            if cfg!(unix) {
                "sigint-direct-leaf"
            } else {
                "unsupported"
            }
        );
        let settings = effective_settings(&args);
        assert_eq!(settings["video"]["requested_jobs"], 20);
        assert_eq!(settings["video"]["worker_jobs_limit"], 6);
        assert_eq!(settings["audio"]["format"], "mp3-sidecar");
        assert_eq!(
            settings["projected_workspace_budget_bytes"],
            512u64 * 1024 * 1024
        );
    }

    #[test]
    fn publication_flag_defaults_strict_and_is_visible_in_machine_plan_settings() {
        let mut argv = vec![
            "encode".into(),
            "input.mp4".into(),
            "out.tnv3".into(),
            "--video-only".into(),
        ];
        let strict = parse_args(argv.clone()).unwrap().unwrap();
        assert!(!strict.allow_nonatomic_copy);
        assert_eq!(
            effective_settings(&strict)["publication"],
            "atomic-no-replace-required"
        );
        argv.push("--allow-nonatomic-copy".into());
        let opted_in = parse_args(argv).unwrap().unwrap();
        assert!(opted_in.allow_nonatomic_copy);
        assert_eq!(
            effective_settings(&opted_in)["publication"],
            "atomic-preferred-nonatomic-copy-opt-in"
        );
    }
    #[test]
    fn control_eof_and_publication_are_mutually_exclusive() {
        use super::*;
        use std::sync::{Arc, Barrier};
        for _ in 0..200 {
            let state = Arc::new(AtomicU8::new(0));
            let barrier = Arc::new(Barrier::new(2));
            let worker_state = Arc::clone(&state);
            let worker_barrier = Arc::clone(&barrier);
            let cancel = std::thread::spawn(move || {
                worker_barrier.wait();
                worker_state
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            });
            barrier.wait();
            let publish = claim_publication(&state).is_ok();
            assert_ne!(publish, cancel.join().unwrap());
        }
    }
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
        assert!(usage().contains("tau-codecs-cli --version [--json]"));
    }

    #[test]
    fn json_events_flag_is_accepted_without_changing_defaults() {
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        argv.push("--json-events".into());
        let args = parse_args(argv).unwrap().unwrap();
        assert!(args.json_events);
        assert!(args.pocket_sidecar);
    }

    #[test]
    fn json_event_is_a_single_schema_versioned_line() {
        let event =
            serde_json::json!({"schema": 1, "event": "progress", "frames": 24, "total": null});
        let encoded = event.to_string();
        let decoded: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded["schema"], 1);
        assert_eq!(decoded["event"], "progress");
        assert_eq!(decoded["frames"], 24);
        assert!(decoded["total"].is_null());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_have_deliberate_lossy_json_representation() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(OsString::from_vec(b"name-\xff.tnv3".to_vec()));
        assert_eq!(display_paths(&[path]), "name-\u{fffd}.tnv3");
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
            width: 400,
            level: 0,
            jobs: 1,
            workspace_mib: 512,
            pocket_sidecar: false,
            rust_c_orchestrator: true,
            json_events: false,
            control_stdin: false,
            allow_nonatomic_copy: false,
        })
        .unwrap_err();
        assert!(
            error.message.contains("exit status: 9"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("Further diagnostics omitted"),
            "{}",
            error.message
        );
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
            width: 400,
            level: 0,
            jobs: 1,
            workspace_mib: 512,
            pocket_sidecar: false,
            rust_c_orchestrator: true,
            json_events: false,
            control_stdin: false,
            allow_nonatomic_copy: false,
        })
        .unwrap_err();
        assert!(
            error.message.contains("invalid Y4M stream"),
            "{}",
            error.message
        );
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
        argv.extend(["--profile".into(), "pocket-sidecar@1".into()]);
        assert!(parse_args(argv).unwrap().unwrap().pocket_sidecar);
        let mut argv = vec!["encode".into(), "in.mp4".into(), "out.tnv3".into()];
        argv.extend(required_options());
        argv.extend(["--profile".into(), "pocket-sidecar@2".into()]);
        assert!(matches!(parse_args(argv), Err(error) if error.contains("pocket-sidecar@1")));
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
            let mut video_stage =
                publication::ProtectedStage::create_with_policy(&dir, true).unwrap();
            video_stage
                .writer()
                .write_all(b"generated video-only TNV3")
                .unwrap();
            let (n, warning) = video_stage
                .publish(OsStr::new("generated-video-only.tnv3"))
                .unwrap();
            assert!(
                warning
                    .as_deref()
                    .is_some_and(|w| w.contains("Explicit --allow-nonatomic-copy"))
            );
            assert_eq!(n, b"generated video-only TNV3".len() as u64);
            assert_eq!(
                fs::read(dir.join("generated-video-only.tnv3")).unwrap(),
                b"generated video-only TNV3"
            );
            if let Some(w) = warning {
                println!("video-only warning: {w}");
            }

            let mut pair_stage =
                publication::ProtectedStage::create_with_policy(&dir, true).unwrap();
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
            assert!(
                warning
                    .as_deref()
                    .is_some_and(|w| w.contains("Explicit --allow-nonatomic-copy"))
            );
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
            let mut fail_stage =
                publication::ProtectedStage::create_with_policy(&dir, true).unwrap();
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
