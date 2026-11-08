//! Detailed per-encode log for later analysis: one JSON object per line (`.jsonl`), each with the milliseconds
//! since the encode started, an event name and its data. Written by the app unless the setting is off.
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub struct EncodeLog {
    file: Mutex<File>,
    started: Instant,
    path: PathBuf,
    last_progress: Mutex<(String, Option<Instant>)>,
    phase_starts: Mutex<Vec<(String, u64)>>,
}

/// UTC date and time from Unix seconds: ("2026-11-08T21:54:03Z", "20261108-215403").
pub fn utc_stamp(secs: u64) -> (String, String) {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    (
        format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z"),
        format!("{year:04}{month:02}{day:02}-{h:02}{m:02}{s:02}"),
    )
}

impl EncodeLog {
    /// Creates `encode-<UTC time>-<name>.jsonl` in `dir` (made if missing).
    pub fn create(dir: &Path, name: &str) -> Result<Arc<Self>, String> {
        fs::create_dir_all(dir).map_err(|e| format!("Could not create the log folder: {e}"))?;
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let (_, compact) = utc_stamp(secs);
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
        let mut path = dir.join(format!("encode-{compact}-{safe}.jsonl"));
        let mut n = 1;
        while path.exists() {
            path = dir.join(format!("encode-{compact}-{safe}-{n}.jsonl"));
            n += 1;
        }
        let file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("Could not create the log: {e}"))?;
        Ok(Arc::new(EncodeLog { file: Mutex::new(file), started: Instant::now(), path, last_progress: Mutex::new((String::new(), None)), phase_starts: Mutex::new(Vec::new()) }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one event. Logging never fails the encode: write errors are ignored.
    pub fn event(&self, kind: &str, data: Value) {
        let line = json!({"t_ms": self.started.elapsed().as_millis() as u64, "event": kind, "data": data}).to_string();
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Logs a progress event when the phase changes or at least a second has passed since the last one.
    pub fn progress(&self, phase: &str, frames: u32, total: Option<u32>, remaining_seconds: Option<u64>) {
        let now = Instant::now();
        let log = {
            let Ok(mut last) = self.last_progress.lock() else { return };
            let due = last.0 != phase || last.1.is_none_or(|t| now.duration_since(t).as_millis() >= 1000);
            if last.0 != phase {
                if let Ok(mut starts) = self.phase_starts.lock() {
                    starts.push((phase.to_owned(), self.started.elapsed().as_millis() as u64));
                }
            }
            if due {
                *last = (phase.to_owned(), Some(now));
            }
            due
        };
        if log {
            self.event("progress", json!({"phase": phase, "frames": frames, "total_frames_estimate": total, "remaining_seconds": remaining_seconds}));
        }
    }
}

impl EncodeLog {
    /// Seconds spent in each phase, from the first progress event of a phase to the first of the next (the last
    /// one runs to `end_ms`): `[{"phase": "video", "seconds": 71.2}, ...]`.
    pub fn phase_summary(&self, end_ms: u64) -> Value {
        let starts = self.phase_starts.lock().map(|s| s.clone()).unwrap_or_default();
        let items: Vec<Value> = starts
            .iter()
            .enumerate()
            .map(|(i, (name, start))| {
                let stop = starts.get(i + 1).map_or(end_ms, |n| n.1);
                json!({"phase": name, "seconds": (stop.saturating_sub(*start)) as f64 / 1000.0})
            })
            .collect();
        Value::Array(items)
    }

    /// Milliseconds since the encode started.
    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamp_matches_known_dates() {
        assert_eq!(utc_stamp(0).0, "1970-01-01T00:00:00Z");
        assert_eq!(utc_stamp(1_000_000_000).0, "2001-09-09T01:46:40Z");
        assert_eq!(utc_stamp(1_794_177_243).1, "20261108-223403");
        assert_eq!(utc_stamp(951_782_400).0, "2000-02-29T00:00:00Z", "leap day");
        assert_eq!(utc_stamp(4_107_542_399).0, "2100-02-28T23:59:59Z", "2100 is not a leap year");
    }

    #[test]
    fn log_lines_are_json_with_increasing_time_and_progress_is_throttled() {
        let dir = std::env::temp_dir().join(format!("tau-encode-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let log = EncodeLog::create(&dir, "my film: one/two").unwrap();
        assert!(log.path().file_name().unwrap().to_string_lossy().ends_with("-my_film__one_two.jsonl"));
        log.event("start", json!({"input": "a.mkv"}));
        log.progress("video", 1, Some(100), None);
        log.progress("video", 2, Some(100), None); // within a second, same phase: not logged
        log.progress("audio", 100, Some(100), None); // phase change: logged
        std::thread::sleep(std::time::Duration::from_millis(1100));
        log.progress("audio", 100, Some(100), None); // a second later: logged
        let second = EncodeLog::create(&dir, "my film: one/two").unwrap();
        assert_ne!(second.path(), log.path(), "the same name in the same second gets a new file");
        let text = fs::read_to_string(log.path()).unwrap();
        let events: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(events.iter().map(|e| e["event"].as_str().unwrap()).collect::<Vec<_>>(), ["start", "progress", "progress", "progress"]);
        assert_eq!(events[1]["data"]["frames"], 1);
        assert!(events.windows(2).all(|w| w[0]["t_ms"].as_u64() <= w[1]["t_ms"].as_u64()));
        assert!(events[3]["t_ms"].as_u64().unwrap() >= 1000);
        let phases = log.phase_summary(log.elapsed_ms());
        let names: Vec<&str> = phases.as_array().unwrap().iter().map(|p| p["phase"].as_str().unwrap()).collect();
        assert_eq!(names, ["video", "audio"]);
        assert!(phases[1]["seconds"].as_f64().unwrap() >= 1.0, "{phases}");
        fs::remove_dir_all(dir).unwrap();
    }
}
