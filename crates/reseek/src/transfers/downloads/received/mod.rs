//! Files other users send us without being asked. The client holds such an offer while we decide:
//! the file is taken when its sender is in one of the groups `transfers.download.received` lets
//! send files, the server asked about them first when its word could put them in one, under a name
//! their address confirms unless everyone may, the file is no larger than allowed and not one the
//! download filters skip, the sender is sending fewer files than allowed (counted by the address
//! they send from when their name is not checked), no download of the file from them is unfinished
//! and none was canceled within the hour. A file taken is downloaded like any other, but always at
//! the size it was offered with and never in place of a file. The files one sender sends close
//! together are one job they started, whose downloads' first tries take the offers; a file whose
//! job cannot be listed is refused, since the job is what marks its download as sent unasked.

mod senders;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use api_types::jobs::{JobKind, JobStarter, JobStarterKind, JobState};
use api_types::transfers::TransferDirection;
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use soulseek_client::ClientError;
use tokio::time::Instant;
use tracing::{Instrument, info, warn};
use uuid::Uuid;

use super::destination::local_name;
use super::enqueue::{check, download_filters};
use super::jobs::job_name;
use super::{Downloads, FileRequest, Group};
use crate::config::{DEFAULT_GROUP, Received};
use crate::db::timestamp;
use crate::jobs::NewJob;
use crate::safety::Os;
use crate::transfers::categories::{CANCELLED, UNFINISHED};
use crate::transfers::{NEWEST_FIRST, Record, direction_text};
use reseek_shares::lock;
use senders::{Joined, Sender};
pub(super) use senders::{Senders, Sending};

/// How long after a download from a user was canceled their offer of the same file is taken as
/// them catching up with the cancel, and refused.
const CANCEL_HEARD: Duration = Duration::from_secs(3600);

/// Why a file a user offers unasked is refused by who may send files and how large, `None`
/// when those let it in: its sender's groups, `in_group` telling of each, and its `size`.
fn refusal(
  received: &Received,
  in_group: impl Fn(&str) -> bool,
  size: u64,
) -> Option<&'static str> {
  if !received.from.iter().any(|group| in_group(group)) {
    return Some("the sender is in no group that may send files");
  }
  if received.max_size.is_some_and(|max| size > max.as_u64()) {
    return Some("the file is larger than allowed");
  }
  None
}

/// Who the files `username` sends from `from` are counted against: their name, or when
/// `everyone` may send, the address, since then the name is not checked and is theirs to pick.
fn counted_as(everyone: bool, username: &str, from: IpAddr) -> Sender {
  if everyone {
    Sender::Address(from)
  } else {
    Sender::Name(username.to_string())
  }
}

impl Downloads {
  /// Takes the file `filename` of `size` bytes that `username`, connected from `from`, offers
  /// to send unasked, as a download in the job their files go to; refused with
  /// [`ClientError::TransferRejected`] and the reason when the options do not let it in. The
  /// client logs the refusal.
  pub async fn receive(
    &self,
    username: &str,
    from: IpAddr,
    filename: &str,
    size: u64,
  ) -> Result<(), ClientError> {
    let refused = |reason: &str| Err(ClientError::TransferRejected(reason.to_string()));
    let file = FileRequest {
      filename: filename.to_string(),
      size,
      position: None,
    };
    // the names any enqueue refuses, and one that makes no file name, before anything is
    // counted or made
    if let Err(e) = check(username, std::slice::from_ref(&file)) {
      return refused(e.message());
    }
    if filename.trim().is_empty() {
      return refused("the file name is only whitespace");
    }
    if let Err(reason) = local_name(filename, Os::CURRENT) {
      return refused(&reason);
    }
    let options = self.config.current();
    let received = &options.values.transfers.download.received;
    let placement = self
      .users
      .placement_letting_in(self.network.client(), username, &received.from)
      .await;
    if let Some(reason) = refusal(received, |group| placement.is_in(group), size) {
      return refused(reason);
    }
    let filters = download_filters(&options.values);
    if filters.iter().any(|filter| filter.is_match(filename)) {
      return refused("the download filters skip the file");
    }
    // a name that is not checked is the sender's to pick, so their address is counted instead
    let everyone = received.from.iter().any(|group| group == DEFAULT_GROUP);
    let sender = counted_as(everyone, username, from);
    let Some(sending) = Senders::reserve(&self.senders, sender, received.max_files) else {
      return refused("the sender is sending as many files as allowed");
    };
    if !everyone && !self.users.confirms(username, from).await {
      return refused("the sender's address does not confirm their name");
    }
    if let Some(reason) = self.downloaded_before(username, filename).await? {
      return refused(reason);
    }
    let group = Group {
      username: username.to_string(),
      files: vec![file],
      destination: None,
      folder: None,
      search_id: None,
    };
    // the rest runs on a task of its own, so a sender who stops waiting leaves no record
    // without its task and no job open
    let this = self.this();
    let ran = self
      .tasks
      .spawn(async move { this.take_sent(group, sending).await }.in_current_span());
    match ran.await {
      Ok(taken) => taken,
      Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
      Err(e) => Err(ClientError::client(e.to_string(), None)),
    }
  }

  /// Whether the download `record` is of a file its sender sent unasked, who chose its name and
  /// size; taken as so when its job cannot be read.
  pub(super) async fn pushed(&self, record: &Record) -> bool {
    let Some(job) = record.job_id else {
      return false;
    };
    match self.jobs.get(job).await {
      Ok(listed) => listed.is_some_and(|listed| listed.starter.kind == JobStarterKind::User),
      Err(e) => {
        warn!(%job, error = %e, "download job not read; its file is taken as sent unasked");
        true
      }
    }
  }

