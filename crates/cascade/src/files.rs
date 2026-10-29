//! Which configuration files exist, their names as shown to users, and their revisions.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Error;

/// A file or a pattern of files, relative to `dir` unless absolute.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
  pub(crate) dir: PathBuf,
  pub(crate) pattern: String,
}

/// A configuration file: its name for users, where it is, and its revision when it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedFile {
  /// as the files were given, e.g. `config.toml` or `config.d/10-shares.toml`
  pub name: String,
  /// where the file is
  pub path: PathBuf,
  /// a hash of the contents, `None` when the file does not exist
  pub revision: Option<String>,
}

impl Entry {
  /// Where a pattern's matches live, `None` for a plain file.
  pub(crate) fn pattern_dir(&self) -> Option<PathBuf> {
    let (parent, last) = split_last(&self.pattern);
    is_pattern(last).then(|| self.dir.join(parent))
  }

  /// The plain file this entry names, `None` for a pattern.
  pub(crate) fn plain_path(&self) -> Option<PathBuf> {
    let (_, last) = split_last(&self.pattern);
    (!is_pattern(last)).then(|| self.dir.join(&self.pattern))
  }

  /// Whether a file with this name, in `dir`, is one the entry names.
  pub(crate) fn names(&self, dir: &Path, name: &str) -> bool {
    match (self.plain_path(), self.pattern_dir()) {
      (Some(path), _) => path.parent() == Some(dir) && path.file_name() == Some(name.as_ref()),
      (None, Some(pattern_dir)) => pattern_dir == dir && matches(split_last(&self.pattern).1, name),
      (None, None) => false,
    }
  }
}

/// The files `entries` name, in order: a plain file whether or not it exists, then each
/// pattern's matches in lexical order. A file named twice is listed once, where it comes last.
/// A pattern's directory that exists but can not be listed is an error, so its files are never
/// taken for gone.
pub(crate) fn expand(entries: &[Entry]) -> Result<Vec<(String, PathBuf)>, Error> {
  let mut files: Vec<(String, PathBuf)> = Vec::new();
  for entry in entries {
    let (parent, last) = split_last(&entry.pattern);
    if !is_pattern(last) {
      files.push((entry.pattern.clone(), entry.dir.join(&entry.pattern)));
      continue;
    }
    let dir = entry.dir.join(parent);
    let listing = match std::fs::read_dir(&dir) {
      Ok(listing) => listing,
      // a pattern's directory that does not exist yet holds no files
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
      Err(source) => return Err(Error::Read { path: dir, source }),
    };
    let mut names: Vec<String> = listing
      // an entry that vanished while listed is a file that is gone
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
      // a name that is not UTF-8 matches no pattern, which are UTF-8
      .filter_map(|e| e.file_name().into_string().ok())
      .filter(|name| matches(last, name))
      .collect();
    names.sort();
    for name in names {
      let shown = if parent.is_empty() {
        name.clone()
      } else {
        format!("{parent}/{name}")
      };
      files.push((shown, dir.join(name)));
    }
  }
  let mut seen = std::collections::HashSet::new();
  let mut deduped: Vec<_> = files
    .into_iter()
    .rev()
    .filter(|(_, path)| seen.insert(path.clone()))
    .collect();
  deduped.reverse();
  Ok(deduped)
}

/// A hash of a file's contents, `sha256:` and hex.
pub(crate) fn revision_of(contents: &[u8]) -> String {
  let digest = Sha256::digest(contents);
  let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
  format!("sha256:{hex}")
}

/// The revision of the file at `path`, `None` when there is no file.
pub(crate) fn revision(path: &Path) -> Result<Option<String>, Error> {
  Ok(read_optional(path)?.map(|contents| revision_of(&contents)))
}

/// The contents of the file at `path`, `None` when there is no file.
pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, Error> {
  match std::fs::read(path) {
    Ok(contents) => Ok(Some(contents)),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
    Err(source) => Err(Error::Read {
      path: path.to_path_buf(),
      source,
    }),
  }
}

/// [`read_optional`] for a text file.
pub(crate) fn read_optional_text(path: &Path) -> Result<Option<String>, Error> {
  read_optional(path)?
    .map(|contents| {
      String::from_utf8(contents).map_err(|e| Error::Read {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
      })
    })
    .transpose()
}

