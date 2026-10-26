//! A scan of the local shares: every shared directory is found first, then worker tasks read
//! the files of each directory while one writer puts each directory into the index in a
//! transaction of its own, and at the end every row the scan did not see is pruned and the
//! filename index rebuilt.
//!
//! The walk leaves out hidden and system entries, links unless they are followed, directories
//! it may not read, and, with everything under them, excluded directories and directories a
//! filter matches. A file whose size and modification time match its row keeps the attributes
//! read before, unless the scan is forced.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use regex::{Regex, RegexBuilder};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, warn};

use super::attributes;
use super::repository::{self, DirectoryRows, FileRow, Known};
use super::share::{self, CASE_SENSITIVE, Share};
use crate::config::Options;
use crate::db::{Database, DbError};

/// How many directories wait for a worker at most.
const QUEUE: usize = 1000;
/// How many read directories wait for the writer at most.
const WRITES: usize = 64;

/// What decides what a scan puts in the index: the shares, the filters and whether links are
/// followed.
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

/// The largest compiled form a filter may take.
const FILTER_SIZE: usize = 10 << 20;

impl Settings {
  /// The settings of `options` for the share list `shares`. A filter that does not compile was
  /// refused when the options were loaded, so one that fails here is left out.
  pub fn new(shares: Vec<Share>, options: &Options) -> Self {
    let filters = options
      .shares
      .filters
      .iter()
      .filter_map(|f| {
        RegexBuilder::new(f)
          .case_insensitive(!options.flags.case_sensitive_regex)
          .size_limit(FILTER_SIZE)
          .build()
          .ok()
      })
      .collect();
    let mut directories: Vec<&str> = shares.iter().map(|s| s.raw.as_str()).collect();
    directories.sort_unstable();
    directories.dedup();
    let mut patterns: Vec<&str> = options.shares.filters.iter().map(String::as_str).collect();
    patterns.sort_unstable();
    patterns.dedup();
    let snapshot = json!({
      "content": {
        "directories": directories,
        "filters": patterns,
        "followSymlinks": options.shares.follow_symlinks,
      },
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
  fn filtered(&self, path: &Path) -> bool {
    let text = path.to_string_lossy();
    self.filters.iter().any(|f| f.is_match(&text))
  }
}

/// The part of a recorded snapshot that decides what is shared.
pub fn content(snapshot: &str) -> Option<serde_json::Value> {
  serde_json::from_str::<serde_json::Value>(snapshot)
    .ok()
    .and_then(|mut value| value.get_mut("content").map(serde_json::Value::take))
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
  /// it was cancelled; what it wrote stays and nothing was pruned
  Cancelled,
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

/// Whether a directory entry is hidden or the system's own, by its attributes.
#[cfg(windows)]
pub(super) fn hidden(_path: &Path, metadata: Option<&std::fs::Metadata>) -> bool {
  use std::os::windows::fs::MetadataExt;
  const HIDDEN: u32 = 0x2;
  const SYSTEM: u32 = 0x4;
  metadata.is_some_and(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0)
}

/// Whether a directory entry is hidden: its name starts with a dot.
#[cfg(not(windows))]
pub(super) fn hidden(path: &Path, _metadata: Option<&std::fs::Metadata>) -> bool {
  path
    .file_name()
    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

/// The key that tells two directory paths apart by this system's case rule.
fn key(path: &Path) -> PathBuf {
  if CASE_SENSITIVE {
    path.to_path_buf()
  } else {
    PathBuf::from(path.to_string_lossy().to_lowercase())
  }
}

/// The directories to scan under `root`, `root` first, by the walk's rules, and how many were
/// left out because they are excluded or filtered. `root` is always taken when `with_root`.
pub(super) fn directories_under(
  settings: &Settings,
  root: &Path,
  with_root: bool,
  cancel: &CancellationToken,
  seen: &mut HashSet<PathBuf>,
) -> (Vec<PathBuf>, u32) {
  let mut found = Vec::new();
  let mut left_out = 0u32;
  if with_root && seen.insert(key(root)) {
    found.push(root.to_path_buf());
  }
  let mut canonical: HashSet<PathBuf> = HashSet::new();
  if settings.follow_symlinks
    && let Ok(root) = root.canonicalize()
  {
    canonical.insert(root);
  }
  let walk = walkdir::WalkDir::new(root)
    .follow_links(settings.follow_symlinks)
    .into_iter()
    .filter_entry(|entry| {
      if entry.depth() == 0 {
        return true;
      }
      let metadata = entry.metadata().ok();
      if !entry.file_type().is_dir() || hidden(entry.path(), metadata.as_ref()) {
        return false;
      }
      let path = entry.path();
      if share::excluded(&settings.shares, path) {
        debug!(
          directory = %path.display(),
          "Excluding directory because it is a subdirectory of an excluded share"
        );
        left_out += 1;
        return false;
      }
      if settings.filtered(path) {
        debug!(
          directory = %path.display(),
          "Excluding directory because it matches one or more share filters"
        );
        left_out += 1;
        return false;
      }
      // a link followed to a directory already walked would be walked twice, or forever
      if settings.follow_symlinks
        && entry.path_is_symlink()
        && !path.canonicalize().is_ok_and(|real| canonical.insert(real))
      {
        return false;
      }
      true
    });
  for entry in walk {
    if cancel.is_cancelled() {
      break;
    }
    // a directory that can not be read is left out, with everything under it
    let Ok(entry) = entry else { continue };
    if entry.depth() > 0 && seen.insert(key(entry.path())) {
      found.push(entry.into_path());
    }
  }
  (found, left_out)
}

/// Every directory to scan, each share's own first, and how many were left out.
fn directories(settings: &Settings, cancel: &CancellationToken) -> (Vec<PathBuf>, u32) {
  let mut seen = HashSet::new();
  let mut all = Vec::new();
  let mut left_out = 0;
  for share in settings.shares.iter().filter(|s| !s.excluded) {
    if let Err(e) = std::fs::metadata(&share.local_path).and_then(|m| {
      if m.is_dir() {
        Ok(())
      } else {
        Err(std::io::Error::other("not a directory"))
      }
    }) {
      warn!(share = %share.local_path.display(), error = %e, "Failed to scan share");
      continue;
    }
    let (found, out) = directories_under(settings, &share.local_path, true, cancel, &mut seen);
    all.extend(found);
    left_out += out;
  }
  (all, left_out)
}

/// The files directly in a directory, read from disk.
#[derive(Debug)]
pub(super) struct Read {
  /// the directory and its files as the index writes them
  pub rows: DirectoryRows,
  /// files a filter left out
  pub filtered: u32,
  /// files whose name was not UTF-8
  pub lossy: u32,
}

/// Reads the files directly in `directory`, which `share` owns: their size, modification time
/// and attributes, reusing those of `known` rows that did not change unless `force`. One file
/// that can not be read is logged and left out; its siblings are read.
pub(super) fn read_directory(
  settings: &Settings,
  share: &Share,
  directory: &Path,
  known: &HashMap<String, Known>,
  force: bool,
) -> Option<Read> {
  let masked = share::mask(share, directory)?;
  let mut read = Read {
    rows: DirectoryRows {
      name: masked.name,
      files: Vec::new(),
    },
    filtered: 0,
    lossy: u32::from(masked.lossy),
  };
  let entries = match std::fs::read_dir(directory) {
    Ok(entries) => entries,
    Err(e) => {
      warn!(directory = %directory.display(), error = %e, "Failed to scan files in directory");
      return Some(read);
    }
  };
  let mut names = HashSet::new();
  for entry in entries {
    let entry = match entry {
      Ok(entry) => entry,
      Err(e) => {
        warn!(directory = %directory.display(), error = %e, "Failed to scan files in directory");
        continue;
      }
    };
    let path = entry.path();
    let link = entry.file_type().is_ok_and(|t| t.is_symlink());
    if link && !settings.follow_symlinks {
      continue;
    }
    let metadata = match std::fs::metadata(&path) {
      Ok(metadata) => metadata,
      Err(e) => {
        debug!(file = %path.display(), error = %e, "file not read");
        continue;
      }
    };
    if !metadata.is_file() || hidden(&path, Some(&metadata)) {
      continue;
    }
    if settings.filtered(&path) {
      read.filtered += 1;
      continue;
    }
    let Some(file) = share::mask(share, &path) else {
      continue;
    };
    read.lossy += u32::from(file.lossy);
    if !names.insert(file.name.clone()) {
      warn!(
        name = file.name,
        file = %path.display(),
        "two local files share one remote name; the later one is shared"
      );
    }
    let touched_at = metadata
      .modified()
      .ok()
      .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
      .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let name = entry.file_name();
    let extension = attributes::extension(&name.to_string_lossy());
    let size = metadata.len();
    let attributes = match known.get(&file.name) {
      Some(row) if !force && row.size == size && row.touched_at == touched_at => {
        row.attributes.clone()
      }
      _ => attributes::read(&path, &extension),
    };
    read.rows.files.push(FileRow {
      masked: file.name,
      original: repository::path_bytes(&path),
      size,
      touched_at,
      extension,
      attributes,
    });
  }
  Some(read)
}

/// Reads one directory with its known rows from the index, on a blocking thread.
pub(super) async fn read_from_disk(
  db: &Database,
  settings: &Arc<Settings>,
  directory: PathBuf,
  force: bool,
) -> Result<Option<Read>, ScanError> {
  let Some(share) = share::owner(&settings.shares, &directory).cloned() else {
    return Ok(None);
  };
  let Some(masked) = share::mask(&share, &directory) else {
    return Ok(None);
  };
  let known = db
    .read(move |conn| repository::known_files(conn, &masked.name))
    .await?;
  let settings = settings.clone();
  tokio::task::spawn_blocking(move || {
    debug!(directory = %directory.display(), "Starting scan of directory");
    read_directory(&settings, &share, &directory, &known, force)
  })
  .await
  .map_err(|e| ScanError::Task(e.to_string()))
}

/// Runs a scan of `settings`' shares into `db`, reporting progress to `report` after every
/// directory written. Cancelling `cancel` stops it between directories.
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
    current += 1;
    cached = cached.saturating_add(count);
    filtered += read.filtered;
    lossy += read.lossy;
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
    files = cached + filtered,
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
    info!("Shared file scan cancelled successfully");
    return Ok(Outcome::Cancelled);
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
    let shares = share::configure(&options, Path::new("/nowhere"));
    Arc::new(Settings::new(shares, &options))
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
      content(&latest.options).unwrap()["filters"],
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
  async fn a_cancelled_scan_prunes_nothing_and_is_suspect() {
    let dir = tree();
    let db = index().await;
    let settings = settings(dir.path(), &[], &[], false);
    run(&db, settings.clone(), false).await;
    let before = names(&db).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = scan(&db, settings, false, 1, cancel, |_| {}).await.unwrap();
    assert_eq!(outcome, Outcome::Cancelled);
    assert_eq!(names(&db).await, before);
    let latest = db.read(repository::latest_scan).await.unwrap().unwrap();
    assert!(latest.ended_at.is_none() && latest.suspect);
  }
}
