//! A scan of the local shares: every shared directory is found first, then worker tasks read
//! the files of each directory while one writer puts each directory into the index in a
//! transaction of its own, and at the end every row the scan did not see is pruned and the
//! filename index rebuilt. Which directories and files the walk takes is in `walk`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, warn};

use super::repository;
use super::share::Share;
use super::walk::{Read, directories, read_from_disk};
use crate::config::{Options, patterns};
use crate::db::{Database, DbError};

/// How many directories wait for a worker at most.
const QUEUE: usize = 1000;
/// How many read directories wait for the writer at most.
const WRITES: usize = 64;

/// What decides what a scan puts in the index: the shares, the filters, whether they match
/// case and whether links are followed.
#[derive(Clone, Debug)]
pub struct Settings {
  /// the shares, deepest path first
  pub shares: Vec<Share>,
  /// the share filters, compiled
  pub filters: Vec<Regex>,
  /// links inside the shares are followed
  pub follow_symlinks: bool,
  /// what the scan records as its options: the share options, and the part of them that
  /// decides what is shared, which is what a later start compares
  pub snapshot: serde_json::Value,
}

/// The share filters as a set, which is what a scan depends on: their order and repeats do not
/// change what it leaves out.
pub(super) fn filter_set(filters: &[String]) -> BTreeSet<&str> {
  filters.iter().map(String::as_str).collect()
}

impl Settings {
  /// The settings of `options` for the share list `shares`, the filters matching case when
  /// `case_sensitive`.
  pub fn new(shares: Vec<Share>, options: &Options, case_sensitive: bool) -> Self {
    let filters = options
      .shares
      .filters
      .iter()
      .filter_map(|f| {
        patterns::compile(f, case_sensitive)
          .inspect_err(|e| warn!(filter = f, error = e, "share filter not used"))
          .ok()
      })
      .collect();
    let snapshot = json!({
      "content": content_of(&shares, options, case_sensitive),
      "shares": options.shares,
    });
    Self {
      shares,
      filters,
      follow_symlinks: options.shares.follow_symlinks,
      snapshot,
    }
  }

  /// Whether a filter matches the full local path `path`.
  pub(super) fn filtered(&self, path: &Path) -> bool {
    let text = path.to_string_lossy();
    self.filters.iter().any(|f| f.is_match(&text))
  }
}

/// What decides what a scan of `shares` with `options` shares: the entries, the filters as a
/// set, whether they match case and whether links are followed.
pub(super) fn content_of(
  shares: &[Share],
  options: &Options,
  case_sensitive: bool,
) -> serde_json::Value {
  let mut directories: Vec<&str> = shares.iter().map(|s| s.raw.as_str()).collect();
  directories.sort_unstable();
  directories.dedup();
  json!({
    "directories": directories,
    "filters": filter_set(&options.shares.filters),
    "caseSensitive": case_sensitive,
    "followSymlinks": options.shares.follow_symlinks,
  })
}

/// The part of a recorded snapshot that decides what is shared, `None` when it has none; an
/// error when the snapshot is no JSON.
pub fn content(snapshot: &str) -> Result<Option<serde_json::Value>, serde_json::Error> {
  let mut value = serde_json::from_str::<serde_json::Value>(snapshot)?;
  Ok(value.get_mut("content").map(serde_json::Value::take))
}

/// The time now in epoch milliseconds.
pub fn now_ms() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// How far a scan is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
  /// directories finished, out of `directories`
  pub fraction: f64,
  /// directories to scan
  pub directories: u32,
  /// files indexed so far
  pub files: u32,
}

/// How a scan ended, when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
  /// every directory was scanned and the index now holds these totals
  Completed {
    /// directories in the index
    directories: u32,
    /// files in the index
    files: u32,
  },
  /// it was canceled; what it wrote stays and nothing was pruned
  Canceled,
}