fn split_last(pattern: &str) -> (&str, &str) {
  pattern.rsplit_once('/').unwrap_or(("", pattern))
}

fn is_pattern(name: &str) -> bool {
  name.contains(['*', '?'])
}

/// Shell-style matching of one file name: `*` is any run of characters, `?` any one. A name
/// starting with `.` only matches a pattern that does too.
fn matches(pattern: &str, name: &str) -> bool {
  if name.starts_with('.') && !pattern.starts_with('.') {
    return false;
  }
  let pattern: Vec<char> = pattern.chars().collect();
  let name: Vec<char> = name.chars().collect();
  // the last `*` seen and where in `name` it was tried up to, to backtrack to
  let (mut p, mut n) = (0, 0);
  let mut star: Option<(usize, usize)> = None;
  while n < name.len() {
    match pattern.get(p) {
      Some('*') => {
        star = Some((p, n));
        p += 1;
      }
      Some(&c) if c == '?' || c == name[n] => {
        p += 1;
        n += 1;
      }
      _ => match star {
        Some((sp, sn)) => {
          p = sp + 1;
          n = sn + 1;
          star = Some((sp, sn + 1));
        }
        None => return false,
      },
    }
  }
  pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  #[test]
  fn shell_style_names() {
    for (pattern, name, expected) in [
      ("*.toml", "10-shares.toml", true),
      ("*.toml", "config.toml.bak", false),
      ("*.toml", ".hidden.toml", false),
      (".*.toml", ".hidden.toml", true),
      ("*.toml", ".config.toml.12-0.tmp", false),
      ("??-*.toml", "10-a.toml", true),
      ("??-*.toml", "1-a.toml", false),
      ("a*b*c", "aXbYbZc", true),
      ("a*b*c", "aXbYbZ", false),
      ("*", "anything", true),
    ] {
      assert_eq!(matches(pattern, name), expected, "{pattern} {name}");
    }
  }

  #[test]
  fn plain_files_then_matches_in_lexical_order() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("config.d")).unwrap();
    for name in ["20-b.toml", "10-a.toml", "notes.txt", "10-a.toml.bak"] {
      fs::write(dir.path().join("config.d").join(name), "").unwrap();
    }
    fs::create_dir(dir.path().join("config.d/sub.toml")).unwrap();
    let entries = [
      Entry {
        dir: dir.path().into(),
        pattern: "config.toml".into(),
      },
      Entry {
        dir: dir.path().into(),
        pattern: "config.d/*.toml".into(),
      },
    ];
    let names: Vec<String> = expand(&entries)
      .unwrap()
      .into_iter()
      .map(|(n, _)| n)
      .collect();
    assert_eq!(
      names,
      ["config.toml", "config.d/10-a.toml", "config.d/20-b.toml"],
      "config.toml is listed though it does not exist"
    );

    assert!(entries[0].names(dir.path(), "config.toml"));
    assert!(entries[1].names(&dir.path().join("config.d"), "30-c.toml"));
    assert!(!entries[1].names(dir.path(), "30-c.toml"));
    assert_eq!(entries[1].pattern_dir(), Some(dir.path().join("config.d")));
    assert_eq!(entries[0].pattern_dir(), None);

    let missing = [Entry {
      dir: dir.path().into(),
      pattern: "nope.d/*.toml".into(),
    }];
    assert!(expand(&missing).unwrap().is_empty());

    fs::write(dir.path().join("file.d"), "").unwrap();
    let not_a_dir = [Entry {
      dir: dir.path().into(),
      pattern: "file.d/*.toml".into(),
    }];
    assert!(
      matches!(expand(&not_a_dir), Err(Error::Read { .. })),
      "a directory that can not be listed is not an empty one"
    );
  }

  #[test]
  fn a_file_named_twice_counts_where_it_comes_last() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.toml"), "").unwrap();
    fs::write(dir.path().join("b.toml"), "").unwrap();
    let entry = |pattern: &str| Entry {
      dir: dir.path().into(),
      pattern: pattern.into(),
    };
    let names: Vec<String> = expand(&[entry("b.toml"), entry("*.toml")])
      .unwrap()
      .into_iter()
      .map(|(n, _)| n)
      .collect();
    assert_eq!(names, ["a.toml", "b.toml"]);
  }
}
