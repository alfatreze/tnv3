use std::{env, fs::File, process::ExitCode};
use tau_video::Tnv3Reader;
fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("info")) {
        eprintln!("Usage: tau-video info FILE");
        return ExitCode::from(2);
    }
    let Some(path) = args.next() else {
        eprintln!("Usage: tau-video info FILE");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("Usage: tau-video info FILE");
        return ExitCode::from(2);
    }
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(3);
        }
    };
    match Tnv3Reader::open(file) {
        Ok(r) => {
            let i = r.info();
            println!(
                "TNV3 {}x{}, {}/{}, level {}, {} frames, {} audio channels\nHeader and layout only; chunk contents have not been checked.",
                i.width, i.height, i.fps_num, i.fps_den, i.level, i.frame_count, i.audio_channels
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
