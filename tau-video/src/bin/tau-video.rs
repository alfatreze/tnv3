use std::{
    env,
    fs::{File, OpenOptions},
    path::Path,
    process::ExitCode,
};
use tau_video::{
    Tnv3Reader,
    container::{decode_video_y4m, mux_tvc},
    tvc::{encoder::encode_y4m, stream::decode_y4m},
};
fn usage() -> ExitCode {
    eprintln!(
        "Usage: tau-video info FILE | tau-video decode-tvc INPUT.tvc OUTPUT.y4m | tau-video encode-tvc INPUT.y4m OUTPUT.tvc | tau-video mux-tvc INPUT.tvc OUTPUT.tnv3 | tau-video decode-tnv3 INPUT.tnv3 OUTPUT.y4m"
    );
    ExitCode::from(2)
}
fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(mode) = args.next() else {
        return usage();
    };
    let Some(input) = args.next() else {
        return usage();
    };
    if mode == "info" {
        if args.next().is_some() {
            return usage();
        }
        let file = match File::open(input) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(3);
            }
        };
        return match Tnv3Reader::open(file) {
            Ok(r) => {
                let i = r.info();
                println!(
                    "TNV3 {}x{}, {}/{}, level {}, {} frames, {} audio channels\nHeader and layout only; chunk contents have not been checked.",
                    i.width,
                    i.height,
                    i.fps_num,
                    i.fps_den,
                    i.level,
                    i.frame_count,
                    i.audio_channels
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        };
    }
    if mode != "decode-tvc" && mode != "encode-tvc" && mode != "mux-tvc" && mode != "decode-tnv3" {
        return usage();
    }
    let Some(output) = args.next() else {
        return usage();
    };
    if args.next().is_some() {
        return usage();
    }
    let file = match File::open(input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(3);
        }
    };
    let out = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(&output))
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(3);
        }
    };
    let result = if mode == "decode-tvc" {
        decode_y4m(file, out).map_err(|e| e.to_string())
    } else if mode == "decode-tnv3" {
        decode_video_y4m(file, out).map_err(|e| e.to_string())
    } else if mode == "mux-tvc" {
        mux_tvc(file, out).map(|_| ()).map_err(|e| e.to_string())
    } else {
        encode_y4m(std::io::BufReader::new(file), out, 30)
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            if let Err(remove) = std::fs::remove_file(&output) {
                eprintln!("Could not remove incomplete output: {remove}");
            }
            ExitCode::from(1)
        }
    }
}
