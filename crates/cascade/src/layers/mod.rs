//! The layers a configuration is loaded from, lowest first: the defaults, environment
//! variables, the files, the command line, and the runtime overrides. A later layer replaces an
//! earlier one's value per key; a list or user-keyed map is one value, replaced whole.

mod declare;

pub(crate) use declare::Declared;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{info, warn};

use crate::files::{self, Entry, LoadedFile};
use crate::loaded::Loaded;
use crate::overrides::{Override, Overrides};
use crate::schema::Schema;
use crate::source::Source;
use crate::{Error, Invalid, Key, Root, tree};

/// What a message about a secret's value says instead of quoting it.
pub(crate) const SECRET_VALUE: &str = "the value does not fit the key (not shown, it is secret)";

/// What splits a list given in one environment variable. Elements are kept as written.
pub(crate) const LIST_SEPARATOR: char = ';';

/// What a file's text would load as.
#[derive(Debug)]
pub struct Checked {
  /// keys, flags and variables the configuration would warn about; empty when it is not read
  /// that far, such as when the text does not parse
  pub warnings: Vec<String>,
  /// why the configuration would not load, `None` when it would
  pub error: Option<Error>,
}

/// The application's checks of a configuration, beyond what its types enforce.
type Validator<T> = Box<dyn Fn(&T) -> Result<(), Vec<Invalid>> + Send + Sync>;

/// Builds a configuration from its layers and loads it again on demand.
///
/// ```
/// # use cascade::{Cascade, Layers};
/// # #[derive(Clone, Default, serde::Serialize, serde::Deserialize, Cascade)]
/// # #[serde(default)]
/// # struct Options {
/// #   /// The port.
/// #   #[cascade(env = "APP_PORT", cli = "--port")]
/// #   port: u16,
/// # }
/// # let dir = tempfile::tempdir().unwrap();
/// # std::fs::write(dir.path().join("config.toml"), "port = 8080\n").unwrap();
/// let mut layers = Layers::<Options>::new()
///   .env([("APP_PORT".to_string(), "2000".to_string())])
///   .files(dir.path(), ["config.toml", "config.d/*.toml"])
///   .cli([("--port".to_string(), "5030".to_string())]);
/// let loaded = layers.load().unwrap();
/// assert_eq!(loaded.values.port, 5030);
/// assert_eq!(loaded.sources["port"], cascade::Source::Cli { flag: "--port".into() });
/// ```
pub struct Layers<T> {
  schema: Arc<Schema>,
  defaults: Value,
  env: BTreeMap<OsString, OsString>,
  entries: Vec<Entry>,
  cli: Vec<(String, String)>,
  pub(crate) overrides: Overrides,
  validator: Option<Validator<T>>,
  managed_by: Option<String>,
  /// the layers below the runtime one as last loaded, so overrides can be applied again
  /// without reading the files
  pub(crate) last: Option<Declared>,
  _values: PhantomData<fn() -> T>,
}

/// A configuration that did not load, with the merged values it was built from when it got
/// that far, to tell which keys the failure held back.
pub(crate) struct Failure {
  pub(crate) error: Error,
  pub(crate) tree: Option<Value>,
}

impl From<Error> for Failure {
  fn from(error: Error) -> Self {
    Self { error, tree: None }
  }
}

impl<T: Root> Default for Layers<T> {
  fn default() -> Self {
    Self::new()
  }
}

impl<T: Root> Layers<T> {
  /// The defaults alone: `T::default()`.
  ///
  /// # Panics
  ///
  /// When `T`'s schema can not be built; see [`Schema::of`].
  pub fn new() -> Self {
    let schema = Schema::of::<T>();
    let defaults = serde_json::to_value(T::default()).expect("checked by Schema::of");
    Self {
      schema: Arc::new(schema),
      defaults,
      env: BTreeMap::new(),
      entries: Vec::new(),
      cli: Vec::new(),
      overrides: Overrides::default(),
      validator: None,
      managed_by: None,
      last: None,
      _values: PhantomData,
    }
  }

