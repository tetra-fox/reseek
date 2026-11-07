//! Creating the file a download writes, and moving a finished one into its directory. Paths
//! are the daemon's own, built from names already made safe; nothing here checks a root.
//!
//! A move never replaces a file unless told to: the file takes the first free name of
//! `name`, `name (1)`, `name (2)` and so on, claimed by a rename that fails when the name is
//! taken, so two downloads finishing into one directory at once do not overwrite each other.
//! Across filesystems the file is copied to a hidden name beside its destination, random for
//! each copy so two files of one name copied at once never share it, flushed to disk and
//! renamed into place, so a half-copied file never has the final name.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use crate::safety::with_suffix;

#[cfg(unix)]
use super::mode::with_execute_for_each_read;

/// How many numbered names a move tries before it gives up.
const NUMBERED_NAMES: u32 = 10_000;

/// How many hex digits of random a copy's hidden name has.
const PART_DIGITS: usize = 16;

/// The end of a copy's hidden name.
const PART_SUFFIX: &str = ".part";

/// `error` with `context` in front of its message, its kind kept.
fn context(error: io::Error, context: impl std::fmt::Display) -> io::Error {
  io::Error::new(error.kind(), format!("{context}: {error}"))
}

/// The last component of `path`, for messages.
fn base(path: &Path) -> String {
  path
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_default()
}

/// Makes `dir` and the directories above it that are missing. With `mode`, each one made gets
/// it with an execute bit for every read bit, set after creation so the umask does not narrow
/// it.
pub fn create_directories(dir: &Path, mode: Option<u32>) -> io::Result<()> {
  let missing: Vec<&Path> = dir.ancestors().take_while(|d| !d.exists()).collect();
  std::fs::create_dir_all(dir)?;
  #[cfg(unix)]
  if let Some(mode) = mode {
    use std::os::unix::fs::PermissionsExt;
    let mode = with_execute_for_each_read(mode);
    for made in missing {
      std::fs::set_permissions(made, std::fs::Permissions::from_mode(mode))?;
    }
  }
  // other systems have no permission modes
  #[cfg(not(unix))]
  let _ = (missing, mode);
  Ok(())
}

/// Opens `path` for a download to write from `offset`. At 0 the missing directories above it
/// are made with `mode` and an execute bit for every read bit, and the file is created with
/// `mode` when it is new. Past 0 the file must exist and hold at least `offset` bytes, since
/// they are the ones kept. The file is cut at `offset` so what follows is written after them.
pub fn create_file(path: &Path, mode: Option<u32>, offset: u64) -> io::Result<File> {
  if offset > 0 {
    let opened = (|| {
      let file = OpenOptions::new().read(true).write(true).open(path)?;
      let length = file.metadata()?.len();
      if length < offset {
        return Err(io::Error::new(
          ErrorKind::UnexpectedEof,
          format!("it holds {length} bytes, fewer than the {offset} resumed from"),
        ));
      }
      file.set_len(offset)?;
      Ok(file)
    })();
    return opened.map_err(|e| context(e, format!("Failed to open partial file {}", base(path))));
  }
  let created = (|| {
    if let Some(dir) = path.parent() {
      create_directories(dir, mode)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    if let Some(mode) = mode {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(mode);
    }
    let file = options.open(path)?;
    file.set_len(0)?;
    Ok(file)
  })();
  created.map_err(|e| context(e, format!("Failed to create file {}", base(path))))
}

/// `name` numbered `n`: `name` itself for 0, else `stem (n).extension`, the stem cut so the
/// name stays within the length a file name may have.
fn numbered(name: &str, n: u32) -> String {
  if n == 0 {
    return name.to_string();
  }
  with_suffix(name, &format!(" ({n})"))
}

/// Renames `from` to `to`, failing with `AlreadyExists` when `to` exists.
#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
  use rustix::fs::{CWD, RenameFlags, renameat_with};
  use rustix::io::Errno;
  match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
    Ok(()) => Ok(()),
    // a filesystem that cannot rename without replacing; a new link refuses a taken name too
    Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => link_new(from, to),
    Err(e) => Err(e.into()),
  }
}

/// Renames `from` to `to`, failing with `AlreadyExists` when `to` exists.
#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
  link_new(from, to)
}

/// Gives `from` the name `to` by a new link, which fails when `to` exists, then drops `from`.
fn link_new(from: &Path, to: &Path) -> io::Result<()> {
  std::fs::hard_link(from, to)?;
  std::fs::remove_file(from)
}

/// Gives the file at `from` the first free numbered name of `name` in `dir`.
fn place_new(from: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
  for n in 0..NUMBERED_NAMES {
    let to = dir.join(numbered(name, n));
    match rename_new(from, &to) {
      Ok(()) => return Ok(to),
      Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
      Err(e) => return Err(e),
    }
  }
  Err(io::Error::new(
    ErrorKind::AlreadyExists,
    format!("{NUMBERED_NAMES} files named like {name} exist already"),
  ))
}

