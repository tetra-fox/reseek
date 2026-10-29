//! Keeps a configuration current: reloads it when its files change or when asked, applies and
//! expires runtime overrides, writes the files, and reports each change grouped by when it
//! takes effect.

mod edits;
mod events;
mod reload;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{Notify, broadcast, watch};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::files::{Entry, LoadedFile};
use crate::layers::{Checked, Failure, Layers};
use crate::loaded::Loaded;
use crate::overrides::Lasts;
use crate::{Error, Key, Root, lock};
use events::{DEBOUNCE, FileEvents, quiet};

/// How long to wait before reading the files again after a reload failed, since an editor that
/// truncates and then writes leaves a broken file for a moment.
const RETRY: Duration = Duration::from_millis(500);

/// How many changes a slow listener may fall behind by before it misses some.
const CHANGES: usize = 16;

/// How the files are watched for changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
  /// the operating system's file notifications
  Events,
  /// reading the files' metadata at this interval, for mounts where notifications do not arrive
  Poll(Duration),
  /// not at all; [`Watcher::reload`] still reloads
  Off,
}

/// The keys of a change, by when each takes effect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ByApply {
  /// in effect now
  pub live: Vec<Key>,
  /// in effect after the next reconnect
  pub reconnect: Vec<Key>,
  /// in effect after a restart
  pub restart: Vec<Key>,
}

/// A change to the values in effect.
#[derive(Debug)]
pub struct Change<T> {
  /// the leaf keys whose values changed
  pub changed: Vec<Key>,
  /// the same keys by when each takes effect
  pub by_apply: ByApply,
  /// the configuration before the change
  pub previous: Arc<Loaded<T>>,
  /// the configuration after it
  pub current: Arc<Loaded<T>>,
}

/// The result of writing a file.
#[derive(Debug)]
pub struct Written<T> {
  /// the file's new revision
  pub revision: String,
  /// what the reload after the write changed, `None` when nothing did
  pub change: Option<Arc<Change<T>>>,
}

/// The current configuration and the task that keeps it current. Cloning gives another handle
/// to the same configuration.
pub struct Watcher<T> {
  shared: Arc<Shared<T>>,
}

impl<T> Clone for Watcher<T> {
  fn clone(&self) -> Self {
    Self {
      shared: self.shared.clone(),
    }
  }
}

struct Shared<T> {
  /// the files and patterns the configuration is read from, which do not change
  entries: Vec<Entry>,
  /// only locked on blocking threads
  state: Mutex<State<T>>,
  current: watch::Sender<Arc<Loaded<T>>>,
  changes: broadcast::Sender<Arc<Change<T>>>,
  /// the runtime overrides changed, so the next expiry may have moved
  overrides_changed: Notify,
}

struct State<T> {
  layers: Layers<T>,
  /// the values at the start, which `restart` keys keep running with
  startup: Value,
  /// the values at the last reconnect
  reconnected: Value,
  /// the files as the last reload read them, to skip events that changed nothing
  attempted: Vec<LoadedFile>,
}

/// What one reading of the files came to.
enum Attempt<T> {
  /// the files were as the last attempt read them, so they were not loaded
  Unchanged,
  Done(Option<Arc<Change<T>>>),
  Failed(Failure),
}

/// How a file is to be changed.
enum FileEdit {
  Patch {
    set: BTreeMap<Key, Value>,
    unset: Vec<Key>,
  },
  Replace(String),
}

impl<T: Root> Watcher<T> {
  /// Loads the configuration, reading the files on this thread. An error here is a
  /// configuration the application can not start with. Call [`Watcher::start`] to have it kept
  /// current.
  pub fn new(mut layers: Layers<T>) -> Result<Self, Error> {
    let loaded = layers.load()?;
    for file in &loaded.files {
      let state = if file.revision.is_some() {
        "read"
      } else {
        "absent"
      };
      info!(file = file.name, path = %file.path.display(), state, "configuration file");
    }
    let entries = layers.entries().to_vec();
    let state = State {
      startup: loaded.tree.clone(),
      reconnected: loaded.tree.clone(),
      attempted: loaded.files.clone(),
      layers,
    };
    Ok(Self {
      shared: Arc::new(Shared {
        entries,
        state: Mutex::new(state),
        current: watch::Sender::new(Arc::new(loaded)),
        changes: broadcast::Sender::new(CHANGES),
        overrides_changed: Notify::new(),
      }),
    })
  }

