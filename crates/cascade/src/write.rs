//! Writing configuration files: atomically, with a `.bak` of the previous text and modes that
//! keep secrets private, and whether a file may be written at all.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::edit::FileEditor;
use crate::{Error, files};

/// Mode of a new file that holds a secret.
const SECRET_MODE: u32 = 0o600;
/// Mode of any other new file.
const PLAIN_MODE: u32 = 0o640;

impl FileEditor<'_> {
  /// Writes `text` to `path` if the file is still at `revision` (`None`: there is no file),
  /// copying the previous text to `<file>.bak` first. Returns the new revision.
  ///
  /// The write is atomic: a temporary file next to it is renamed over it. A file that exists
  /// keeps its permissions; a new one is readable only by its owner when it holds a secret, and
  /// by its group as well otherwise.
  pub fn write(&self, path: &Path, revision: Option<&str>, text: &str) -> Result<String, Error> {
    let current = files::read_optional(path)?;
    if current.as_deref().map(files::revision_of).as_deref() != revision {
      return Err(Error::Stale);
    }
    if let Some(previous) = current {
      // the backup holds what the file held, so it gets the file's mode whatever it had
      let mode = file_mode(path).map_or(Mode::KeepOr(SECRET_MODE), Mode::Set);
      write_atomic(&backup_path(path), &previous, mode)?;
    }
    let mode = if self.holds_secrets(text) {
      SECRET_MODE
    } else {
      PLAIN_MODE
    };
    write_atomic(path, text.as_bytes(), Mode::KeepOr(mode))?;
    Ok(files::revision_of(text.as_bytes()))
  }
}

/// Whether the application may write the file at `path`, and if not, why: the configuration is
/// managed by a tool (`managed_by`, e.g. `nixos`), the file is in the Nix store, or the
/// filesystem refuses.
pub fn writability(path: &Path, managed_by: Option<&str>) -> Result<(), String> {
  if let Some(tool) = managed_by {
    return Err(format!("managed by {tool}"));
  }
  // a file that does not exist yet is checked where it is named
  let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
  if target.starts_with("/nix/store") {
    return Err("in the Nix store".to_string());
  }
  // an atomic write creates a temporary file next to the target and renames it over
  let dir = target
    .parent()
    .ok_or_else(|| "has no parent directory".to_string())?;
  let probe = dir.join(format!(".{}.write-probe", std::process::id()));
  match fs::write(&probe, b"") {
    // the probe only tests writing; a failed cleanup leaves an empty file behind
    Ok(()) => drop(fs::remove_file(&probe)),
    Err(e) => return Err(format!("{} is not writable ({e})", dir.display())),
  }
  if target.exists() {
    fs::OpenOptions::new()
      .write(true)
      .open(&target)
      .map_err(|e| format!("not writable ({e})"))?;
  }
  Ok(())
}

/// The permissions a written file ends up with, where the platform has modes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mode {
  /// those of the file it replaces, or this mode for a new file
  KeepOr(u32),
  /// this mode, whatever the file had
  Set(u32),
}

