//! Merging the layers below the runtime one: the defaults, environment variables, the files and
//! the command line.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

use serde_json::Value;
use tracing::debug;

use super::{LIST_SEPARATOR, Layers, SECRET_VALUE};
use crate::describe::Kind;
use crate::files::{self, LoadedFile};
use crate::schema::Field;
use crate::source::Source;
use crate::{Error, Key, Root, paths, tree};

/// The layers below the runtime one, merged.
#[derive(Clone, Debug)]
pub(crate) struct Declared {
  pub(crate) tree: Value,
  pub(crate) sources: BTreeMap<Key, Source>,
  /// the last file that sets each key, whatever layer wins it
  pub(crate) file_of: BTreeMap<Key, String>,
  pub(crate) files: Vec<LoadedFile>,
  pub(crate) warnings: Vec<String>,
}

impl<T: Root> Layers<T> {
  /// Merges the defaults, environment, files and command line. With `substitute`, the file of
  /// that name is read as holding that text.
  pub(crate) fn declare(&self, substitute: Option<(&str, &str)>) -> Result<Declared, Error> {
    let mut declared = Declared {
      tree: self.defaults.clone(),
      sources: self
        .schema
        .fields()
        .iter()
        .map(|f| (f.key.clone(), Source::Default))
        .collect(),
      file_of: BTreeMap::new(),
      files: Vec::new(),
      warnings: Vec::new(),
    };
    self.declare_env(&mut declared)?;
    let mut substituted = false;
    for (name, path) in files::expand(&self.entries)? {
      let text = match substitute {
        Some((n, text)) if n == name => {
          substituted = true;
          Some(text.to_string())
        }
        _ => files::read_optional_text(&path)?,
      };
      let revision = text.as_deref().map(|t| files::revision_of(t.as_bytes()));
      debug!(file = name, revision, "configuration file read");
      declared.files.push(LoadedFile {
        name: name.clone(),
        path: path.clone(),
        revision,
      });
      let Some(text) = text else {
        continue;
      };
      let table: toml::Table = text.parse().map_err(|e: toml::de::Error| Error::Parse {
        file: name.clone(),
        message: tree::parse_error(&text, e.message(), e.span()),
      })?;
      let file = File {
        name: &name,
        dir: path.parent().expect("a file's path ends in its name"),
      };
      self.declare_file(&file, table, "", &mut declared)?;
    }
    if let Some((name, _)) = substitute
      && !substituted
    {
      return Err(Error::UnknownFile(name.to_string()));
    }
    self.declare_cli(&mut declared)?;
    Ok(declared)
  }

  fn declare_env(&self, declared: &mut Declared) -> Result<(), Error> {
    for field in self.schema.fields() {
      let mut winner: Option<(&str, &OsStr)> = None;
      for name in field.env_names() {
        let Some(raw) = self.env.get(OsStr::new(name)) else {
          continue;
        };
        match winner {
          None => winner = Some((name, raw)),
          Some((wins, value)) if value != raw => declared
            .warnings
            .push(format!("{name}: ignored, {wins} is set too and wins")),
          Some(_) => {}
        }
      }
      let Some((var, raw)) = winner else {
        continue;
      };
      let error = |message: String| Error::Value {
        key: field.key.clone(),
        by: var.to_string(),
        message: if field.secret {
          SECRET_VALUE.to_string()
        } else {
          message
        },
      };
      let raw = raw
        .to_str()
        .ok_or_else(|| error("the value is not UTF-8".to_string()))?;
      let mut value = tree::coerce(raw, &field.kind, Some(LIST_SEPARATOR)).map_err(error)?;
      paths::resolve(field, &mut value, None).map_err(error)?;
      debug!(key = field.key, var, "set by the environment");
      tree::set(&mut declared.tree, &field.key, value);
      declared.sources.insert(
        field.key.clone(),
        Source::Env {
          var: var.to_string(),
        },
      );
    }
    Ok(())
  }

