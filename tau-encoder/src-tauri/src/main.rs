#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn temporary_file() -> Result<(Temporary, File), String> {
    for _ in 0..100 {
        let name = format!(
            "tau-encoder-{}-{}.tvc",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((Temporary(path), file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not create temporary video: {e}")),
        }
    }
    Err("Could not reserve a temporary video file.".into())
}
#[derive(Serialize)]
struct ResultInfo {
    frames: u32,
    level: u8,
    reencoded: u32,
    bytes: u64,
}
fn encode_file(input: &Path, output: &Path, qp: u8) -> Result<ResultInfo, String> {
    if qp > 51 {
        return Err("Quality value must be between 0 and 51.".into());
    }
    let source = File::open(input).map_err(|e| format!("Could not open input: {e}"))?;
    let (_temp, mut intermediate) = temporary_file()?;
    let stats = tau_video::tvc::encoder::encode_y4m(BufReader::new(source), &mut intermediate, qp)
        .map_err(|e| format!("Video encoding failed: {e}"))?;
    intermediate
        .seek(SeekFrom::Start(0))
        .map_err(|e| format!("Could not read temporary video: {e}"))?;
    let mut dest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| {
            format!("Could not create output (existing files are not overwritten): {e}")
        })?;
    let result = tau_video::container::mux_tvc(&mut intermediate, &mut dest)
        .map_err(|e| format!("Could not create TNV3: {e}"))
        .and_then(|_| {
            dest.metadata()
                .map(|m| m.len())
                .map_err(|e| format!("Could not inspect output: {e}"))
        });
    drop(dest);
    match result {
        Ok(bytes) => Ok(ResultInfo {
            frames: stats.frames,
            level: stats.level,
            reencoded: stats.reencoded,
            bytes,
        }),
        Err(error) => match std::fs::remove_file(output) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!(
                "{error} Could not remove incomplete output: {cleanup}"
            )),
        },
    }
}
#[tauri::command]
async fn encode(
    input: String,
    output: String,
    qp: u8,
    busy: tauri::State<'_, Mutex<bool>>,
) -> Result<ResultInfo, String> {
    {
        let mut running = busy.lock().map_err(|_| "Encoder state unavailable.")?;
        if *running {
            return Err("An encode is already running.".into());
        }
        *running = true;
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        encode_file(Path::new(&input), Path::new(&output), qp)
    })
    .await;
    *busy.lock().map_err(|_| "Encoder state unavailable.")? = false;
    result.map_err(|e| format!("Encoder task stopped: {e}"))?
}
fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(false))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![encode])
        .run(tauri::generate_context!())
        .expect("Failed to start Tau Encoder");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn encode_creates_tnv3_and_preserves_existing_output() {
        let (input_guard, mut input) = temporary_file().unwrap();
        write!(input, "YUV4MPEG2 W16 H16 F24:1 Ip C420jpeg\nFRAME\n").unwrap();
        input.write_all(&[128; 384]).unwrap();
        drop(input);
        let (output_guard, existing) = temporary_file().unwrap();
        drop(existing);
        assert!(encode_file(&input_guard.0, &output_guard.0, 30).is_err());
        assert_eq!(std::fs::metadata(&output_guard.0).unwrap().len(), 0);
        std::fs::remove_file(&output_guard.0).unwrap();
        let result = encode_file(&input_guard.0, &output_guard.0, 30).unwrap();
        assert_eq!(result.frames, 1);
        let reader = tau_video::Tnv3Reader::open(File::open(&output_guard.0).unwrap()).unwrap();
        assert_eq!(reader.info().frame_count, 1);
    }
}