/// Why a scan failed.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
  /// the index refused a write or a read
  #[error(transparent)]
  Db(#[from] DbError),
  /// a worker or the walk stopped without finishing
  #[error("a scan task did not finish: {0}")]
  Task(String),
}

/// Runs a scan of `settings`' shares into `db`, reporting progress to `report` after every
/// directory written. Canceling `cancel` stops it between directories.
pub async fn scan(
  db: &Database,
  settings: Arc<Settings>,
  force: bool,
  workers: u32,
  cancel: CancellationToken,
  report: impl Fn(Progress) + Send + Sync + 'static,
) -> Result<Outcome, ScanError> {
  let started = Instant::now();
  info!("Starting shared file scan");
  let timestamp = now_ms();
  let options = settings.snapshot.to_string();
  db.write(move |conn| repository::insert_scan(conn, timestamp, &options))
    .await?;
  if settings.shares.is_empty() {
    warn!("Aborting shared file scan; no shares configured.");
  }
  for share in &settings.shares {
    if share.excluded {
      info!(path = %share.local_path.display(), "Excluding");
    } else {
      info!(path = %share.local_path.display(), remote = share.alias, "Sharing");
    }
  }
  info!("Enumerating shared directories");
  let walk = {
    let settings = settings.clone();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || directories(&settings, &cancel))
      .await
      .map_err(|e| ScanError::Task(e.to_string()))?
  };
  let (dirs, left_out) = walk;
  let total = u32::try_from(dirs.len()).unwrap_or(u32::MAX);
  info!(
    directories = total,
    excluded = left_out,
    ms = started.elapsed().as_millis() as u64,
    "Found shared directories.  Starting file scan."
  );
  report(Progress {
    fraction: 0.0,
    directories: total,
    files: 0,
  });

  let files_started = Instant::now();
  let (queue, receiver) = mpsc::channel::<PathBuf>(QUEUE);
  let receiver = Arc::new(Mutex::new(receiver));
  let (writes, mut written) = mpsc::channel::<Read>(WRITES);
  let mut tasks = tokio::task::JoinSet::<Result<(), ScanError>>::new();
  for _ in 0..workers.max(1) {
    let receiver = receiver.clone();
    let writes = writes.clone();
    let settings = settings.clone();
    let db = db.clone();
    let cancel = cancel.clone();
    tasks.spawn(
      async move {
        loop {
          let next = tokio::select! {
            () = cancel.cancelled() => None,
            next = async { receiver.lock().await.recv().await } => next,
          };
          let Some(directory) = next else {
            return Ok(());
          };
          if let Some(read) = read_from_disk(&db, &settings, directory, force).await?
            && writes.send(read).await.is_err()
          {
            return Ok(());
          }
        }
      }
      .in_current_span(),
    );
  }
  drop(writes);
  let producer = {
    let cancel = cancel.clone();
    tokio::spawn(
      async move {
        for directory in dirs {
          if cancel.is_cancelled() {
            info!("Shared file scan cancellation requested");
            return;
          }
          if queue.send(directory).await.is_err() {
            return;
          }
        }
      }
      .in_current_span(),
    )
  };
  let (mut current, mut cached, mut filtered, mut lossy) = (0u32, 0u32, 0u32, 0u32);
  while let Some(read) = written.recv().await {
    let count = u32::try_from(read.rows.files.len()).unwrap_or(u32::MAX);
    let rows = read.rows;
    let name = rows.name.clone();
    db.write(move |conn| {
      let tx = conn.transaction()?;
      repository::write_directory(&tx, timestamp, &rows)?;
      tx.commit()
    })
    .await?;
    current = current.saturating_add(1);
    cached = cached.saturating_add(count);
    filtered = filtered.saturating_add(read.filtered);
    lossy = lossy.saturating_add(read.lossy);
    debug!(
      directory = name,
      added = count,
      filtered = read.filtered,
      "Finished scanning directory"
    );
    report(Progress {
      fraction: if total == 0 {
        1.0
      } else {
        f64::from(current) / f64::from(total)
      },
      directories: total,
      files: cached,
    });
  }
  producer.await.map_err(|e| ScanError::Task(e.to_string()))?;
  while let Some(done) = tasks.join_next().await {
    done.map_err(|e| ScanError::Task(e.to_string()))??;
  }
  info!(
    files = cached.saturating_add(filtered),
    cached,
    filtered,
    ms = files_started.elapsed().as_millis() as u64,
    "Scan found files"
  );
  if lossy > 0 {
    warn!(
      files = lossy,
      "shared names that are not UTF-8 are shown with U+FFFD in their place"
    );
  }
  if cancel.is_cancelled() {
    db.write(move |conn| repository::mark_suspect(conn, timestamp))
      .await?;
    info!("Shared file scan canceled successfully");
    return Ok(Outcome::Canceled);
  }
  let pruning = Instant::now();
  info!("Rebuilding filename index...");
  let pruned = db
    .write(move |conn| repository::finish_scan(conn, timestamp))
    .await?;
  info!(
    files = pruned.files,
    directories = pruned.directories,
    ms = pruning.elapsed().as_millis() as u64,
    "Removed or renamed files and directories"
  );
  info!("Filename index rebuild complete");
  let (directories, files) = db
    .write(move |conn| {
      repository::maintain(conn);
      repository::end_scan(conn, timestamp, now_ms())?;
      repository::count(conn, None)
    })
    .await?;
  info!(
    ms = started.elapsed().as_millis() as u64,
    directories, files, "Shared file cache created or updated"
  );
  Ok(Outcome::Completed { directories, files })
}