  fn declare_file(
    &self,
    file: &File,
    table: toml::Table,
    prefix: &str,
    declared: &mut Declared,
  ) -> Result<(), Error> {
    let name = file.name;
    for (k, v) in table {
      let key = if prefix.is_empty() {
        k
      } else {
        format!("{prefix}.{k}")
      };
      let shape_error = |message: String| Error::Value {
        key: key.clone(),
        by: name.to_string(),
        message,
      };
      if let Some(field) = self.schema.field(&key) {
        let mut v = v;
        drop_unknown_in_entries(field, &mut v, &key, name, &mut declared.warnings);
        let mut value = tree::from_toml(v).map_err(shape_error)?;
        paths::resolve(field, &mut value, Some(file.dir)).map_err(shape_error)?;
        tree::set(&mut declared.tree, &key, value);
        declared.sources.insert(
          key.clone(),
          Source::File {
            file: name.to_string(),
          },
        );
        declared.file_of.insert(key, name.to_string());
      } else if self.schema.is_section(&key) {
        match v {
          toml::Value::Table(inner) => self.declare_file(file, inner, &key, declared)?,
          _ => {
            return Err(shape_error(format!(
              "a section, written [{key}], not a single value"
            )));
          }
        }
      } else {
        declared
          .warnings
          .push(format!("{name}: unknown key `{key}`"));
      }
    }
    Ok(())
  }

  fn declare_cli(&self, declared: &mut Declared) -> Result<(), Error> {
    let mut lists: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for (flag, raw) in &self.cli {
      let Some(field) = self.schema.by_flag(flag) else {
        declared.warnings.push(format!("{flag}: unknown flag"));
        continue;
      };
      let error = |message| Error::Value {
        key: field.key.clone(),
        by: flag.clone(),
        message: if field.secret {
          SECRET_VALUE.to_string()
        } else {
          message
        },
      };
      match &field.kind {
        Kind::List(item) => lists
          .entry(&field.key)
          .or_default()
          .push(tree::coerce(raw, item, None).map_err(error)?),
        kind => {
          let mut value = tree::coerce(raw, kind, None).map_err(error)?;
          paths::resolve(field, &mut value, None).map_err(error)?;
          tree::set(&mut declared.tree, &field.key, value);
        }
      }
      declared
        .sources
        .insert(field.key.clone(), Source::Cli { flag: flag.clone() });
    }
    for (key, values) in lists {
      tree::set(&mut declared.tree, key, Value::Array(values));
    }
    Ok(())
  }
}

/// A configuration file being read: its name for users and the directory it is in.
struct File<'a> {
  name: &'a str,
  dir: &'a Path,
}

/// Takes out of each entry of a user-keyed map or list of tables the keys its type does not
/// have, with a warning for each, as the files' other unknown keys are. `shown` is where the
/// value is, for the warning.
fn drop_unknown_in_entries(
  field: &Field,
  value: &mut toml::Value,
  shown: &str,
  file: &str,
  warnings: &mut Vec<String>,
) {
  if field.entries.is_empty() {
    return;
  }
  match value {
    toml::Value::Table(entries) if field.kind == Kind::Map => {
      for (name, entry) in entries.iter_mut() {
        if let toml::Value::Table(entry) = entry {
          drop_unknown(
            &field.entries,
            entry,
            "",
            &format!("{shown}.{name}"),
            file,
            warnings,
          );
        }
      }
    }
    toml::Value::Array(items) => {
      for (i, item) in items.iter_mut().enumerate() {
        if let toml::Value::Table(item) = item {
          drop_unknown(
            &field.entries,
            item,
            "",
            &format!("{shown}[{i}]"),
            file,
            warnings,
          );
        }
      }
    }
    // a value of the wrong shape is refused when the configuration is read into its type
    _ => {}
  }
}