  /// Starts the task that reloads on file changes (as `watch` says) and ends runtime
  /// overrides when they expire. It stops when `cancel` is canceled. Call it once.
  pub fn start(&self, watch: Watch, cancel: CancellationToken) {
    let _span = info_span!("config_watcher").entered();
    let events = Arc::new(Notify::new());
    let poll = match watch {
      Watch::Events => Some(None),
      Watch::Poll(interval) => Some(Some(interval)),
      Watch::Off => None,
    };
    // watched before this returns, so no change made after it is missed
    let files = poll.and_then(|poll| {
      match FileEvents::new(poll, self.shared.entries.clone(), events.clone()) {
        Ok(files) => Some(files),
        Err(e) => {
          warn!(error = %e, "configuration files are not watched; reload with a signal or the API");
          None
        }
      }
    });
    let this = self.clone();
    tokio::spawn(this.run(files, events, cancel).in_current_span());
  }

  /// The configuration in effect.
  pub fn current(&self) -> Arc<Loaded<T>> {
    self.shared.current.borrow().clone()
  }

  /// Every new state of the configuration, including failed reloads, which keep the values
  /// and set `load_error`.
  pub fn subscribe(&self) -> watch::Receiver<Arc<Loaded<T>>> {
    self.shared.current.subscribe()
  }

  /// Every change to the values in effect.
  pub fn changes(&self) -> broadcast::Receiver<Arc<Change<T>>> {
    self.shared.changes.subscribe()
  }

  /// Reads the files again, as a change to them would: a failure is tried once more after a
  /// moment, then reported, and the values in effect stay. Returns what changed.
  pub async fn reload(&self) -> Result<Option<Arc<Change<T>>>, Error> {
    self.reload_files(false).await
  }

  /// What the configuration would load with if the file `name` held `text`: its warnings, and
  /// why it would not load.
  pub async fn check_file(&self, name: &str, text: &str) -> Result<Checked, Error> {
    let (name, text) = (name.to_string(), text.to_string());
    self
      .blocking(move |state, _| state.layers.check_file(&name, &text))
      .await
  }

  /// Sets the keys of `set` and removes those of `unset` in the file `name`, keeping its
  /// comments, then reloads. `revision` is the file's revision the edit was made against,
  /// `None` when there was no file.
  ///
  /// Refused, in this order, when the file changed since `revision`, can not be written, would
  /// leave a configuration that does not load, or when a key's value comes from an env var or
  /// flag, or from a later file, which this file would not beat. For a secret, `*****` keeps the value and an empty
  /// string removes it.
  pub async fn patch_file(
    &self,
    name: &str,
    revision: Option<&str>,
    set: BTreeMap<Key, Value>,
    unset: Vec<Key>,
  ) -> Result<Written<T>, Error> {
    self
      .write_file(name, revision, FileEdit::Patch { set, unset })
      .await
  }

  /// Replaces the whole text of the file `name`, with the checks of [`Watcher::patch_file`].
  pub async fn replace_file(
    &self,
    name: &str,
    revision: Option<&str>,
    text: String,
  ) -> Result<Written<T>, Error> {
    self
      .write_file(name, revision, FileEdit::Replace(text))
      .await
  }

  /// Sets runtime overrides for the keys of `set`, lasting as `lasts` says, and removes those
  /// for `unset`, saving them before they take effect. Refused for a key that only takes effect
  /// on restart, or whose value comes from an env var or flag.
  pub async fn set_overrides(
    &self,
    set: BTreeMap<Key, Value>,
    unset: Vec<Key>,
    set_by: &str,
    lasts: Lasts,
  ) -> Result<Option<Arc<Change<T>>>, Error> {
    let set_by = set_by.to_string();
    self
      .blocking(move |state, shared| state.change_overrides(shared, set, &unset, &set_by, lasts))
      .await?
  }