/// The hidden name `.{random}.part` a copy across filesystems is written to.
fn part_name(random: u64) -> String {
  format!(".{random:0PART_DIGITS$x}{PART_SUFFIX}")
}

/// Whether `name` is one [`part_name`] gives, so a copy cut off before its rename can be found.
pub(super) fn is_part(name: &OsStr) -> bool {
  name
    .to_str()
    .and_then(|name| name.strip_prefix('.')?.strip_suffix(PART_SUFFIX))
    .is_some_and(|random| {
      random.len() == PART_DIGITS
        && random
          .bytes()
          .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// Copies `source` to a hidden name of its own in `dir`, from [`part_name`], flushed to disk,
/// for a move across filesystems; the copy is deleted when it fails.
fn copy_beside(source: &Path, dir: &Path) -> io::Result<PathBuf> {
  let part = dir.join(part_name(rand::random()));
  let copied = (|| {
    let mut from = File::open(source)?;
    let mut to = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&part)?;
    io::copy(&mut from, &mut to)?;
    to.sync_all()
  })();
  if let Err(e) = copied {
    if let Err(removed) = std::fs::remove_file(&part)
      && removed.kind() != ErrorKind::NotFound
    {
      debug!(path = %part.display(), error = %removed, "unfinished copy not removed");
    }
    return Err(e);
  }
  Ok(part)
}

/// Moves `source` into `dir` as `name`, the missing directories made with `mode` and an
/// execute bit for every read bit, and gives the moved file `mode`. With `overwrite` a file of
/// that name is replaced; without, the file takes the first free numbered name. The source's
/// directory is deleted when the move leaves it empty. Returns where the file is now.
pub fn move_file(
  source: &Path,
  dir: &Path,
  name: &str,
  mode: Option<u32>,
  overwrite: bool,
) -> io::Result<PathBuf> {
  move_inner(source, dir, name, mode, overwrite)
    .map_err(|e| context(e, format!("Failed to move file {name}")))
}

fn move_inner(
  source: &Path,
  dir: &Path,
  name: &str,
  mode: Option<u32>,
  overwrite: bool,
) -> io::Result<PathBuf> {
  if !source.is_file() {
    return Err(io::Error::new(
      ErrorKind::NotFound,
      "The specified source file does not exist",
    ));
  }
  // flushed before the rename so a crash soon after never leaves a short file under the final
  // name; Windows flushes only a handle opened for writing
  OpenOptions::new()
    .read(true)
    .write(cfg!(windows))
    .open(source)?
    .sync_all()?;
  create_directories(dir, mode)?;
  let placed = if overwrite {
    let to = dir.join(name);
    match std::fs::rename(source, &to) {
      Ok(()) => Ok(to),
      Err(e) if e.kind() == ErrorKind::CrossesDevices => {
        let part = copy_beside(source, dir)?;
        std::fs::rename(&part, &to)?;
        std::fs::remove_file(source)?;
        Ok(to)
      }
      Err(e) => Err(e),
    }
  } else {
    match place_new(source, dir, name) {
      Err(e) if e.kind() == ErrorKind::CrossesDevices => {
        let part = copy_beside(source, dir)?;
        let to = place_new(&part, dir, name)?;
        std::fs::remove_file(source)?;
        Ok(to)
      }
      placed => placed,
    }
  }?;
  #[cfg(unix)]
  if let Some(mode) = mode {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&placed, std::fs::Permissions::from_mode(mode))?;
  }
  // a rename is on disk only once the directories it changed are, so a power loss cannot
  // leave the file under its incomplete name
  #[cfg(unix)]
  {
    sync_directory(dir);
    if let Some(parent) = source.parent()
      && parent != dir
    {
      sync_directory(parent);
    }
  }
  if let Some(parent) = source.parent() {
    match std::fs::remove_dir(parent) {
      Ok(()) => debug!(dir = %parent.display(), "emptied directory removed"),
      // other files are in it; only an empty one goes
      Err(e) if e.kind() == ErrorKind::DirectoryNotEmpty => {}
      Err(e) => debug!(dir = %parent.display(), error = %e, "emptied directory not removed"),
    }
  }
  Ok(placed)
}

/// Flushes the entries of `dir` to disk. A failure is logged and the move stands, since the
/// file is in place by then.
#[cfg(unix)]
fn sync_directory(dir: &Path) {
  if let Err(e) = File::open(dir).and_then(|dir| dir.sync_all()) {
    warn!(dir = %dir.display(), error = %e, "directory not flushed to disk after a move");
  }
}

#[cfg(test)]
mod tests {
  use std::ffi::OsString;

  use super::*;
  use crate::safety::SEGMENT_BYTES;

  fn names(dir: &Path) -> Vec<OsString> {
    let mut names: Vec<OsString> = std::fs::read_dir(dir)
      .unwrap()
      .map(|entry| entry.unwrap().file_name())
      .collect();
    names.sort();
    names
  }

  #[test]
  fn numbered_names_keep_the_extension() {
    assert_eq!(numbered("01 - Track.flac", 0), "01 - Track.flac");
    assert_eq!(numbered("01 - Track.flac", 2), "01 - Track (2).flac");
    assert_eq!(numbered("README", 1), "README (1)");
    assert_eq!(numbered(".hidden", 1), ".hidden (1)");
  }

  #[test]
  fn numbered_long_names_fit_and_keep_whole_characters() {
    let long = format!("a{}.flac", "\u{97f3}".repeat(83));
    assert_eq!(long.len(), SEGMENT_BYTES);
    for n in [1, 9_999] {
      let name = numbered(&long, n);
      assert!(name.len() <= SEGMENT_BYTES, "{}", name.len());
      assert!(name.ends_with(&format!("\u{97f3} ({n}).flac")), "{name}");
    }
  }

  #[test]
  fn a_file_is_created_cut_at_the_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user").join("album").join("01.flac");
    drop(create_file(&path, None, 0).unwrap());
    std::fs::write(&path, b"0123456789").unwrap();
    let file = create_file(&path, None, 4).unwrap();
    assert_eq!(file.metadata().unwrap().len(), 4);
    assert_eq!(std::fs::read(&path).unwrap(), b"0123");
  }

  #[test]
  fn a_resumed_partial_must_still_hold_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("01.flac");
    let gone = create_file(&path, None, 4).unwrap_err();
    assert_eq!(gone.kind(), ErrorKind::NotFound);
    assert!(!path.exists(), "a vanished partial is not made again");
    std::fs::write(&path, b"01").unwrap();
    let short = create_file(&path, None, 4).unwrap_err();
    assert_eq!(short.kind(), ErrorKind::UnexpectedEof);
    assert_eq!(
      short.to_string(),
      "Failed to open partial file 01.flac: it holds 2 bytes, fewer than the 4 resumed from"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"01", "left as it was");
  }

  #[cfg(unix)]
  #[test]
  fn directories_and_files_take_the_configured_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("made").join("01.flac");
    drop(create_file(&path, Some(0o640), 0).unwrap());
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&dir.path().join("made")), 0o750);
    let moved = move_file(
      &path,
      &dir.path().join("final"),
      "01.flac",
      Some(0o604),
      false,
    )
    .unwrap();
    assert_eq!(mode(&moved), 0o604);
    assert_eq!(mode(&dir.path().join("final")), 0o705);
  }

  #[test]
  fn a_move_never_replaces_a_file_unless_told_to() {
    let dir = tempfile::tempdir().unwrap();
    let incoming = dir.path().join("incoming");
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("01.flac"), "kept").unwrap();
    for (n, content) in ["one", "two"].iter().enumerate() {
      let from = incoming.join(format!("user{n}")).join("01.flac");
      std::fs::create_dir_all(from.parent().unwrap()).unwrap();
      std::fs::write(&from, content).unwrap();
      let moved = move_file(&from, &downloads, "01.flac", None, false).unwrap();
      assert_eq!(std::fs::read_to_string(&moved).unwrap(), *content);
      assert!(
        !from.parent().unwrap().exists(),
        "the emptied directory goes"
      );
    }
    assert_eq!(names(&downloads), ["01 (1).flac", "01 (2).flac", "01.flac"]);
    assert_eq!(
      std::fs::read_to_string(downloads.join("01.flac")).unwrap(),
      "kept"
    );
    let from = incoming.join("01.flac");
    std::fs::write(&from, "new").unwrap();
    std::fs::write(incoming.join("other.flac"), "stays").unwrap();
    let moved = move_file(&from, &downloads, "01.flac", None, true).unwrap();
    assert_eq!(moved, downloads.join("01.flac"));
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "new");
    assert!(incoming.exists(), "a directory with other files stays");
  }

  #[test]
  fn a_hidden_copy_is_renamed_whole_to_a_free_numbered_name() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("01.flac");
    std::fs::write(&source, "copied").unwrap();
    std::fs::write(dir.path().join("taken.flac"), "x").unwrap();
    let part = copy_beside(&source, dir.path()).unwrap();
    let placed = place_new(&part, dir.path(), "taken.flac").unwrap();
    assert_eq!(placed, dir.path().join("taken (1).flac"));
    assert_eq!(std::fs::read_to_string(&placed).unwrap(), "copied");
    assert!(!part.exists());
  }

  #[test]
  fn only_a_copys_hidden_name_is_taken_for_one() {
    for random in [0, 0xdead_beef, u64::MAX] {
      assert!(is_part(OsStr::new(&part_name(random))), "{random:x}");
    }
    for name in [
      ".0123456789abcdef.partial",
      "0123456789abcdef.part",
      ".0123456789ABCDEF.part",
      ".0123456789abcde.part",
      ".0123456789abcdef0.part",
      ".hidden.part",
    ] {
      assert!(!is_part(OsStr::new(name)), "{name}");
    }
  }
}