  /// Environment variables, such as `std::env::vars_os()`, read once: reloads see the same
  /// values. Only variables a field names are used. A list is one variable with its elements
  /// split by `;` and kept as written; an empty variable is an empty list.
  pub fn env<K: Into<OsString>, V: Into<OsString>>(
    mut self,
    vars: impl IntoIterator<Item = (K, V)>,
  ) -> Self {
    self.env = vars
      .into_iter()
      .map(|(k, v)| (k.into(), v.into()))
      .collect();
    self
  }

  /// Configuration files, relative to `dir` unless absolute, in the order they apply: a later
  /// file wins. A name whose last part has `*` or `?` is a pattern, and its matches apply in
  /// lexical order. A file that does not exist sets nothing. May be called again for more
  /// files, which apply after these.
  pub fn files<S: Into<String>>(
    mut self,
    dir: impl AsRef<Path>,
    patterns: impl IntoIterator<Item = S>,
  ) -> Self {
    // absolute, so it compares equal to the paths file notifications carry
    let dir = std::path::absolute(dir.as_ref()).unwrap_or_else(|_| dir.as_ref().to_path_buf());
    self
      .entries
      .extend(patterns.into_iter().map(|pattern| Entry {
        dir: dir.clone(),
        pattern: pattern.into(),
      }));
    self
  }

  /// Values from the command line, as `(long flag, value)` in the order given. A list flag
  /// given more than once collects every value; any other keeps the last. A flag without a
  /// value, such as a bare `--debug`, is given as `"true"`.
  pub fn cli(mut self, values: impl IntoIterator<Item = (String, String)>) -> Self {
    self.cli = values.into_iter().collect();
    self
  }

  /// The runtime overrides. Without a store they are kept in memory and end with the process.
  pub fn overrides(mut self, store: Overrides) -> Self {
    self.overrides = store;
    self
  }

  /// The application's checks, run on every load: a configuration they refuse does not load.
  pub fn validate(
    mut self,
    validator: impl Fn(&T) -> Result<(), Vec<Invalid>> + Send + Sync + 'static,
  ) -> Self {
    self.validator = Some(Box::new(validator));
    self
  }

  /// The tool that manages the configuration files, such as `nixos`; the files are then never
  /// written.
  pub fn managed_by(mut self, tool: Option<String>) -> Self {
    self.managed_by = tool.filter(|t| !t.is_empty());
    self
  }

  /// The schema of the configuration's type.
  pub fn schema(&self) -> &Schema {
    &self.schema
  }

  /// Loads every layer. Overrides that expired or whose declared value moved are dropped and the
  /// store is saved without them.
  pub fn load(&mut self) -> Result<Loaded<T>, Error> {
    self.load_at(Utc::now()).map_err(|failure| failure.error)
  }

  /// What the configuration would load with if the file `name` held `text`: its warnings, and
  /// why it would not load. Nothing is saved.
  pub fn check_file(&self, name: &str, text: &str) -> Checked {
    let declared = match self.declare(Some((name, text))) {
      Ok(declared) => declared,
      Err(error) => {
        return Checked {
          warnings: Vec::new(),
          error: Some(error),
        };
      }
    };
    Checked {
      error: self.refusal(&declared),
      warnings: declared.warnings,
    }
  }

  /// Why the configuration would not load from `declared` with the overrides that still apply
  /// over it, `None` when it would.
  pub(crate) fn refusal(&self, declared: &Declared) -> Option<Error> {
    let (kept, _) =
      self
        .overrides
        .pruned(&self.schema, &declared.tree, &declared.sources, Utc::now());
    self.finish(declared, &kept).err().map(|f| f.error)
  }

