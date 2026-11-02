//! Append-only sample log shared by the collector (status line) and the app.

use crate::model::Sample;
use chrono::{DateTime, Duration, Utc};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SAMPLES: &str = "samples.jsonl";
const LOCK: &str = "samples.lock";
/// A sample whose readings repeat the previous one is skipped unless this much
/// time has passed, so idle polling does not grow the file.
const DEDUPE_SECS: i64 = 10 * 60;

#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Store { dir: dir.into() }
    }

    pub fn open_default() -> Self {
        Store::new(crate::paths::data_dir())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn samples_path(&self) -> PathBuf {
        self.dir.join(SAMPLES)
    }

    fn lock(&self) -> io::Result<File> {
        fs::create_dir_all(&self.dir)?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join(LOCK))?;
        f.lock()?;
        Ok(f) // unlocked when dropped
    }

    /// The last parseable sample, read from the file's tail.
    pub fn last(&self) -> io::Result<Option<Sample>> {
        let mut f = match File::open(self.samples_path()) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let len = f.metadata()?.len();
        let start = len.saturating_sub(16 * 1024);
        f.seek(SeekFrom::Start(start))?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        Ok(buf
            .lines()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .find_map(|l| serde_json::from_str::<Sample>(l).ok()))
    }

    /// Appends `s` unless it repeats the last sample's readings within the dedupe
    /// window. Returns whether a line was written.
    pub fn append(&self, s: &Sample) -> io::Result<bool> {
        let _guard = self.lock()?;
        if let Some(last) = self.last()? {
            let age = (s.ts - last.ts).num_seconds();
            if last.same_readings(s) && (0..DEDUPE_SECS).contains(&age) {
                return Ok(false);
            }
        }
        let mut line = serde_json::to_string(s).map_err(io::Error::other)?;
        line.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.samples_path())?;
        f.write_all(line.as_bytes())?;
        Ok(true)
    }

    /// Every sample at or after `since`, oldest first. Unparseable lines are skipped.
    pub fn load_since(&self, since: DateTime<Utc>) -> io::Result<Vec<Sample>> {
        let f = match File::open(self.samples_path()) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out: Vec<Sample> = BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| serde_json::from_str::<Sample>(&l).ok())
            .filter(|s| s.ts >= since)
            .collect();
        out.sort_by_key(|s| s.ts);
        Ok(out)
    }

    /// Samples from the last `days` days (enough history for one weekly window).
    pub fn load_recent(&self, now: DateTime<Utc>, days: i64) -> io::Result<Vec<Sample>> {
        self.load_since(now - Duration::days(days))
    }

    /// Drops samples older than `before`. Returns how many were removed.
    pub fn prune_before(&self, before: DateTime<Utc>) -> io::Result<usize> {
        let _guard = self.lock()?;
        let path = self.samples_path();
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut kept = String::with_capacity(content.len());
        let mut removed = 0;
        for line in content.lines() {
            match serde_json::from_str::<Sample>(line) {
                Ok(s) if s.ts >= before => {
                    kept.push_str(line);
                    kept.push('\n');
                }
                _ => removed += 1,
            }
        }
        if removed > 0 {
            let tmp = path.with_extension("jsonl.tmp");
            fs::write(&tmp, kept)?;
            fs::rename(&tmp, &path)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Source, Window};
    use chrono::TimeZone;

    fn sample(min: i64, pct: f64) -> Sample {
        let mut s = Sample::new(
            Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap() + Duration::minutes(min),
            Source::Statusline,
        );
        s.seven_day = Some(Window {
            pct,
            resets_at: None,
        });
        s
    }

    #[test]
    fn append_dedupes_and_loads_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert!(store.last().unwrap().is_none());
        assert!(store.append(&sample(0, 10.0)).unwrap());
        assert!(
            !store.append(&sample(5, 10.0)).unwrap(),
            "same readings within 10 min"
        );
        assert!(store.append(&sample(6, 11.0)).unwrap(), "changed readings");
        assert!(
            store.append(&sample(30, 11.0)).unwrap(),
            "heartbeat after 10 min"
        );
        let all = store.load_since(Utc.timestamp_opt(0, 0).unwrap()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(store.last().unwrap().unwrap().ts, all[2].ts);
    }

    #[test]
    fn skips_garbage_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.append(&sample(0, 1.0)).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(store.samples_path())
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();
        store.append(&sample(100, 2.0)).unwrap();
        assert_eq!(
            store
                .load_since(Utc.timestamp_opt(0, 0).unwrap())
                .unwrap()
                .len(),
            2
        );
        let cut = sample(50, 0.0).ts;
        assert_eq!(store.prune_before(cut).unwrap(), 2); // old sample + garbage line
        let left = store.load_since(Utc.timestamp_opt(0, 0).unwrap()).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].seven_day.unwrap().pct, 2.0);
    }
}
