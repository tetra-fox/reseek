//! Where a program's configuration files are: `config.toml` in its configuration directory, or
//! the files given on its command line, then the drop-ins beside the first.

use std::path::{Path, PathBuf};

/// The configuration files: a directory and the files in it, in the order they apply.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigFiles {
  /// the directory the names are relative to; the `config.d` drop-ins are in it
  pub dir: PathBuf,
  /// the files, then the drop-in pattern
  pub names: Vec<String>,
}

/// A configuration file or directory given by a path that cannot be made absolute.
#[derive(Debug, thiserror::Error)]
#[error("The configuration files could not be found: {} has no absolute path: {source}", path.display())]
pub struct NotFound {
  /// the path as given
  pub path: PathBuf,
  /// why
  pub source: std::io::Error,
}

impl ConfigFiles {
  /// `config.toml` in `config_dir`, or the files given on the command line, followed by every
  /// `config.d/*.toml` next to the first of them. Relative paths count from the working
  /// directory, with their `..` resolved by name, so a relative path in the files counts from
  /// a directory without one.
  pub fn new(given: &[PathBuf], config_dir: &Path) -> Result<Self, NotFound> {
    let absolute = |path: &Path| {
      cascade::absolute(path).map_err(|source| NotFound {
        path: path.to_path_buf(),
        source,
      })
    };
    let Some((first, rest)) = given.split_first() else {
      return Ok(Self {
        dir: absolute(config_dir)?,
        names: vec!["config.toml".to_string(), "config.d/*.toml".to_string()],
      });
    };
    let first = absolute(first)?;
    let dir = first
      .parent()
      .map_or_else(|| absolute(config_dir), |dir| Ok(dir.to_path_buf()))?;
    let name = |path: &Path| match path.strip_prefix(&dir) {
      Ok(relative) => relative.display().to_string(),
      Err(_) => path.display().to_string(),
    };
    let mut names = vec![name(&first)];
    for path in rest {
      names.push(name(&absolute(path)?));
    }
    names.push("config.d/*.toml".to_string());
    Ok(Self { dir, names })
  }

  /// The first file, which is where a new configuration is written.
  pub fn first(&self) -> PathBuf {
    self.dir.join(&self.names[0])
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn given_files_come_first_then_the_drop_ins_next_to_the_first() {
    let files = ConfigFiles::new(&[], Path::new("/home/u/.config/reseek")).unwrap();
    assert_eq!(files.names, ["config.toml", "config.d/*.toml"]);
    assert_eq!(
      files.first(),
      Path::new("/home/u/.config/reseek/config.toml")
    );
    let files = ConfigFiles::new(
      &[
        "/etc/reseek/config.toml".into(),
        "/run/secrets/reseek.toml".into(),
      ],
      Path::new("/var/lib/reseek"),
    )
    .unwrap();
    assert_eq!(files.dir, Path::new("/etc/reseek"));
    assert_eq!(
      files.names,
      ["config.toml", "/run/secrets/reseek.toml", "config.d/*.toml"]
    );
  }

  #[test]
  fn config_files_given_relative_count_from_the_working_directory() {
    let here = std::env::current_dir().unwrap();
    let files =
      ConfigFiles::new(&["etc/reseek.toml".into()], Path::new("/var/lib/reseek")).unwrap();
    assert_eq!(files.first(), here.join("etc/reseek.toml"));
    let files = ConfigFiles::new(&[], Path::new("conf")).unwrap();
    assert_eq!(files.dir, here.join("conf"));
    let refused = ConfigFiles::new(&["".into()], Path::new("/etc")).unwrap_err();
    assert!(
      refused
        .to_string()
        .starts_with("The configuration files could not be found"),
      "{refused}"
    );
  }

  #[test]
  fn a_dot_dot_in_a_given_path_is_resolved() {
    let parent = std::env::current_dir().unwrap().parent().unwrap().join("x");
    let files = ConfigFiles::new(&["../x/reseek.toml".into()], Path::new("/etc")).unwrap();
    assert_eq!(files.dir, parent);
    assert!(!files.dir.to_string_lossy().contains(".."), "{files:?}");
    let files = ConfigFiles::new(&[], Path::new("../x")).unwrap();
    assert_eq!(files.dir, parent);
  }
}
