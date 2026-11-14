//! Path options: making a relative one absolute, from the file that sets it or the working
//! directory, and writing one back to its file as the file had it.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::describe::Kind;
use crate::schema::{Field, Schema};
use crate::{Key, tree};

/// `path` made absolute against the working directory, without `.` and `..`: `..` leaves the
/// place the part before it leads to, as the system does when it opens the path, so after a
/// link it leaves the link's target. A part that does not exist yet leads nowhere else, so its
/// `..` drops it by name.
pub fn absolute(path: &Path) -> std::io::Result<PathBuf> {
  let mut resolved = PathBuf::new();
  for part in std::path::absolute(path)?.components() {
    match part {
      Component::CurDir => {}
      Component::ParentDir => {
        match std::fs::canonicalize(&resolved) {
          Ok(real) => resolved = real,
          Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
          Err(e) => return Err(e),
        }
        // the root has no parent, so `/..` stays `/`
        resolved.pop();
      }
      part => resolved.push(part),
    }
  }
  Ok(resolved)
}

/// Calls `f` on each path in `value`, a value of `field`, with what `other`, another value of
/// the field, holds in the same place: the value itself when the field is a path, else the
/// paths in its entries, matched by name in a map and by position in a list.
fn each_path<'a, E>(
  field: &Field,
  value: &mut Value,
  other: Option<&'a Value>,
  f: &mut impl FnMut(&mut Value, Option<&'a Value>) -> Result<(), E>,
) -> Result<(), E> {
  if field.path {
    return f(value, other);
  }
  if field.entries.is_empty() {
    return Ok(());
  }
  let entries: Vec<(&mut Value, Option<&Value>)> = match value {
    Value::Object(entries) if field.kind == Kind::Map => entries
      .iter_mut()
      .map(|(name, entry)| (entry, other.and_then(|other| other.get(name.as_str()))))
      .collect(),
    Value::Array(items) => items
      .iter_mut()
      .enumerate()
      .map(|(i, item)| (item, other.and_then(|other| other.get(i))))
      .collect(),
    // a value of the wrong shape is refused when the configuration is read into its type
    _ => return Ok(()),
  };
  for (entry, other) in entries {
    for inner in &field.entries {
      if let Some(value) = tree::get_mut(entry, &inner.key) {
        let other = other.and_then(|other| tree::get(other, &inner.key));
        each_path(inner, value, other, f)?;
      }
    }
  }
  Ok(())
}

/// Makes each relative path in `value`, a value of `field`, absolute against `base`: the
/// directory of the file that sets it, or the working directory when `None`.
pub(crate) fn resolve(field: &Field, value: &mut Value, base: Option<&Path>) -> Result<(), String> {
  each_path(field, value, None, &mut |value, _| resolve_one(value, base))
}

/// [`resolve`] for one path, which an absolute one goes through too for its `.` and `..`. An
/// empty one names no path and stays, for the application's checks to refuse.
fn resolve_one(value: &mut Value, base: Option<&Path>) -> Result<(), String> {
  let Value::String(text) = value else {
    return Ok(());
  };
  if text.is_empty() {
    return Ok(());
  }
  // joining an absolute path gives the path itself
  let joined = match base {
    Some(dir) => dir.join(text.as_str()),
    None => PathBuf::from(text.as_str()),
  };
  let resolved =
    absolute(&joined).map_err(|e| format!("{} cannot be resolved: {e}", joined.display()))?;
  *text = resolved
    .into_os_string()
    .into_string()
    .map_err(|path| format!("{} is not UTF-8", Path::new(&path).display()))?;
  Ok(())
}

/// Puts back in each value of `set`, to be written to the file in `dir` that holds `text`, the
/// paths as the file writes them wherever the value holds them as they resolve: a client reads
/// the resolved values, and sends back a whole map when it changes one entry.
pub(crate) fn as_written(schema: &Schema, set: &mut BTreeMap<Key, Value>, text: &str, dir: &Path) {
  // a file that does not parse is refused by the edit itself
  let Ok(table) = text.parse::<toml::Table>() else {
    return;
  };
  let Ok(written) = tree::from_toml(toml::Value::Table(table)) else {
    return;
  };
  for (key, value) in set {
    let Some(field) = schema.field(key) else {
      continue;
    };
    let kept: Result<(), Infallible> = each_path(
      field,
      value,
      tree::get(&written, key),
      &mut |value, written| {
        if let Some(written) = written {
          let mut resolved = written.clone();
          if resolve_one(&mut resolved, Some(dir)).is_ok() && resolved == *value {
            *value = written.clone();
          }
        }
        Ok(())
      },
    );
    let Ok(()) = kept;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn dots_resolve_by_name() {
    assert_eq!(
      absolute(Path::new("/srv/reseek/../conf/./x.toml")).unwrap(),
      Path::new("/srv/conf/x.toml")
    );
    assert_eq!(absolute(Path::new("/..")).unwrap(), Path::new("/"));
    let here = std::env::current_dir().unwrap();
    assert_eq!(
      absolute(Path::new("../conf")).unwrap(),
      here.parent().unwrap().join("conf")
    );
    assert!(absolute(Path::new("")).is_err());
  }

  #[cfg(unix)]
  #[test]
  fn dot_dot_after_a_link_leaves_where_the_link_leads() {
    let dir = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(dir.join("releases/42")).unwrap();
    std::fs::create_dir_all(dir.join("releases/shared")).unwrap();
    std::os::unix::fs::symlink(dir.join("releases/42"), dir.join("current")).unwrap();
    let file = dir.join("current/../shared/reseek.toml");
    std::fs::write(&file, "x").unwrap();
    let resolved = absolute(&file).unwrap();
    assert_eq!(resolved, dir.join("releases/shared/reseek.toml"));
    assert_eq!(
      std::fs::read(&resolved).unwrap(),
      b"x",
      "the file the system opens"
    );
    assert_eq!(
      absolute(&dir.join("current/new/../x")).unwrap(),
      dir.join("current/x"),
      "a part that does not exist is dropped by name"
    );
  }
}
