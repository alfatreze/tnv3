//! Manual check (ignored by default): how fast the Rust picture decoder runs on a real TNV3 file, and whether the
//! decoded pictures are plausible (mean luma). Usage:
//!   TAU_FILM=/path/film.tnv3 TAU_FRAMES=1000 cargo test --release --test decode_speed -- --ignored --nocapture
//! Run it with and without `--release`: a debug build (what `cargo tauri dev` runs) is far slower.
use std::{fs::File, time::Instant};
use tau_video::{Tnv3Reader, tvc::frame::PictureDecoder};

#[test]
#[ignore]
fn decode_speed_and_luma() {
    let Some(path) = std::env::var_os("TAU_FILM") else { panic!("set TAU_FILM to a .tnv3 file") };
    let frames: u32 = std::env::var("TAU_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let mut r = Tnv3Reader::open(File::open(&path).unwrap()).unwrap();
    let info = r.info().clone();
    let mut dec = PictureDecoder::new(info.width, info.height).unwrap();
    // Optional: TAU_DUMP_DIR=dir TAU_DUMP_AT=100,1827 writes those decoded frames as grey PGM files.
    let dump_dir = std::env::var_os("TAU_DUMP_DIR");
    let dump_at: Vec<u32> = std::env::var("TAU_DUMP_AT").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let mut out_index = 0u32;
    let start = Instant::now();
    let (mut shown, mut luma_sum, mut samples) = (0u32, 0u64, 0u32);
    let mut bright = Vec::new();
    for n in 0..frames.min(info.frame_count) {
        let pic = r.read_picture(n).unwrap();
        if let Some(f) = dec.decode(&pic).unwrap() {
            shown += 1;
            if let Some(dir) = &dump_dir {
                if dump_at.contains(&out_index) {
                    let mut pgm = format!("P5\n{} {}\n255\n", info.width, info.height).into_bytes();
                    pgm.extend(&f.y);
                    std::fs::write(std::path::Path::new(dir).join(format!("frame{out_index}.pgm")), pgm).unwrap();
                }
            }
            out_index += 1;
            let mean = f.y.iter().map(|&v| u64::from(v)).sum::<u64>() / f.y.len() as u64;
            luma_sum += mean;
            samples += 1;
            if shown % (frames / 8).max(1) == 0 {
                bright.push(mean);
            }
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{}x{}: {} pictures read, {} frames out in {:.2} s = {:.1} fps (real time needs {:.1}); mean luma {:.0}, samples {:?}",
        info.width, info.height, frames.min(info.frame_count), shown, secs, f64::from(shown) / secs,
        f64::from(info.fps_num) / f64::from(info.fps_den), luma_sum as f64 / f64::from(samples.max(1)), bright
    );
}