/// Writes through a temporary file and a rename, so no reader sees a half-written file. A
/// symlink is followed so the link stays in place.
pub(crate) fn write_atomic(path: &Path, contents: &[u8], mode: Mode) -> Result<(), Error> {
  // a file that does not exist yet has no link to follow
  let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
  let error = |source| Error::Write {
    path: target.clone(),
    source,
  };
  let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
    return Err(error(std::io::Error::other("not a file path")));
  };
  // unique, so two writes at once do not share one temporary file
  static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
  let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  let tmp = dir.join(format!(
    ".{}.{}-{n}.tmp",
    name.to_string_lossy(),
    std::process::id()
  ));
  let existing = fs::metadata(&target).ok().map(|m| m.permissions());
  let mut options = fs::OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    // created as private as it ends up, so no reader sees the contents in between
    let (Mode::KeepOr(mode) | Mode::Set(mode)) = mode;
    options.mode(mode);
  }
  let written = options.open(&tmp).and_then(|mut file| {
    file.write_all(contents)?;
    match (mode, existing) {
      (Mode::KeepOr(_), Some(permissions)) => file.set_permissions(permissions)?,
      #[cfg(unix)]
      (Mode::Set(mode), _) => {
        use std::os::unix::fs::PermissionsExt;
        // the creation mode is narrowed by the umask, this is not
        file.set_permissions(fs::Permissions::from_mode(mode))?;
      }
      _ => {}
    }
    file.sync_all()
  });
  let replaced = written.and_then(|()| fs::rename(&tmp, &target));
  if let Err(e) = replaced {
    // the temporary file is ours and useless after a failed write; it may not exist at all
    drop(fs::remove_file(&tmp));
    return Err(error(e));
  }
  Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
  let mut name = path.file_name().unwrap_or_default().to_os_string();
  name.push(".bak");
  path.with_file_name(name)
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Option<u32> {
  use std::os::unix::fs::PermissionsExt;
  fs::metadata(path)
    .ok()
    .map(|m| m.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode(_: &Path) -> Option<u32> {
  None
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{example_schema, read_only_dirs_hold};

  #[cfg(unix)]
  fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
  }

  #[test]
  fn writes_check_the_revision_and_keep_a_backup() {
    let schema = example_schema();
    let editor = FileEditor::new(&schema);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let first = editor.write(&path, None, "[server]\nport = 1\n").unwrap();
    assert_eq!(first, files::revision_of(b"[server]\nport = 1\n"));
    assert!(
      !dir.path().join("config.toml.bak").exists(),
      "nothing to back up"
    );
    assert!(matches!(
      editor.write(&path, None, "x = 1\n"),
      Err(Error::Stale)
    ));
    assert!(matches!(
      editor.write(&path, Some("sha256:00"), "x = 1\n"),
      Err(Error::Stale)
    ));

    let second = editor
      .write(&path, Some(&first), "[server]\nport = 2\n")
      .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "[server]\nport = 2\n");
    assert_eq!(
      fs::read_to_string(dir.path().join("config.toml.bak")).unwrap(),
      "[server]\nport = 1\n"
    );
    assert_ne!(first, second);
    let left: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(left.len(), 2, "no temporary files are left behind");
  }

  #[cfg(unix)]
  #[test]
  fn writes_keep_modes_and_new_secret_files_start_private() {
    use std::os::unix::fs::PermissionsExt;
    let schema = example_schema();
    let editor = FileEditor::new(&schema);
    let dir = tempfile::tempdir().unwrap();

    let plain = dir.path().join("plain.toml");
    editor.write(&plain, None, "[server]\nport = 1\n").unwrap();
    assert_eq!(mode(&plain), 0o640 & !umask());
    let secret = dir.path().join("secret.toml");
    editor
      .write(&secret, None, "[server]\npassword = \"x\"\n")
      .unwrap();
    assert_eq!(mode(&secret), 0o600);

    fs::set_permissions(&plain, fs::Permissions::from_mode(0o604)).unwrap();
    let revision = files::revision(&plain).unwrap();
    editor
      .write(&plain, revision.as_deref(), "[server]\npassword = \"x\"\n")
      .unwrap();
    assert_eq!(mode(&plain), 0o604, "an existing file keeps its mode");
    assert_eq!(mode(&dir.path().join("plain.toml.bak")), 0o604);

    let bak = dir.path().join("secret.toml.bak");
    fs::write(&bak, "old").unwrap();
    fs::set_permissions(&bak, fs::Permissions::from_mode(0o644)).unwrap();
    let revision = files::revision(&secret).unwrap();
    editor
      .write(&secret, revision.as_deref(), "[server]\npassword = \"y\"\n")
      .unwrap();
    assert_eq!(
      mode(&bak),
      0o600,
      "a backup of a secret is as private as the file was"
    );
  }

  #[cfg(unix)]
  fn umask() -> u32 {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("probe");
    {
      use std::os::unix::fs::OpenOptionsExt;
      fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o777)
        .open(&probe)
        .unwrap();
    }
    0o777 & !mode(&probe)
  }

  #[cfg(unix)]
  #[test]
  fn a_symlinked_file_is_written_through_the_link() {
    let schema = example_schema();
    let editor = FileEditor::new(&schema);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real.toml");
    let link = dir.path().join("config.toml");
    fs::write(&target, "a = 1\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let revision = files::revision(&link).unwrap();
    editor.write(&link, revision.as_deref(), "a = 2\n").unwrap();
    assert!(
      fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "a = 2\n");
  }

  #[test]
  fn writes_at_once_do_not_clobber_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");
    let contents: Vec<String> = (0..8).map(|i| format!("{i}").repeat(100_000)).collect();
    std::thread::scope(|s| {
      for text in &contents {
        let file = &file;
        s.spawn(move || {
          for _ in 0..20 {
            write_atomic(file, text.as_bytes(), Mode::KeepOr(0o640)).unwrap();
          }
        });
      }
    });
    let written = fs::read_to_string(&file).unwrap();
    assert!(contents.contains(&written), "one write's contents, whole");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
  }

  #[test]
  fn writability_says_why_not() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");
    assert_eq!(
      writability(&file, None),
      Ok(()),
      "a missing file in a writable directory"
    );
    fs::write(&file, "").unwrap();
    assert_eq!(writability(&file, None), Ok(()));
    assert_eq!(
      writability(&file, Some("nixos")),
      Err("managed by nixos".to_string())
    );
    assert_eq!(
      fs::read_dir(dir.path()).unwrap().count(),
      1,
      "the probe is gone"
    );

    #[cfg(unix)]
    if read_only_dirs_hold(dir.path()) {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
      let err = writability(&file, None).unwrap_err();
      fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
      assert!(err.contains("is not writable"), "{err}");
    }
  }
}