  /// Where the file `name` is, when it is one of the configuration's files.
  pub(crate) fn file_path(&self, name: &str) -> Result<Option<PathBuf>, Error> {
    Ok(
      files::expand(&self.entries)?
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, path)| path),
    )
  }

  pub(crate) fn entries(&self) -> &[Entry] {
    &self.entries
  }

  pub(crate) fn managed(&self) -> Option<&str> {
    self.managed_by.as_deref()
  }

  pub(crate) fn schema_arc(&self) -> &Arc<Schema> {
    &self.schema
  }

  /// The files as they are on disk now, with their revisions.
  pub(crate) fn files_now(&self) -> Result<Vec<LoadedFile>, Error> {
    files::expand(&self.entries)?
      .into_iter()
      .map(|(name, path)| {
        Ok(LoadedFile {
          revision: files::revision(&path)?,
          name,
          path,
        })
      })
      .collect()
  }

  pub(crate) fn load_at(&mut self, now: DateTime<Utc>) -> Result<Loaded<T>, Failure> {
    let declared = self.declare(None)?;
    for warning in &declared.warnings {
      warn!(%warning, "configuration warning");
    }
    let loaded = self.apply(&declared, now)?;
    self.last = Some(declared);
    Ok(loaded)
  }

  /// Puts the runtime layer on `declared`: drops the overrides that no longer apply, then
  /// those the configuration can not load with, saving both removals.
  pub(crate) fn apply(
    &mut self,
    declared: &Declared,
    now: DateTime<Utc>,
  ) -> Result<Loaded<T>, Failure> {
    let (kept, dropped) =
      self
        .overrides
        .pruned(&self.schema, &declared.tree, &declared.sources, now);
    if !dropped.is_empty() {
      for (key, reason) in &dropped {
        info!(key, reason, "runtime override dropped");
      }
      self.overrides.keep_only(kept);
    }
    loop {
      let failure = match self.finish(declared, self.overrides.items()) {
        Ok(loaded) => return Ok(loaded),
        Err(failure) => failure,
      };
      // an override saved by an older version can stop fitting its key
      let keys = error_keys(&failure.error);
      let (bad, kept): (BTreeMap<Key, Override>, _) = self
        .overrides
        .items()
        .clone()
        .into_iter()
        .partition(|(key, _)| keys.contains(key.as_str()));
      if bad.is_empty() {
        return Err(failure);
      }
      for key in bad.keys() {
        warn!(key, error = %failure.error, "runtime override dropped, the configuration does not load with it");
      }
      self.overrides.keep_only(kept);
    }
  }

  /// The configuration with `overrides` on `declared`, deserialized and validated.
  pub(crate) fn finish(
    &self,
    declared: &Declared,
    overrides: &BTreeMap<Key, Override>,
  ) -> Result<Loaded<T>, Failure> {
    let mut tree = declared.tree.clone();
    let mut sources = declared.sources.clone();
    for (key, o) in overrides {
      tree::set(&mut tree, key, o.value.clone());
      sources.insert(
        key.clone(),
        Source::Runtime {
          expires_at: o.expires_at,
          until_restart: o.until_restart,
          set_by: o.set_by.clone(),
          set_at: o.set_at,
        },
      );
    }
    let values: T = match serde_path_to_error::deserialize(&tree) {
      Ok(values) => values,
      Err(e) => {
        let path = e.path().to_string();
        let key = self
          .schema
          .field_under(&path)
          .map_or(path.clone(), |f| f.key.clone());
        let by = sources
          .get(&key)
          .map_or("the defaults", Source::describe)
          .to_string();
        let mut message = if self.schema.may_hold_secret(&path) {
          SECRET_VALUE.to_string()
        } else {
          e.inner().to_string()
        };
        if path != key {
          message = format!("{message} (at {path})");
        }
        return Err(Failure {
          error: Error::Value { key, by, message },
          tree: Some(tree),
        });
      }
    };
    if let Some(validator) = &self.validator
      && let Err(invalid) = validator(&values)
    {
      return Err(Failure {
        error: Error::Invalid(invalid),
        tree: Some(tree),
      });
    }
    Ok(Loaded {
      values,
      sources,
      warnings: declared.warnings.clone(),
      load_error: None,
      load_error_at: None,
      not_applied: BTreeMap::new(),
      files: declared.files.clone(),
      overrides: overrides.clone(),
      pending_restart: Vec::new(),
      pending_reconnect: Vec::new(),
      managed_by: self.managed_by.clone(),
      schema: self.schema.clone(),
      tree,
      file_of: declared.file_of.clone(),
    })
  }
}