/// [`drop_unknown_in_entries`] for one entry, whose keys from `prefix` on are those of
/// `fields`.
fn drop_unknown(
  fields: &[Field],
  table: &mut toml::Table,
  prefix: &str,
  shown: &str,
  file: &str,
  warnings: &mut Vec<String>,
) {
  let mut unknown = Vec::new();
  for (k, v) in table.iter_mut() {
    let key = if prefix.is_empty() {
      k.clone()
    } else {
      format!("{prefix}.{k}")
    };
    let here = format!("{shown}.{k}");
    if let Some(field) = fields.iter().find(|f| f.key == key) {
      drop_unknown_in_entries(field, v, &here, file, warnings);
    } else if fields.iter().any(|f| f.key.starts_with(&format!("{key}."))) {
      if let toml::Value::Table(inner) = v {
        drop_unknown(fields, inner, &key, &here, file, warnings);
      }
    } else {
      warnings.push(format!("{file}: unknown key `{here}`"));
      unknown.push(k.clone());
    }
  }
  for k in unknown {
    table.remove(&k);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{Example, Located, Mode, example_layers, pairs, write};
  use chrono::Utc;
  use std::fs;
  use std::path::PathBuf;

  fn file(name: &str) -> Source {
    Source::File { file: name.into() }
  }

  fn env_source(var: &str) -> Source {
    Source::Env { var: var.into() }
  }

  #[test]
  fn each_layer_beats_the_one_below_and_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    write(
      dir.path(),
      "config.toml",
      "[server]\nport = 2\naddress = \"file\"\n",
    );
    write(
      dir.path(),
      "config.d/10-a.toml",
      "[server]\naddress = \"drop-in\"\n",
    );
    let env = pairs(&[("APP_SERVER_PORT", "1"), ("APP_USERNAME", "env")]);
    let cli = pairs(&[("--server-port", "3")]);

    let loaded = example_layers(dir.path())
      .env(env.clone())
      .cli(cli)
      .load()
      .unwrap();
    assert_eq!(loaded.values.server.port, 3);
    assert_eq!(
      loaded.sources["server.port"],
      Source::Cli {
        flag: "--server-port".into()
      }
    );
    assert_eq!(loaded.values.server.address, "drop-in");
    assert_eq!(loaded.sources["server.address"], file("config.d/10-a.toml"));
    assert_eq!(loaded.values.server.username, "env");
    assert_eq!(
      loaded.sources["server.username"],
      env_source("APP_USERNAME")
    );
    assert_eq!(loaded.sources["server.timeout"], Source::Default);
    assert_eq!(
      loaded.file_for("server.address"),
      Some("config.d/10-a.toml")
    );
    assert_eq!(loaded.file_for("server.port"), Some("config.toml"));
    assert_eq!(
      loaded.file_for("debug"),
      Some("config.toml"),
      "a key no file sets goes to the first file"
    );

    let loaded = example_layers(dir.path()).env(env.clone()).load().unwrap();
    assert_eq!(loaded.values.server.port, 2, "the file beats env");
    assert_eq!(loaded.sources["server.port"], file("config.toml"));

    fs::remove_file(dir.path().join("config.toml")).unwrap();
    let loaded = example_layers(dir.path()).env(env).load().unwrap();
    assert_eq!(loaded.values.server.port, 1);
    assert_eq!(loaded.sources["server.port"], env_source("APP_SERVER_PORT"));
  }

  #[test]
  fn a_relative_path_counts_from_the_file_that_sets_it_or_the_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let layers = || Layers::<Located>::new().files(dir, ["config.toml", "config.d/*.toml"]);
    write(dir, "config.toml", "data = \"dl\"\nname = \"dl\"\n");
    let loaded = layers().load().unwrap();
    assert_eq!(loaded.values.data, Some(dir.join("dl")));
    assert_eq!(
      loaded.values.name, "dl",
      "a value that is not a path is kept"
    );

    write(
      dir,
      "config.d/10-a.toml",
      "data = \"./music/dl\"\n[providers.home]\nca = \"ca.pem\"\n[[mirrors]]\nca = \"../m.pem\"\n",
    );
    let loaded = layers().load().unwrap();
    assert_eq!(
      loaded.tree()["data"].as_str().map(Path::new),
      Some(dir.join("config.d/music/dl").as_path())
    );
    assert!(
      !loaded.tree()["data"].as_str().unwrap().contains("/./"),
      "the path is written without its `.`"
    );
    assert_eq!(
      loaded.values.providers["home"].ca,
      Some(dir.join("config.d/ca.pem")),
      "a path inside an entry counts from its file too"
    );
    assert_eq!(
      loaded.tree()["mirrors"][0]["ca"].as_str().map(Path::new),
      Some(dir.join("m.pem").as_path()),
      "and one in a list of tables, its `..` resolved by name"
    );

    let absolute = format!("{}/sub/./../dl", dir.display());
    write(
      dir,
      "config.d/10-a.toml",
      &format!("data = \"{absolute}\"\n"),
    );
    let loaded = layers().load().unwrap();
    assert_eq!(
      loaded.tree()["data"].as_str().map(Path::new),
      Some(dir.join("dl").as_path()),
      "an absolute path's `.` and `..` resolve as a relative one's do"
    );
    write(dir, "config.d/10-a.toml", "data = \"\"\n");
    let loaded = layers().load().unwrap();
    assert_eq!(loaded.values.data, Some(PathBuf::new()));

    std::fs::remove_dir_all(dir.join("config.d")).unwrap();
    std::fs::remove_file(dir.join("config.toml")).unwrap();
    let here = std::env::current_dir().unwrap();
    let loaded = layers()
      .env(pairs(&[("LOC_DATA", "from-env"), ("LOC_NAME", "x")]))
      .load()
      .unwrap();
    assert_eq!(loaded.values.data, Some(here.join("from-env")));
    let loaded = layers()
      .cli(pairs(&[("--data", "./from-cli")]))
      .load()
      .unwrap();
    assert_eq!(loaded.values.data, Some(here.join("from-cli")));
  }

  #[test]
  fn lists_and_user_keyed_maps_are_replaced_whole() {
    let dir = tempfile::tempdir().unwrap();
    write(
      dir.path(),
      "config.toml",
      "[shares]\ndirectories = [\"/x\"]\n[api_keys.a]\nkey = \"aaaaaaaaaaaaaaaa\"\n",
    );
    let env = pairs(&[("APP_SHARED_DIR", "/one;/two;/three")]);
    let loaded = example_layers(dir.path()).env(env.clone()).load().unwrap();
    assert_eq!(
      loaded.values.shares.directories,
      ["/x"],
      "the file's list replaces env's, it does not merge by index"
    );
    assert_eq!(loaded.sources["shares.directories"], file("config.toml"));

    let cli = pairs(&[("--shared", "/a"), ("--shared", "/b")]);
    let loaded = example_layers(dir.path())
      .env(env.clone())
      .cli(cli)
      .load()
      .unwrap();
    assert_eq!(loaded.values.shares.directories, ["/a", "/b"]);

    fs::remove_file(dir.path().join("config.toml")).unwrap();
    let loaded = example_layers(dir.path()).env(env).load().unwrap();
    assert_eq!(loaded.values.shares.directories, ["/one", "/two", "/three"]);

    write(
      dir.path(),
      "config.toml",
      "[api_keys.a]\nkey = \"aaaaaaaaaaaaaaaa\"\n",
    );
    write(
      dir.path(),
      "config.d/10-keys.toml",
      "[api_keys.b]\nkey = \"bbbbbbbbbbbbbbbb\"\nrole = \"admin\"\n",
    );
    let loaded = example_layers(dir.path()).load().unwrap();
    assert_eq!(
      loaded.values.api_keys.keys().collect::<Vec<_>>(),
      ["b"],
      "a later file's table replaces the whole map"
    );
    assert_eq!(loaded.sources["api_keys"], file("config.d/10-keys.toml"));
    assert_eq!(loaded.values.api_keys["b"].role, "admin");
  }

  #[test]
  fn env_aliases_win_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let load = |vars: &[(&str, &str)]| example_layers(dir.path()).env(pairs(vars)).load().unwrap();

    let loaded = load(&[("APP_SERVER_PORT", "1"), ("NEW_SERVER_PORT", "2")]);
    assert_eq!(
      loaded.values.server.port, 2,
      "the prefix alias beats the main name"
    );
    assert_eq!(loaded.sources["server.port"], env_source("NEW_SERVER_PORT"));
    assert_eq!(
      loaded.warnings,
      ["APP_SERVER_PORT: ignored, NEW_SERVER_PORT is set too and wins"]
    );

    let loaded = load(&[
      ("APP_SERVER_PORT", "1"),
      ("NEW_SERVER_PORT", "2"),
      ("NEW_PORT", "3"),
    ]);
    assert_eq!(
      loaded.values.server.port, 3,
      "the field's own alias comes first"
    );
    assert_eq!(loaded.warnings.len(), 2);

    let loaded = load(&[("APP_SERVER_PORT", "4"), ("NEW_SERVER_PORT", "4")]);
    assert!(
      loaded.warnings.is_empty(),
      "the same value twice is no conflict"
    );
  }

  #[test]
  fn env_values_are_read_as_their_kind() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = example_layers(dir.path())
      .env(pairs(&[
        ("APP_DEBUG", "TRUE"),
        ("APP_SERVER_TIMEOUT", "1m 30s"),
        ("APP_SERVER_BUFFER", "1048576"),
        ("APP_SHARE_LIMITS", "1;2"),
        ("APP_CACHE_MODE", "disk"),
        ("APP_SERVER_PASSWORD", ""),
      ]))
      .load()
      .unwrap();
    assert!(loaded.values.debug);
    assert_eq!(loaded.values.server.timeout.as_secs(), 90);
    assert_eq!(loaded.values.server.buffer, bytesize::ByteSize::mib(1));
    assert_eq!(loaded.values.shares.limits, [1, 2]);
    assert_eq!(loaded.values.shares.cache.mode, Mode::Disk);
    assert_eq!(
      loaded.values.server.password.as_deref(),
      Some(""),
      "an empty variable is set, to the empty string"
    );

    let loaded = example_layers(dir.path())
      .env(pairs(&[("APP_SERVER_BUFFER", "2 MiB")]))
      .cli(pairs(&[("--debug", "true")]))
      .load()
      .unwrap();
    assert_eq!(loaded.values.server.buffer, bytesize::ByteSize::mib(2));
    assert!(loaded.values.debug);
  }

  #[cfg(unix)]
  #[test]
  fn a_variable_that_is_not_utf8_is_an_error_only_when_used() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let bad = std::ffi::OsString::from_vec(vec![0xff, 0xfe]);
    let loaded = example_layers(dir.path())
      .env([("UNRELATED", bad.clone())])
      .load();
    assert!(loaded.is_ok());
    let e = example_layers(dir.path())
      .env([("APP_USERNAME", bad)])
      .load()
      .unwrap_err();
    assert!(e.to_string().contains("not UTF-8"), "{e}");
  }

  #[test]
  fn unknown_keys_and_flags_are_warnings() {
    let dir = tempfile::tempdir().unwrap();
    write(
      dir.path(),
      "config.toml",
      "[server]\nprot = 1\n[nope]\nx = 1\n[api_keys.mine]\nkey = \"k\"\n",
    );
    let loaded = example_layers(dir.path())
      .cli(pairs(&[("--color", "red")]))
      .load()
      .unwrap();
    assert_eq!(
      loaded.warnings,
      [
        "config.toml: unknown key `nope`",
        "config.toml: unknown key `server.prot`",
        "--color: unknown flag",
      ]
    );
  }

  #[test]
  fn unknown_keys_inside_entries_are_warnings_too() {
    let dir = tempfile::tempdir().unwrap();
    write(
      dir.path(),
      "config.toml",
      "[api_keys.mine]\nkey = \"k\"\nrolle = \"admin\"\n\n[[webhooks]]\nurl = \"u\"\ntokn = \"t\"\n",
    );
    let loaded = example_layers(dir.path()).load().unwrap();
    assert_eq!(
      loaded.warnings,
      [
        "config.toml: unknown key `api_keys.mine.rolle`",
        "config.toml: unknown key `webhooks[0].tokn`",
      ],
      "and they are dropped, so a type that refuses unknown fields still loads"
    );
    assert_eq!(loaded.values.api_keys["mine"].key, "k");
    assert_eq!(loaded.values.webhooks[0].token, None);
  }

  #[test]
  fn bad_values_name_the_key_and_what_set_it() {
    let dir = tempfile::tempdir().unwrap();
    let error = |mut layers: Layers<Example>| layers.load_at(Utc::now()).err().unwrap().error;

    let e = error(example_layers(dir.path()).env(pairs(&[("APP_SERVER_PORT", "fifty")])));
    assert!(
      matches!(&e, Error::Value { key, by, .. } if key == "server.port" && by == "APP_SERVER_PORT"),
      "{e}"
    );

    let e = error(example_layers(dir.path()).cli(pairs(&[("--debug", "maybe")])));
    assert!(
      matches!(&e, Error::Value { by, .. } if by == "--debug"),
      "{e}"
    );

    write(dir.path(), "config.toml", "[server]\nport = \"fifty\"\n");
    let e = error(example_layers(dir.path()));
    assert!(
      matches!(&e, Error::Value { key, by, .. } if key == "server.port" && by == "config.toml"),
      "{e}"
    );

    write(dir.path(), "config.toml", "server = 5\n");
    let e = error(example_layers(dir.path()));
    assert!(e.to_string().contains("a section, written [server]"), "{e}");

    write(dir.path(), "config.toml", "[server]\nport = 1\nport = 2\n");
    let e = error(example_layers(dir.path()));
    assert!(
      matches!(&e, Error::Parse { file, message } if file == "config.toml" && message.starts_with("line 3, column 1:")),
      "{e}"
    );

    write(
      dir.path(),
      "config.toml",
      "[api_keys.mine]\nkey = \"k\"\nrole = 5\n",
    );
    let e = error(example_layers(dir.path()));
    assert!(
      matches!(&e, Error::Value { key, message, .. } if key == "api_keys" && message.contains("(at api_keys.mine.role")),
      "{e}"
    );
    assert!(
      e.to_string().contains("invalid type: integer `5`"),
      "a field of an entry that is not secret is quoted: {e}"
    );

    write(dir.path(), "config.toml", "[server]\npassword = 12345\n");
    let e = error(example_layers(dir.path()));
    assert!(
      matches!(&e, Error::Value { key, message, .. } if key == "server.password" && !message.contains("12345")),
      "a secret's value is not quoted: {e}"
    );
    write(dir.path(), "config.toml", "[api_keys.mine]\nkey = 12345\n");
    let e = error(example_layers(dir.path()));
    assert!(!e.to_string().contains("12345"), "{e}");
    write(dir.path(), "config.toml", "[api_keys]\nmine = 12345\n");
    let e = error(example_layers(dir.path()));
    assert!(
      !e.to_string().contains("12345"),
      "an entry that holds a secret is not quoted whole: {e}"
    );
    write(dir.path(), "config.toml", "[[webhooks]]\nurl = 5\n");
    let e = error(example_layers(dir.path()));
    assert!(e.to_string().contains("integer `5`"), "{e}");
  }
}