  /// Why the file `filename` that `username` sends unasked is refused for its download from
  /// them: one not finished, such as one paused or waiting to retry, or one canceled within
  /// [`CANCEL_HEARD`], which the offer is the peer catching up with. Records removed from the
  /// list do not count.
  async fn downloaded_before(
    &self,
    username: &str,
    filename: &str,
  ) -> Result<Option<&'static str>, ClientError> {
    let since = timestamp(Utc::now() - CANCEL_HEARD);
    let (name, file) = (username.to_string(), filename.to_string());
    let latest = self
      .db
      .read(move |conn| {
        conn
          .prepare_cached(&format!(
            "SELECT {UNFINISHED}, {CANCELLED} AND EndedAt > ?4 FROM Transfers \
             WHERE Username = ?1 AND Direction = ?2 AND Filename = ?3 AND Removed = 0 \
             ORDER BY {NEWEST_FIRST} LIMIT 1"
          ))?
          .query_row(
            params![
              name,
              direction_text(TransferDirection::Download),
              file,
              since
            ],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
          )
          .optional()
      })
      .await
      .map_err(|e| ClientError::client(format!("downloads not read: {e}"), None))?;
    Ok(match latest {
      Some((true, _)) => Some("a download of the file from the sender is not finished"),
      Some((_, true)) => Some("its download from the sender was canceled"),
      _ => None,
    })
  }

  /// Enqueues the one file of `group` that its user is sending unasked into the job their files
  /// go to, starting one when none gathers them.
  async fn take_sent(self: Arc<Self>, group: Group, sending: Sending) -> Result<(), ClientError> {
    let username = group.username.clone();
    let filename = group.files[0].filename.clone();
    let joined = lock(&self.senders).join(&username, &filename, Uuid::new_v4());
    let (job, listed) = match joined {
      Joined::Started(job, started) => {
        let listed = self.start_gathering(job, &group).await;
        started.send_replace(Some(listed));
        let this = self.clone();
        let name = username.clone();
        self
          .tasks
          .spawn(async move { this.gather(name, job).await }.in_current_span());
        (job, listed)
      }
      Joined::Gathering(job, mut ready) => {
        // an error means the start was dropped halfway, before it listed the job
        let listed = ready
          .wait_for(Option::is_some)
          .await
          .is_ok_and(|listed| *listed == Some(true));
        (job, listed)
      }
    };
    if !listed {
      let reason = "its job could not be listed";
      lock(&self.senders).joined(&username, Err(reason.to_string()));
      return Err(ClientError::client(reason.to_string(), None));
    }
    let result = self.enqueue_group(job, group, &[], Some(sending)).await;
    let made = match &result {
      Ok(enqueued) if !enqueued.enqueued.is_empty() => Ok(()),
      Ok(enqueued) => Err(
        enqueued
          .failed
          .first()
          .map_or("nothing was queued", |f| f.message.as_str())
          .to_string(),
      ),
      Err(e) => Err(e.message().to_string()),
    };
    lock(&self.senders).joined(&username, made.clone());
    match (result, made) {
      (_, Ok(())) => {
        info!(username, filename, %job, "file sent unasked queued");
        Ok(())
      }
      (Ok(_), Err(reason)) => Err(ClientError::TransferRejected(reason)),
      (Err(_), Err(reason)) => Err(ClientError::client(reason, None)),
    }
  }

  /// Lists the job `job` of the files the user of `group` sends unasked, named after its first,
  /// and opens it; false when it could not be listed.
  async fn start_gathering(&self, job: Uuid, group: &Group) -> bool {
    let new = NewJob {
      id: job,
      kind: JobKind::Download,
      name: job_name(std::slice::from_ref(group)),
      starter: JobStarter::user(&group.username),
      state: JobState::Running,
      follows: None,
      data: None,
    };
    self.open_job(job);
    match self.jobs.create(new).await {
      Ok(_) => true,
      Err(e) => {
        warn!(%job, error = %e, "download job of files sent unasked not listed; its files are refused");
        false
      }
    }
  }

  /// Gathers the files `username` sends unasked into the job `job` until none came for a while,
  /// then closes it, named after all its files, and ends it failed when none was queued.
  async fn gather(self: Arc<Self>, username: String, job: Uuid) {
    let gathered = loop {
      let wait = match lock(&self.senders).close(&username, Instant::now()) {
        Ok(gathered) => break gathered,
        Err(wait) => wait,
      };
      tokio::select! {
        () = tokio::time::sleep(wait) => {}
        // the stop ends the downloads; the job is brought in step at the next start
        () = self.shutdown.cancelled() => return,
      }
    };
    if gathered.files.len() > 1 {
      let group = Group {
        username,
        files: gathered
          .files
          .iter()
          .map(|filename| FileRequest {
            filename: filename.clone(),
            size: 0,
            position: None,
          })
          .collect(),
        destination: None,
        folder: None,
        search_id: None,
      };
      let name = job_name(&[group]);
      if let Err(e) = self
        .jobs
        .update(job, move |listed| listed.name = name)
        .await
      {
        warn!(%job, error = %e, "download job of files sent unasked not renamed");
      }
    }
    self.close_job(job).await;
    if !gathered.made {
      self.end_unqueued(job, &gathered.reasons).await;
    }
  }
}

#[cfg(test)]
mod tests;