/// The keys an error is about.
pub(crate) fn error_keys(error: &Error) -> BTreeSet<&str> {
  match error {
    Error::Value { key, .. } => BTreeSet::from([key.as_str()]),
    Error::Invalid(invalid) => invalid.iter().map(|i| i.key.as_str()).collect(),
    _ => BTreeSet::new(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{Example, example_layers, write};
  use serde_json::json;

  #[test]
  fn defaults_alone() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = example_layers(dir.path()).load().unwrap();
    assert_eq!(loaded.values, Example::default());
    assert!(loaded.sources.values().all(|s| *s == Source::Default));
    assert_eq!(loaded.sources.len(), loaded.schema().fields().len());
    assert!(loaded.warnings.is_empty());
    assert_eq!(
      loaded.files.len(),
      1,
      "config.toml, though it does not exist"
    );
    assert_eq!(loaded.files[0].revision, None);
  }

  #[test]
  fn a_runtime_override_beats_the_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "config.toml", "[server]\nport = 2\n");
    let mut overrides = Overrides::load(dir.path().join("overrides.toml")).unwrap();
    let at = Utc::now();
    overrides
      .replace(BTreeMap::from([(
        "server.port".to_string(),
        Override {
          value: json!(9),
          declared: json!(2),
          set_by: "admin".into(),
          set_at: at,
          expires_at: None,
          until_restart: false,
        },
      )]))
      .unwrap();
    let loaded = example_layers(dir.path())
      .overrides(overrides)
      .load()
      .unwrap();
    assert_eq!(loaded.values.server.port, 9);
    assert_eq!(
      loaded.sources["server.port"],
      Source::Runtime {
        expires_at: None,
        until_restart: false,
        set_by: "admin".into(),
        set_at: at
      }
    );
    assert_eq!(loaded.overrides.len(), 1);
  }

  #[test]
  fn an_override_ends_when_the_files_move_the_value_under_it() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "config.toml", "[server]\nport = 5\n");
    let path = dir.path().join("overrides.toml");
    let mut overrides = Overrides::load(&path).unwrap();
    overrides
      .replace(BTreeMap::from([(
        "server.port".to_string(),
        Override {
          value: json!(9),
          declared: json!(2),
          set_by: "admin".into(),
          set_at: Utc::now(),
          expires_at: None,
          until_restart: false,
        },
      )]))
      .unwrap();
    let loaded = example_layers(dir.path())
      .overrides(overrides)
      .load()
      .unwrap();
    assert_eq!(loaded.values.server.port, 5);
    assert!(loaded.overrides.is_empty());
    assert!(
      Overrides::load(&path).unwrap().items().is_empty(),
      "the drop is saved"
    );
  }

  #[test]
  fn an_override_the_configuration_no_longer_takes_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.toml");
    let mut overrides = Overrides::load(&path).unwrap();
    overrides
      .replace(BTreeMap::from([(
        "server.port".to_string(),
        Override {
          value: json!("not a port"),
          declared: json!(2242),
          set_by: "an older version".into(),
          set_at: Utc::now(),
          expires_at: None,
          until_restart: false,
        },
      )]))
      .unwrap();
    let loaded = example_layers(dir.path())
      .overrides(overrides)
      .load()
      .unwrap();
    assert_eq!(loaded.values.server.port, 2242);
    assert!(Overrides::load(&path).unwrap().items().is_empty());
  }

  #[test]
  fn the_validator_refuses_a_configuration() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "config.toml", "[server]\nport = 80\n");
    let validated = || {
      example_layers(dir.path()).validate(|o: &Example| {
        if o.server.port < 1024 {
          Err(vec![Invalid::new("server.port", "must be 1024 or more")])
        } else {
          Ok(())
        }
      })
    };
    let e = validated().load().unwrap_err();
    assert_eq!(
      e.to_string(),
      "invalid configuration: server.port: must be 1024 or more"
    );
    let checked = validated().check_file("config.toml", "[server]\nport = 2000\n");
    assert!(checked.error.is_none() && checked.warnings.is_empty());
    let checked = validated().check_file("config.toml", "[server]\nport = 80\nprot = 1\n");
    assert!(matches!(checked.error, Some(Error::Invalid(_))));
    assert_eq!(
      checked.warnings,
      ["config.toml: unknown key `server.prot`"],
      "the warnings come with the refusal"
    );
    let checked = validated().check_file("config.toml", "[server\n");
    assert!(matches!(checked.error, Some(Error::Parse { .. })) && checked.warnings.is_empty());
    assert!(matches!(
      validated().check_file("nope.toml", "").error,
      Some(Error::UnknownFile(_))
    ));
  }
}
