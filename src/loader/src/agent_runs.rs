use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use serde::{Deserialize, Serialize};

pub const POLL_EVERY: Duration = Duration::from_secs(1);
const KEEP: Duration = Duration::from_secs(24 * 3600);

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Start {
        pid: u32,
        title: String,
    },
    Line {
        text: String,
    },
    End {
        error: Option<String>,
        stopped: bool,
    },
}

fn dir() -> Option<PathBuf> {
    crate::paths::project_dirs().map(|dirs| dirs.data_dir().join("agent-runs"))
}

pub struct Record {
    file: Option<fs::File>,
    pub path: Option<PathBuf>,
}

impl Record {
    pub fn create(run: u64, pid: u32, title: &str) -> Self {
        let name = format!(
            "{}-{}-{run}.jsonl",
            crate::paths::unix_secs(),
            std::process::id()
        );
        let opened = dir()
            .ok_or_else(|| "no data folder".to_owned())
            .and_then(|dir| {
                fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let path = dir.join(name);
                fs::OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .map(|file| (file, path))
                    .map_err(|e| e.to_string())
            });
        let (file, path) = opened
            .map_err(|error| eprintln!("Couldn't record the run for the loader window: {error}"))
            .ok()
            .unzip();
        let mut record = Self { file, path };
        record.write(&Entry::Start {
            pid,
            title: title.to_owned(),
        });
        record
    }

    pub fn write(&mut self, entry: &Entry) {
        let Some(file) = &mut self.file else {
            return;
        };
        let mut line = serde_json::to_string(entry).unwrap_or_default();
        line.push('\n');
        if let Err(error) = file.write_all(line.as_bytes()) {
            eprintln!("Couldn't record the run for the loader window: {error}");
            self.file = None;
        }
    }
}

pub struct Seen {
    pub file: PathBuf,
    pub entry: Entry,
}

pub struct Watcher {
    opened: SystemTime,
    offsets: HashMap<PathBuf, u64>,
    next: Instant,
}

impl Default for Watcher {
    fn default() -> Self {
        Self {
            opened: SystemTime::now(),
            offsets: HashMap::new(),
            next: Instant::now(),
        }
    }
}

impl Watcher {
    pub fn poll(&mut self, now: Instant) -> Option<Vec<Seen>> {
        if now < self.next {
            return None;
        }
        self.next = now + POLL_EVERY;
        Some(dir().map_or_else(Vec::new, |dir| self.scan(&dir)))
    }

    pub fn rewind(&mut self, file: &Path) {
        self.offsets.remove(file);
    }

    fn scan(&mut self, dir: &Path) -> Vec<Seen> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut seen = Vec::new();
        for path in entries.flatten().map(|entry| entry.path()) {
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
            if modified
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age > KEEP)
            {
                let _ = fs::remove_file(&path);
                self.offsets.remove(&path);
                continue;
            }
            let first = self.offsets.get(&path).is_none_or(|offset| *offset == 0);
            let entries = self.read_new(&path);
            let before_window = modified.is_some_and(|at| at < self.opened);
            let ended = entries.iter().any(|e| matches!(e, Entry::End { .. }));
            if first && before_window && ended {
                continue;
            }
            seen.extend(entries.into_iter().map(|entry| Seen {
                file: path.clone(),
                entry,
            }));
        }
        seen
    }

    fn read_new(&mut self, path: &PathBuf) -> Vec<Entry> {
        let offset = self.offsets.get(path).copied().unwrap_or(0);
        let mut text = String::new();
        let read = fs::File::open(path).and_then(|mut file| {
            file.seek(SeekFrom::Start(offset))?;
            file.read_to_string(&mut text)
        });
        if read.is_err() {
            return Vec::new();
        }
        let Some(end) = text.rfind('\n') else {
            self.offsets.entry(path.clone()).or_insert(offset);
            return Vec::new();
        };
        self.offsets.insert(path.clone(), offset + end as u64 + 1);
        text[..end]
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(entry: &Entry) -> String {
        format!("{}\n", serde_json::to_string(entry).unwrap())
    }

    fn kinds(seen: &[Seen]) -> Vec<&'static str> {
        seen.iter()
            .map(|s| match s.entry {
                Entry::Start { .. } => "start",
                Entry::Line { .. } => "line",
                Entry::End { .. } => "end",
            })
            .collect()
    }

    #[test]
    fn a_record_is_read_line_by_line_and_old_ended_runs_are_skipped() {
        let dir = std::env::temp_dir().join(format!("agent-runs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("1-1-1.jsonl");
        fs::write(
            &old,
            line(&Entry::Start {
                pid: 7,
                title: "old".into(),
            }) + &line(&Entry::End {
                error: None,
                stopped: false,
            }),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let mut watcher = Watcher::default();
        std::thread::sleep(Duration::from_millis(20));
        let live = dir.join("2-1-2.jsonl");
        let start = line(&Entry::Start {
            pid: 7,
            title: "live".into(),
        });
        let first = line(&Entry::Line {
            text: "hello".into(),
        });
        fs::write(&live, format!("{start}{}", &first[..5])).unwrap();
        assert_eq!(kinds(&watcher.scan(&dir)), ["start"]);
        let mut file = fs::OpenOptions::new().append(true).open(&live).unwrap();
        file.write_all(&first.as_bytes()[5..]).unwrap();
        file.write_all(
            line(&Entry::End {
                error: Some("boom".into()),
                stopped: false,
            })
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(kinds(&watcher.scan(&dir)), ["line", "end"]);
        assert!(watcher.scan(&dir).is_empty());
        watcher.rewind(&live);
        assert_eq!(kinds(&watcher.scan(&dir)), ["start", "line", "end"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
