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

/// Calls `f` on each path in `value`, a value of `field`, with what `others`, other values of
/// the field, hold in its place: themselves when the field is a path, else the paths at the
/// same key in any of their entries, since an entry may have been renamed or moved.
fn each_path<'a, E>(
  field: &Field,
  value: &mut Value,
  others: &[&'a Value],
  f: &mut impl FnMut(&mut Value, &[&'a Value]) -> Result<(), E>,
) -> Result<(), E> {
  if field.path {
    return f(value, others);
  }
  if field.entries.is_empty() {
    return Ok(());
  }
  let entries: Vec<&mut Value> = match value {
    Value::Object(entries) if field.kind == Kind::Map => entries.values_mut().collect(),
    Value::Array(items) => items.iter_mut().collect(),
    // a value of the wrong shape is refused when the configuration is read into its type
    _ => return Ok(()),
  };
  let other_entries: Vec<&Value> = others.iter().flat_map(|other| entries_of(other)).collect();
  for entry in entries {
    for inner in &field.entries {
      if let Some(value) = tree::get_mut(entry, &inner.key) {
        let others: Vec<&Value> = other_entries
          .iter()
          .filter_map(|other| tree::get(other, &inner.key))
          .collect();
        each_path(inner, value, &others, f)?;
      }
    }
  }
  Ok(())
}

/// The entries of a map's or a list's value.
fn entries_of(value: &Value) -> Vec<&Value> {
  match value {
    Value::Object(entries) => entries.values().collect(),
    Value::Array(items) => items.iter().collect(),
    _ => Vec::new(),
  }
}

/// Makes each relative path in `value`, a value of `field`, absolute against `base`: the
/// directory of the file that sets it, or the working directory when `None`.
pub(crate) fn resolve(field: &Field, value: &mut Value, base: Option<&Path>) -> Result<(), String> {
  each_path(field, value, &[], &mut |value, _| resolve_one(value, base))
}

/// [`resolve`] for a value set at run time, where nothing says what a relative path counts
/// from, so one is refused. The message does not quote the path, which may be secret.
pub(crate) fn resolve_runtime(field: &Field, value: &mut Value) -> Result<(), String> {
  each_path(field, value, &[], &mut |value, _| {
    if let Value::String(text) = value
      && !text.is_empty()
      && !Path::new(text.as_str()).is_absolute()
    {
      return Err("a path set at run time must be absolute".to_string());
    }
    resolve_one(value, None)
  })
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
/// the resolved values, and sends back a whole map or list when it changes one entry.
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
      tree::get(&written, key).as_slice(),
      &mut |value, written| {
        let same = written.iter().find(|written| {
          let mut resolved = Value::clone(written);
          resolve_one(&mut resolved, Some(dir)).is_ok() && resolved == *value
        });
        if let Some(written) = same {
          *value = Value::clone(written);
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
