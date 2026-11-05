//! Whether we are online or away, as other users see us. Away is set by hand, or after nobody
//! has had the UI open for `chat.auto_away`; an away set by hand is kept across restarts and
//! lasts until it is ended by hand, one that came from nobody being at the UI ends when someone
//! opens it. The server is told at once while logged in, else at the next login. While away, the
//! first private message from each user is answered once with `chat.auto_reply`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_types::state::OwnStatus;
use rusqlite::OptionalExtension;
use soulseek_proto::UserPresence;
use tokio::sync::Notify;
use tokio::time::{Instant, sleep_until};
use tracing::{debug, info, warn};

use super::Messaging;
use crate::api::error::ApiError;
use crate::sync::lock;

/// What the reply is sent with, so the one who gets it can tell it was not typed.
const AUTOMATIC: &str = "[Automatic Message] ";

/// What changes our status when nobody asked: the UI being open or not for a while.
const IDLE: &str = "auto-away";

/// What sets the away kept from the last run.
const RESTORED: &str = "restart";

/// Why we are away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Away {
  /// we said so
  ByHand,
  /// nobody had the UI open for a while
  Idle,
}

/// Our status and who was answered while away.
#[derive(Default)]
pub(super) struct Status {
  inner: Mutex<Inner>,
  /// woken when the status is set by hand, after which the wait for nobody at the UI starts
  /// over: an away set by hand may have ended while nobody was there
  set_by_hand: Notify,
}

#[derive(Default)]
struct Inner {
  away: Option<Away>,
  /// the users sent the reply since we went away
  answered: HashSet<String>,
}

impl Messaging {
  /// Takes the away status set by hand that the last run kept.
  pub(super) async fn restore_status(&self) {
    let kept = self
      .db
      .read(|conn| {
        conn
          .query_row("SELECT away FROM own_status WHERE id = 1", [], |row| {
            row.get::<_, bool>(0)
          })
          .optional()
      })
      .await;
    match kept {
      Ok(Some(true)) => {
        self.change_status(Some(Away::ByHand), RESTORED).await;
      }
      Ok(_) => {}
      Err(e) => warn!(error = %e, "kept status not read; starting online"),
    }
  }

  /// Sets us online or away by hand, as `by` asked, which is kept across restarts.
  pub(super) async fn set_status(&self, status: OwnStatus, by: &str) -> Result<(), ApiError> {
    let away = status == OwnStatus::Away;
    self
      .db
      .write(move |conn| {
        conn.execute(
          "INSERT INTO own_status (id, away) VALUES (1, ?1) \
           ON CONFLICT (id) DO UPDATE SET away = excluded.away",
          [away],
        )
      })
      .await
      .map_err(ApiError::internal)?;
    self.change_status(away.then_some(Away::ByHand), by).await;
    self.status.set_by_hand.notify_one();
    Ok(())
  }

  /// Makes `next` our status, for `by`: who asked, or what changed it.
  async fn change_status(&self, next: Option<Away>, by: &str) {
    self.change_status_if(|_| true, next, by).await;
  }

  /// Makes `next` our status, for `by`, when `when` holds for the status as it is, both read and
  /// set in one hold of the lock. Going from online to away or back is shown in the state and
  /// told to the server; coming back forgets who was answered.
  async fn change_status_if(
    &self,
    when: impl FnOnce(Option<Away>) -> bool,
    next: Option<Away>,
    by: &str,
  ) {
    let changed = {
      let mut inner = lock(&self.status.inner);
      let was = inner.away;
      if !when(was) {
        return;
      }
      inner.away = next;
      if next.is_none() {
        inner.answered.clear();
      }
      was.is_some() != next.is_some()
    };
    if !changed {
      return;
    }
    let status = if next.is_some() {
      OwnStatus::Away
    } else {
      OwnStatus::Online
    };
    info!(?status, by, "our status changed");
    self.state.update(|state| state.user.status = status);
    self.tell_status().await;
  }

  /// Tells the server whether we are away, while logged in, and shows us so in the rooms. The
  /// server announces our going away to others but not our coming back, so the rooms are told
  /// here.
  async fn tell_status(&self) {
    if !self.logged_in_now() {
      return;
    }
    let presence = if lock(&self.status.inner).away.is_some() {
      UserPresence::Away
    } else {
      UserPresence::Online
    };
    if let Err(e) = self.client().set_status(presence).await {
      return warn!(error = %e, ?presence, "status not sent to the server");
    }
    debug!(?presence, "status sent to the server");
    if let Some(us) = self.client().username() {
      self.member_status(&us, presence.into());
    }
  }

