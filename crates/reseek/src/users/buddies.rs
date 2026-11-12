//! Our buddies: the users we listed, held in memory from `users.db` so every answer to another
//! user knows at once which shares they see, the buddies group being one that shares can be
//! shown to. We may ask to be told when a buddy's presence changes.
//! Every buddy is watched at each login and when added, so the server tells us their presence
//! from one login to the next. Whether a request in a buddy's name comes from the buddy is
//! decided in `identity`.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use api_types::events::{BuddyPresenceChanged, Event};
use api_types::live::LiveFrame;
use api_types::users::{Buddy, BuddyChange, UserPresence};
use reseek_shares::lock;
use rusqlite::Connection;
use soulseek_client::Client;
use soulseek_proto::SERVER_USERNAME;
use tracing::{Instrument, debug, info};

use super::store::{self, Change};
use super::{Directory, groups};
use crate::api::error::ApiError;

/// Every buddy by name, as `users.db` has them.
pub struct Buddies {
  all: Mutex<HashMap<String, Buddy>>,
  /// held while a buddy is changed, from reading their settings to the row and the map holding
  /// the new ones, so of two changes made at once the second builds on the first
  changing: tokio::sync::Mutex<()>,
}

impl Buddies {
  /// The buddies of `users.db` as `conn` reads them.
  pub fn read(conn: &Connection) -> rusqlite::Result<Self> {
    Ok(Self {
      all: Mutex::new(store::buddies(conn)?.into_iter().collect()),
      changing: tokio::sync::Mutex::new(()),
    })
  }

  fn all(&self) -> MutexGuard<'_, HashMap<String, Buddy>> {
    lock(&self.all)
  }

  /// Waits until no other buddy change runs, and holds off the next one until it is dropped.
  pub async fn changing(&self) -> tokio::sync::MutexGuard<'_, ()> {
    self.changing.lock().await
  }

  /// `username`'s row now says `buddy`.
  pub fn set(&self, username: &str, buddy: Option<Buddy>) {
    match buddy {
      Some(buddy) => self.all().insert(username.to_string(), buddy),
      None => self.all().remove(username),
    };
  }
}

/// `change` applied to `current`, the settings of a user who may not be a buddy yet.
fn changed(current: Option<Buddy>, change: BuddyChange) -> Buddy {
  let current = current.unwrap_or_default();
  Buddy {
    notify_presence: change.notify_presence.unwrap_or(current.notify_presence),
  }
}

impl Directory {
  /// `username`'s settings as our buddy, `None` when they are not one.
  pub fn buddy(&self, username: &str) -> Option<Buddy> {
    self.buddies.all().get(username).copied()
  }

  /// Every buddy, by name.
  pub fn buddy_names(&self) -> Vec<String> {
    let mut names: Vec<String> = self.buddies.all().keys().cloned().collect();
    names.sort();
    names
  }

  /// The buddies the server last said are online or away, by name.
  pub fn buddies_online(&self) -> Vec<String> {
    self
      .buddy_names()
      .into_iter()
      .filter(|name| {
        self
          .groups
          .facts(name)
          .and_then(|facts| facts.presence)
          .is_some_and(|presence| presence != UserPresence::Offline)
      })
      .collect()
  }

  /// The groups whose shares `username` sees once their name is confirmed: those of their
  /// manual groups some share is shown to, empty for everyone else, so a member of no such
  /// group costs no confirmation. A request in our own name could come from anyone, so it sees
  /// what everyone sees. [`Directory::sees_from`] confirms the name first.
  pub fn sees(&self, username: &str) -> BTreeSet<String> {
    if self.is_us(username) {
      return BTreeSet::new();
    }
    let options = &self.config.current().values;
    let facts = self.groups.facts(username).unwrap_or_default();
    let buddy = self.buddy(username).is_some();
    let mut groups = groups::place(options, &facts, buddy).share_groups();
    groups.retain(|group| {
      options
        .shares
        .visible_to
        .values()
        .any(|seen_by| seen_by.contains(group))
    });
    groups
  }

  /// Makes `username` our buddy with `change` applied to their settings, or changes the
  /// settings of a buddy; a new buddy is watched at once while we are logged in.
  pub(crate) async fn set_buddy(
    self: &Arc<Self>,
    username: &str,
    change: BuddyChange,
  ) -> Result<(), ApiError> {
    if self.is_us(username) || username == SERVER_USERNAME {
      return Err(ApiError::conflict(
        format!("{username} cannot be a buddy"),
        None,
      ));
    }
    let _changing = self.buddies.changing().await;
    let before = self.buddy(username);
    let buddy = changed(before, change);
    self.change(username, Change::Buddy(Some(buddy))).await?;
    if before.is_none() {
      info!(username, "buddy added");
      self.watch_new_buddy(username);
    }
    Ok(())
  }

  /// Takes `username` off the buddy list. The server goes on telling us their presence until
  /// the session ends, which changes nothing we show.
  pub(crate) async fn remove_buddy(&self, username: &str) -> Result<(), ApiError> {
    let _changing = self.buddies.changing().await;
    self.change(username, Change::Buddy(None)).await?;
    info!(username, "buddy removed");
    Ok(())
  }

  /// Watches a buddy just added, in the background, while we are logged in; otherwise the
  /// next login watches them.
  fn watch_new_buddy(self: &Arc<Self>, username: &str) {
    let Some(client) = self.client.get().cloned() else {
      return;
    };
    if !client.state().is_logged_in() {
      return;
    }
    let (this, username) = (self.clone(), username.to_string());
    self
      .tasks
      .spawn(async move { this.watch(&client, &username).await }.in_current_span());
  }

  /// After a login: every buddy is watched, so the server tells us their presence. One taken
  /// off the list while the others are watched is passed over.
  pub async fn watch_buddies(&self, client: &Client) {
    let names = self.buddy_names();
    debug!(buddies = names.len(), "buddies watched after the login");
    for username in names {
      if self.buddy(&username).is_some() {
        self.watch(client, &username).await;
      }
    }
  }

  /// `username`'s presence moved from `previous` to `presence` on its own: when they are a
  /// buddy we asked to be told about, `BuddyPresenceChanged` is raised and every open page
  /// is told.
  pub fn presence_moved(&self, username: &str, previous: UserPresence, presence: UserPresence) {
    if !self.buddy(username).is_some_and(|b| b.notify_presence) {
      return;
    }
    debug!(username, ?previous, ?presence, "buddy presence changed");
    let changed = BuddyPresenceChanged {
      version: 0,
      username: username.to_string(),
      presence,
      previous,
    };
    self.live.publish(LiveFrame::UsersPresence(changed.clone()));
    self.events.raise(Event::BuddyPresenceChanged(changed));
  }
}