#[cfg(test)]
pub(super) mod tests {
  use super::*;
  use crate::shares::repository::tests::index;
  use crate::shares::share;

  /// A share tree in a temporary directory: `music` with an album, a hidden directory, a
  /// hidden file, a filtered file and a private subdirectory.
  pub fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("music");
    for sub in [
      "Artist/Album",
      "Artist/Album/Scans",
      ".hidden",
      "private",
      "Thumbs",
    ] {
      std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    for file in [
      "Artist/Album/01.mp3",
      "Artist/Album/02.mp3",
      "Artist/Album/desktop.ini",
      "Artist/Album/.secret.mp3",
      "Artist/Album/Scans/front.jpg",
      ".hidden/x.mp3",
      "private/p.mp3",
      "Thumbs/t.mp3",
      "top.flac",
    ] {
      std::fs::write(root.join(file), file).unwrap();
    }
    dir
  }

  pub fn settings(dir: &Path, extra: &[&str], filters: &[&str], follow: bool) -> Arc<Settings> {
    let mut options = Options::default();
    options.shares.directories = vec![format!("[Music]{}", dir.join("music").display())];
    options.shares.directories.extend(
      extra
        .iter()
        .map(|e| e.replace("{}", &dir.display().to_string())),
    );
    options.shares.filters = filters.iter().map(|f| f.to_string()).collect();
    options.shares.follow_symlinks = follow;
    let shares = share::configure(&options.shares, Path::new("/nowhere"));
    Arc::new(Settings::new(shares, &options, false))
  }

  pub async fn names(db: &Database) -> Vec<String> {
    db.read(|conn| {
      let mut s = conn.prepare("SELECT maskedFilename FROM files ORDER BY maskedFilename")?;
      s.query_map([], |r| r.get(0))?.collect()
    })
    .await
    .unwrap()
  }

  async fn run(db: &Database, settings: Arc<Settings>, force: bool) -> Outcome {
    scan(db, settings, force, 2, CancellationToken::new(), |_| {})
      .await
      .unwrap()
  }