  /// Removes the runtime override for `key`.
  pub async fn remove_override(&self, key: &str) -> Result<Option<Arc<Change<T>>>, Error> {
    let key = key.to_string();
    self
      .blocking(move |state, shared| {
        if !state.layers.overrides.items().contains_key(&key) {
          return Err(Error::NoOverride(key));
        }
        state.change_overrides(shared, BTreeMap::new(), &[key], "", Lasts::UntilRemoved)
      })
      .await?
  }

  /// Marks the values in effect as the ones the application reconnected with, which clears
  /// `pending_reconnect`.
  pub async fn reconnected(&self) -> Result<(), Error> {
    self
      .blocking(|state, shared| {
        let current = shared.current.borrow().clone();
        state.reconnected = current.tree.clone();
        let mut loaded = (*current).clone();
        loaded.pending_reconnect.clear();
        shared.current.send_replace(Arc::new(loaded));
      })
      .await
  }

  async fn run(
    self,
    mut files: Option<FileEvents>,
    events: Arc<Notify>,
    cancel: CancellationToken,
  ) {
    loop {
      let next_expiry = self
        .current()
        .overrides
        .values()
        .filter_map(|o| o.expires_at)
        .min();
      let expiry = async {
        match next_expiry {
          // an expiry already past is a negative wait, which ends at once
          Some(at) => tokio::time::sleep((at - Utc::now()).to_std().unwrap_or_default()).await,
          None => std::future::pending().await,
        }
      };
      tokio::select! {
        () = cancel.cancelled() => break,
        () = events.notified() => {
          if !quiet(&events, DEBOUNCE, &cancel).await {
            break;
          }
          if let Some(files) = &mut files {
            files.watch_new_dirs();
          }
          // a failure is logged and kept in the current state where it happens
          drop(self.reload_files(true).await);
        }
        () = expiry => {
          let reapplied = self.blocking(|state, shared| state.reapply(shared)).await;
          // a failure is logged and kept in the current state where it happens
          drop(reapplied);
        }
        () = self.shared.overrides_changed.notified() => {}
      }
    }
    debug!("configuration watcher stopped");
  }

  /// Reloads the files, once more after [`RETRY`] if that fails, then reports the failure.
  /// With `only_if_changed`, files as the last attempt read them are not loaded again.
  async fn reload_files(&self, only_if_changed: bool) -> Result<Option<Arc<Change<T>>>, Error> {
    let first = self
      .blocking(move |state, shared| state.attempt(shared, only_if_changed))
      .await?;
    let failure = match first {
      Attempt::Unchanged => return Ok(None),
      Attempt::Done(change) => return Ok(change),
      Attempt::Failed(failure) => failure,
    };
    debug!(error = %failure.error, "configuration reload failed, trying again");
    tokio::time::sleep(RETRY).await;
    self
      .blocking(move |state, shared| match state.attempt(shared, true) {
        Attempt::Unchanged => Err(state.report(shared, failure)),
        Attempt::Done(change) => Ok(change),
        Attempt::Failed(again) => Err(state.report(shared, again)),
      })
      .await?
  }

  async fn write_file(
    &self,
    name: &str,
    revision: Option<&str>,
    edit: FileEdit,
  ) -> Result<Written<T>, Error> {
    let name = name.to_string();
    let revision = revision.map(str::to_string);
    self
      .blocking(move |state, shared| state.write_file(shared, &name, revision.as_deref(), edit))
      .await?
  }

  /// Runs `f` on the state on a blocking thread, since loading reads files.
  async fn blocking<R: Send + 'static>(
    &self,
    f: impl FnOnce(&mut State<T>, &Shared<T>) -> R + Send + 'static,
  ) -> Result<R, Error> {
    let shared = self.shared.clone();
    let task = tokio::task::spawn_blocking(move || {
      let mut state = lock(&shared.state);
      f(&mut state, &shared)
    });
    match task.await {
      Ok(result) => Ok(result),
      Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
      // the runtime is shutting down
      Err(_) => Err(Error::Stopped),
    }
  }
}