  /// After a login: the server takes us as online, so an away status is told again.
  pub(super) async fn status_at_login(&self) {
    if lock(&self.status.inner).away.is_some() {
      self.tell_status().await;
    }
  }

  /// The reply `username` gets to the message just received: the configured one, while we are
  /// away and they were not answered since we went away.
  pub(super) fn reply_for(&self, username: &str) -> Option<String> {
    let reply = self.config.current().values.chat.auto_reply.clone()?;
    let reply = reply.trim();
    if reply.is_empty() || self.us().as_deref() == Some(username) {
      return None;
    }
    let mut inner = lock(&self.status.inner);
    if inner.away.is_none() || !inner.answered.insert(username.to_string()) {
      return None;
    }
    Some(format!("{AUTOMATIC}{reply}"))
  }

  /// Sends `username` the reply while we are away, on its own task so the messages after it are
  /// not held up.
  pub(super) fn auto_reply(&self, username: &str, reply: String) {
    let (this, username) = (self.this(), username.to_string());
    self.tasks.spawn(async move {
      match this.send(&username, reply, true).await {
        Ok(()) => info!(username, "away reply sent"),
        Err(e) => warn!(username, error = ?e, "away reply not sent"),
      }
    });
  }

  /// Sets us away once nobody has had the UI open for `chat.auto_away`, and back online when
  /// someone opens it, unless we are away by hand. Runs until the daemon stops.
  pub(super) async fn idle_away(self: Arc<Self>) {
    let mut audience = self.live.audience();
    let mut config = self.config.subscribe();
    let mut after = config.borrow_and_update().values.chat.auto_away;
    let mut people = audience.borrow_and_update().people;
    // when we go away, while nobody is at the UI and `after` is not zero
    let arm = |people: u32, after: Duration| {
      (people == 0 && !after.is_zero()).then(|| Instant::now() + after)
    };
    let mut deadline = arm(people, after);
    loop {
      tokio::select! {
        () = self.shutdown.cancelled() => return,
        changed = audience.changed() => {
          if changed.is_err() {
            return;
          }
          // the feed's followers change the audience too; only people restart the wait
          let now = audience.borrow_and_update().people;
          if now == people {
            continue;
          }
          people = now;
          deadline = arm(people, after);
          if people > 0 {
            self.end_idle_away().await;
          }
        }
        changed = config.changed() => {
          if changed.is_err() {
            return;
          }
          let now = config.borrow_and_update().values.chat.auto_away;
          if now == after {
            continue;
          }
          after = now;
          deadline = arm(people, after);
          if after.is_zero() {
            self.end_idle_away().await;
          }
        }
        () = self.status.set_by_hand.notified() => deadline = arm(people, after),
        () = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
          deadline = None;
          debug!(after = ?after, "nobody has had the UI open");
          self
            .change_status_if(|away| away.is_none(), Some(Away::Idle), IDLE)
            .await;
        }
      }
    }
  }

  /// Ends an away that came from nobody being at the UI; one set by hand stays.
  async fn end_idle_away(&self) {
    self
      .change_status_if(|away| away == Some(Away::Idle), None, IDLE)
      .await;
  }

  /// The connection ended: the next session starts with nobody answered.
  pub(super) fn status_disconnected(&self) {
    lock(&self.status.inner).answered.clear();
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::testing::{TestApp, app, request, with};

  const TRUSTED: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n";

  #[tokio::test]
  async fn the_first_message_from_each_user_while_away_is_answered_once() {
    let config = format!("{TRUSTED}[chat]\nauto_reply = \"back tonight\"\n");
    let app = app(&config, &[]).await;
    let messaging = &app.state.messaging;
    let set = |status: &str| {
      request(
        "PUT",
        "/api/v0/application/status",
        Some(serde_json::json!({ "status": status })),
      )
    };
    assert_eq!(messaging.reply_for("alice"), None, "online");
    let away = app.send_from("127.0.0.1:40000", set("Away")).await;
    assert_eq!(away.status, 204);
    assert_eq!(
      app.state.state.current().user.status,
      api_types::state::OwnStatus::Away
    );
    let reply = Some("[Automatic Message] back tonight".to_string());
    assert_eq!(messaging.reply_for("alice"), reply);
    assert_eq!(messaging.reply_for("alice"), None, "answered already");
    assert_eq!(messaging.reply_for("bob"), reply);
    app.send_from("127.0.0.1:40000", set("Online")).await;
    assert_eq!(messaging.reply_for("alice"), None, "back");
    app.send_from("127.0.0.1:40000", set("Away")).await;
    assert_eq!(messaging.reply_for("alice"), reply, "a new away period");
  }

  #[tokio::test]
  async fn nobody_at_the_ui_sets_away_until_someone_opens_it() {
    let config = format!(
      "{TRUSTED}[web.authentication.api_keys.script]\nkey = \"script-key-0123456789\"\n\
       role = \"readonly\"\n[chat]\nauto_away = \"200ms\"\n"
    );
    let app = app(&config, &[]).await;
    let watch = |from| {
      let opened = request("GET", "/api/v0/live?topics=application", None);
      app.stream_from(from, with(opened, "x-api-key", "script-key-0123456789"))
    };
    // a script's stream is no one at the UI
    let mut script = watch("10.0.0.1:40000").await;
    let status = async |stream: &mut crate::testing::Stream, wanted: &str| loop {
      let frame = stream.until("application.state").await.pop().unwrap();
      if frame.data["user"]["status"] == wanted {
        return;
      }
    };
    status(&mut script, "Away").await;
    let mut person = app
      .stream_from(
        "127.0.0.1:40000",
        request("GET", "/api/v0/live?topics=application", None),
      )
      .await;
    person.until("hello").await;
    status(&mut script, "Online").await;
  }

  #[tokio::test]
  async fn streams_of_scripts_opening_and_closing_do_not_hold_off_away() {
    let config = format!(
      "{TRUSTED}[web.authentication.api_keys.script]\nkey = \"script-key-0123456789\"\n\
       role = \"readonly\"\n[chat]\nauto_away = \"300ms\"\n"
    );
    let app = app(&config, &[]).await;
    let watch = |from| {
      let opened = request("GET", "/api/v0/live?topics=application", None);
      app.stream_from(from, with(opened, "x-api-key", "script-key-0123456789"))
    };
    let mut script = watch("10.0.0.1:40000").await;
    let away = async {
      loop {
        let frame = script.until("application.state").await.pop().unwrap();
        if frame.data["user"]["status"] == "Away" {
          return;
        }
      }
    };
    // another script opens a stream and closes it, again and again, well within the wait
    let churn = async {
      loop {
        drop(watch("10.0.0.2:40000").await);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
      }
    };
    tokio::select! {
      () = away => {}
      () = churn => unreachable!("the churn runs until away"),
    }
  }

  /// A daemon whose away comes after `auto_away` of 300 ms, a stream of a script watching our
  /// status, which counts as nobody at the UI, and a way to set the status by request, which
  /// does not count either.
  async fn watched() -> (TestApp, crate::testing::Stream) {
    let config = format!(
      "{TRUSTED}[web.authentication.api_keys.script]\nkey = \"script-key-0123456789\"\n\
       role = \"readonly\"\n[chat]\nauto_away = \"300ms\"\n"
    );
    let app = app(&config, &[]).await;
    let opened = request("GET", "/api/v0/live?topics=application", None);
    let script = app
      .stream_from(
        "10.0.0.1:40000",
        with(opened, "x-api-key", "script-key-0123456789"),
      )
      .await;
    (app, script)
  }

  async fn shown(stream: &mut crate::testing::Stream, wanted: &str) {
    loop {
      let frame = stream.until("application.state").await.pop().unwrap();
      if frame.data["user"]["status"] == wanted {
        return;
      }
    }
  }

  async fn set(app: &TestApp, status: &str) {
    let body = serde_json::json!({ "status": status });
    let set = request("PUT", "/api/v0/application/status", Some(body));
    assert_eq!(app.send_from("127.0.0.1:40000", set).await.status, 204);
  }

  async fn person(app: &TestApp) -> crate::testing::Stream {
    let opened = request("GET", "/api/v0/live?topics=application", None);
    let mut stream = app.stream_from("127.0.0.1:40000", opened).await;
    stream.until("hello").await;
    stream
  }

  // paused time runs the timers in order: the wait runs out while we are away by hand, before
  // the test sets us online
  #[tokio::test(start_paused = true)]
  async fn ending_an_away_set_by_hand_with_nobody_at_the_ui_waits_again() {
    let (app, mut script) = watched().await;
    // the wait armed at the start runs out during the start's database work
    shown(&mut script, "Away").await;
    let someone = person(&app).await;
    shown(&mut script, "Online").await;
    set(&app, "Away").await;
    shown(&mut script, "Away").await;
    drop(someone);
    tokio::time::sleep(Duration::from_millis(600)).await;
    set(&app, "Online").await;
    shown(&mut script, "Online").await;
    shown(&mut script, "Away").await;
  }
}