  #[tokio::test]
  async fn a_scan_indexes_what_the_rules_let_through() {
    let dir = tree();
    let db = index().await;
    let settings = settings(
      dir.path(),
      &["!{}/music/private"],
      &[r"\.ini$", "/Thumbs$"],
      false,
    );
    let outcome = run(&db, settings, false).await;
    assert_eq!(
      outcome,
      Outcome::Completed {
        directories: 4,
        files: 4
      }
    );
    assert_eq!(
      names(&db).await,
      [
        "Music\\Artist\\Album\\01.mp3",
        "Music\\Artist\\Album\\02.mp3",
        "Music\\Artist\\Album\\Scans\\front.jpg",
        "Music\\top.flac",
      ]
    );
    let latest = db.read(repository::latest_scan).await.unwrap().unwrap();
    assert!(latest.ended_at.is_some() && !latest.suspect);
    assert_eq!(
      content(&latest.options).unwrap().unwrap()["filters"],
      json!(["/Thumbs$", r"\.ini$"])
    );
  }

  #[tokio::test]
  async fn a_rescan_prunes_what_went_and_reuses_what_did_not_change() {
    let dir = tree();
    let db = index().await;
    let settings = settings(dir.path(), &[], &[], false);
    run(&db, settings.clone(), false).await;
    let album = dir.path().join("music/Artist/Album");
    std::fs::remove_file(album.join("02.mp3")).unwrap();
    // a reused row keeps the attributes it had, so write ones a probe could never produce
    db.write(|conn| {
      conn.execute(
        "UPDATE files SET attributeJson = '[{\"type\":\"BitRate\",\"value\":999}]' \
         WHERE maskedFilename LIKE '%01.mp3'",
        [],
      )
    })
    .await
    .unwrap();
    run(&db, settings.clone(), false).await;
    let names = names(&db).await;
    assert!(!names.iter().any(|n| n.ends_with("02.mp3")), "{names:?}");
    let reused = |db: Database| async move {
      db.read(|conn| {
        conn.query_row(
          "SELECT attributeJson FROM files WHERE maskedFilename LIKE '%01.mp3'",
          [],
          |r| r.get::<_, String>(0),
        )
      })
      .await
      .unwrap()
    };
    assert!(
      reused(db.clone()).await.contains("999"),
      "unchanged, so not read again"
    );
    run(&db, settings, true).await;
    assert_eq!(
      reused(db.clone()).await,
      "[]",
      "a forced scan reads every file"
    );
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn links_are_followed_only_when_asked_and_never_in_circles() {
    let dir = tree();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("o.mp3"), "o").unwrap();
    let music = dir.path().join("music");
    std::os::unix::fs::symlink(&outside, music.join("linked")).unwrap();
    std::os::unix::fs::symlink(&music, music.join("Artist/loop")).unwrap();
    std::os::unix::fs::symlink(outside.join("o.mp3"), music.join("file-link.mp3")).unwrap();
    let db = index().await;
    run(&db, settings(dir.path(), &[], &[], false), false).await;
    let plain = names(&db).await;
    assert!(
      !plain
        .iter()
        .any(|n| n.contains("linked") || n.contains("link.mp3")),
      "{plain:?}"
    );
    run(&db, settings(dir.path(), &[], &[], true), false).await;
    let followed = names(&db).await;
    assert!(
      followed.contains(&"Music\\linked\\o.mp3".to_string()),
      "{followed:?}"
    );
    assert!(followed.contains(&"Music\\file-link.mp3".to_string()));
    assert!(!followed.iter().any(|n| n.contains("loop")), "{followed:?}");
  }

  #[tokio::test]
  async fn a_canceled_scan_prunes_nothing_and_is_suspect() {
    let dir = tree();
    let db = index().await;
    let settings = settings(dir.path(), &[], &[], false);
    run(&db, settings.clone(), false).await;
    let before = names(&db).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = scan(&db, settings, false, 1, cancel, |_| {}).await.unwrap();
    assert_eq!(outcome, Outcome::Canceled);
    assert_eq!(names(&db).await, before);
    let latest = db.read(repository::latest_scan).await.unwrap().unwrap();
    assert!(latest.ended_at.is_none() && latest.suspect);
  }
}
