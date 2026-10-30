//! The daily log files: `reseek-YYYYMMDD.log` in the log directory, a new one at local
//! midnight, and those older than the retention deleted when a file is started.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{Local, NaiveDate};

/// The start of every log file's name.
const PREFIX: &str = "reseek-";
/// The end of every log file's name.
const SUFFIX: &str = ".log";

/// Writes to the day's log file, starting a new one when the local date changes.
pub struct DailyFile {
  dir: PathBuf,
  retention: Duration,
  current: Option<(NaiveDate, File)>,
}

impl DailyFile {
  /// Log files in `dir`, kept for `retention`. Old files are deleted now and at every new
  /// file.
  pub fn new(dir: PathBuf, retention: Duration) -> Self {
    prune(&dir, retention, SystemTime::now());
    Self {
      dir,
      retention,
      current: None,
    }
  }

  fn file(&mut self) -> io::Result<&mut File> {
    let today = Local::now().date_naive();
    if self.current.as_ref().map(|(date, _)| *date) != Some(today) {
      if self.current.is_some() {
        prune(&self.dir, self.retention, SystemTime::now());
      }
      let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(self.dir.join(file_name(today)))?;
      self.current = Some((today, file));
    }
    let (_, file) = self.current.as_mut().expect("opened above");
    Ok(file)
  }
}

impl Write for DailyFile {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.file()?.write(buf)
  }

  fn flush(&mut self) -> io::Result<()> {
    match &mut self.current {
      Some((_, file)) => file.flush(),
      None => Ok(()),
    }
  }
}

/// The name of the log file of `date`.
pub fn file_name(date: NaiveDate) -> String {
  format!("{PREFIX}{}{SUFFIX}", date.format("%Y%m%d"))
}

/// Deletes the log files in `dir` last written more than `retention` before `now`. A file that
/// cannot be read or deleted is left; this runs inside the log writer, which has nowhere to
/// report it.
fn prune(dir: &Path, retention: Duration, now: SystemTime) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if !(name.starts_with(PREFIX) && name.ends_with(SUFFIX)) {
      continue;
    }
    let old = entry
      .metadata()
      .and_then(|m| m.modified())
      .is_ok_and(|modified| {
        now
          .duration_since(modified)
          .is_ok_and(|age| age > retention)
      });
    if old {
      // see above: a file that stays is deleted on a later pass
      drop(std::fs::remove_file(entry.path()));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn files_are_named_by_day_and_old_ones_pruned() {
    assert_eq!(
      file_name(NaiveDate::from_ymd_opt(2026, 1, 9).unwrap()),
      "reseek-20260109.log"
    );
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("reseek-20200101.log");
    let other = dir.path().join("notes.txt");
    std::fs::write(&old, "x").unwrap();
    std::fs::write(&other, "x").unwrap();
    let later = SystemTime::now() + Duration::from_secs(3 * 24 * 3600);
    prune(dir.path(), Duration::from_secs(2 * 24 * 3600), later);
    assert!(!old.exists());
    assert!(other.exists(), "only log files are pruned");
  }
}
