//! One upload's task: the user is watched, the file found again, and the client's upload run
//! with the queue's slot, the group's speed limit and every state written as it comes, the
//! progress at most every [`PROGRESS_EVERY`]. How it ended is written, counted and, for a
//! success, raised as `UploadFileComplete`; whatever the end, the upload leaves the queue. A
//! stop of the daemon leaves the record as it is, for the stop to end.

use std::sync::Arc;
use std::time::Duration;

use api_types::events::{Event, UploadFileComplete};
use api_types::live::LiveFrame;
use api_types::relay::RelayUploadOutcome;
use api_types::transfers::{TransferDirection, TransferOutcome, TransferStates};
use chrono::Utc;
use reseek_shares::lock;
use soulseek_client::{
  ClientError, InputFactory, LogLimiter, ProgressUpdated, SlotAwaiter, SlotReleased, StateChanged,
  Transfer as Snapshot, TransferOptions, UploadInput, UploadRequest, UploadSource,
};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, trace, warn};
use uuid::Uuid;

use super::Uploads;
use super::intake::Located;
use crate::relay::Controller;
use crate::shares::LOCAL_HOST;
use crate::transfers::state::{self, State};
use crate::transfers::{Record, removal, store};

/// More states than one upload reports.
const STATES: usize = 16;

/// How often an upload's progress is written and pushed at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Why an upload whose file is gone when it starts ended.
const FILE_GONE: &str = "File could not be found";

/// Progress not written, which a failing database would repeat four times a second.
static PROGRESS_UNWRITTEN: LogLimiter = LogLimiter::new();

/// The I/O error from the input that `error` ended with, when it ended with one: a relay agent's
/// file that is not there, or did not come in time, fails its upload as such.
fn input_error(error: &ClientError) -> Option<&std::io::Error> {
  match error {
    ClientError::Io {
      source: Some(source),
      ..
    } => source.downcast_ref::<std::io::Error>(),
    _ => None,
  }
}

/// How an upload that ended with `error` ended.
fn outcome(error: &ClientError) -> TransferOutcome {
  match error {
    ClientError::Canceled => TransferOutcome::Cancelled,
    ClientError::Timeout(_) => TransferOutcome::TimedOut,
    ClientError::TransferRejected(_) => TransferOutcome::Rejected,
    ClientError::NotFound(_) => TransferOutcome::Aborted,
    other => match input_error(other).map(std::io::Error::kind) {
      Some(std::io::ErrorKind::NotFound) => TransferOutcome::Aborted,
      Some(std::io::ErrorKind::TimedOut) => TransferOutcome::TimedOut,
      _ => TransferOutcome::Errored,
    },
  }
}

/// Why an upload that ended with `error` failed, as its record shows it: the peer's refusal as
/// it sent it, why a relay agent's file did not come, else the error's text.
fn reason(error: &ClientError) -> String {
  match error {
    ClientError::TransferRejected(reason) | ClientError::NotFound(reason) => reason.clone(),
    other => match input_error(other) {
      Some(input) => input.to_string(),
      None => other.to_string(),
    },
  }
}

/// How the relay controller tells the agent an upload of its file ended.
fn relayed(result: &Result<Snapshot, ClientError>, record: &Record) -> (RelayUploadOutcome, u64) {
  match result {
    Ok(transfer) => (RelayUploadOutcome::Succeeded, transfer.bytes_transferred),
    Err(ClientError::Canceled) => (RelayUploadOutcome::Cancelled, record.bytes_transferred),
    Err(_) => (RelayUploadOutcome::Failed, record.bytes_transferred),
  }
}

impl Uploads {
  /// Starts the task of the upload `record` of the file `located`, which stops when `cancel`
  /// is canceled.
  pub(super) fn spawn(&self, record: Record, located: Located, cancel: CancellationToken) {
    let this = self.this();
    let span = info_span!(
      "transfer",
      id = %record.id,
      direction = "upload",
      username = record.username
    );
    self.tasks.spawn(
      async move {
        let id = record.id;
        this.run(record, located, &cancel).await;
        this.forget(id).await;
      }
      .instrument(span),
    );
  }

  /// The upload from its start to how it ended, written.
  async fn run(&self, mut record: Record, located: Located, cancel: &CancellationToken) {
    let client = match self.client() {
      Ok(client) => client,
      Err(e) => return self.fail(&mut record, &e, None).await,
    };
    self.users.watch(&client, &record.username).await;
    info!(filename = record.filename, "Initializing upload");
    // the request was confirmed when it was queued, and the file goes to the user the server
    // knows by the name; whether they still see the file is decided again
    let sees = self.users.sees(&record.username);
    let located = match self.locate(&sees, &record.filename).await {
      Ok(Some(found)) => found,
      Ok(None) => {
        let gone = ClientError::NotFound(FILE_GONE.to_string());
        return self.fail(&mut record, &gone, None).await;
      }
      Err(e) => {
        warn!(error = %e, "share index not read; the file found when the request was accepted is sent");
        located
      }
    };
    let (states_sent, mut states) = mpsc::channel(STATES);
    let ending = self.this();
    let id = record.id;
    let state_changed: StateChanged = Arc::new(move |_, snapshot: &Snapshot| {
      // marked before the client tells the peer, which may ask again at once
      if snapshot.state.is_terminal()
        && let Some(entry) = lock(&ending.in_flight).get_mut(&id)
      {
        entry.ended = true;
      }
      if let Err(e) = states_sent.try_send(snapshot.clone()) {
        warn!(error = %e, "upload state not recorded");
      }
    });
    let (progress_sent, mut progress) = watch::channel((0u64, 0.0f64));
    let progress_updated: ProgressUpdated = Arc::new(move |_, snapshot: &Snapshot| {
      progress_sent.send_replace((snapshot.bytes_transferred, snapshot.average_speed));
    });
    let username = record.username.clone();
    let users = self.users.clone();
    let (governor, reporter) = self
      .governor
      .pace(Arc::new(move || users.speed_group(&username)));
    let (input, relay) = self.input(&record, &located);
    let request = UploadRequest {
      username: record.username.clone(),
      filename: record.filename.clone().into(),
      input,
      token: None,
      options: TransferOptions {
        // an agent sends its file from the offset already
        seek_input_stream_automatically: relay.is_none(),
        governor: Some(governor),
        reporter: Some(reporter),
        state_changed: Some(state_changed),
        progress_updated: Some(progress_updated),
        slot_awaiter: Some(self.slot_awaiter(record.id)),
        slot_released: Some(self.slot_released(record.id)),
        ..TransferOptions::default()
      },
    };
    let running = client.upload(request, cancel);
    tokio::pin!(running);
    let throttle = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(throttle);
    let mut throttled = false;
    let mut ended: Option<(Snapshot, TransferOutcome)> = None;
    let result = loop {
      tokio::select! {
        biased;
        Some(snapshot) = states.recv() => self.state_changed(&mut record, snapshot, &mut ended).await,
        result = &mut running => break result,
        Ok(()) = progress.changed(), if !throttled => {
          let (bytes, speed) = *progress.borrow_and_update();
          self.progressed(&mut record, bytes, speed).await;
          throttled = true;
          throttle.as_mut().reset(Instant::now() + PROGRESS_EVERY);
        }
        () = &mut throttle, if throttled => throttled = false,
      }
    };
    // the states the client reported before its task ended are all queued by now
    while let Ok(snapshot) = states.try_recv() {
      self.state_changed(&mut record, snapshot, &mut ended).await;
    }
    if let Some(relay) = relay {
      let (outcome, bytes) = relayed(&result, &record);
      relay.finished(record.id, outcome, bytes);
    }
    if self.shutdown.is_cancelled() {
      return debug!("upload left for the stop to end");
    }
    match result {
      Ok(transfer) => self.succeeded(&mut record, &transfer, &located).await,
      Err(error) => self.fail(&mut record, &error, ended.as_ref()).await,
    }
  }

  /// What the upload of `record` reads: the local file, or the bytes the relay agent holding it
  /// sends from the offset the peer asks for, with the controller then.
  fn input(&self, record: &Record, located: &Located) -> (UploadInput, Option<Arc<Controller>>) {
    if located.resolved.host == LOCAL_HOST {
      return (UploadInput::File(located.resolved.path.clone()), None);
    }
    let relay = self
      .relay
      .clone()
      .expect("only a relay controller has agents' shares");
    let (agent, filename, transfer) = (
      located.resolved.host.clone(),
      record.filename.clone(),
      record.id,
    );
    let opening = relay.clone();
    let open: InputFactory = Box::new(move |start| {
      Box::pin(async move {
        let stream = opening
          .open_stream(&agent, &filename, start, transfer)
          .await?;
        Ok(Box::new(stream) as Box<dyn UploadSource>)
      })
    });
    let input = UploadInput::Stream {
      size: located.size,
      open,
    };
    (input, Some(relay))
  }

  /// What the client waits on for the upload `id`'s slot: the queue's start, or the cancel.
  fn slot_awaiter(&self, id: Uuid) -> SlotAwaiter {
    let this = self.this();
    Arc::new(move |_, cancel: CancellationToken| {
      let start = this.queue.await_start(id);
      Box::pin(async move {
        let start = start.map_err(|e| ClientError::client(e.to_string(), None))?;
        tokio::select! {
          biased;
          () = cancel.cancelled() => Err(ClientError::Canceled),
          started = start => started.map_err(|_| {
            ClientError::client(format!("upload {id} left the queue before its slot"), None)
          }),
        }
      })
    })
  }

  /// What the client calls once the upload `id` lets go of its slot.
  fn slot_released(&self, id: Uuid) -> SlotReleased {
    let this = self.this();
    Arc::new(move |_| {
      this.queue.complete(id);
    })
  }

  /// Takes a state the client reported into `record` and writes it; a final state is kept in
  /// `ended` with its outcome for the task to write.
  async fn state_changed(
    &self,
    record: &mut Record,
    snapshot: Snapshot,
    ended: &mut Option<(Snapshot, TransferOutcome)>,
  ) {
    let state = state::reported(snapshot.state);
    if let TransferStates::Completed(outcome) = state {
      *ended = Some((snapshot, outcome));
      return;
    }
    // records are left as they are once the daemon stops
    if self.shutdown.is_cancelled() {
      return;
    }
    let previous = record.state.kind();
    if let Err(refused) = record.enter(State::of(state)) {
      return warn!(%refused, "upload state not recorded");
    }
    record.take_figures(&snapshot);
    if state == TransferStates::Initializing && previous != state {
      // the server never pushes what a user shares, and their group depends on it
      self.users.refetch_statistics(&record.username);
    }
    if state == TransferStates::InProgress && previous != state {
      info!(
        filename = record.filename,
        start = snapshot.start_offset,
        size = record.size,
        "upload started"
      );
    }
    self.save(record, previous != state).await;
  }

  /// Writes and pushes the bytes sent and the speed of an upload in progress.
  async fn progressed(&self, record: &mut Record, bytes: u64, speed: f64) {
    if record.state != State::InProgress || self.shutdown.is_cancelled() {
      return;
    }
    record.bytes_transferred = bytes;
    record.average_speed = speed;
    let id = record.id;
    match self
      .db
      .write(move |conn| store::progress(conn, id, bytes, speed))
      .await
    {
      Ok(true) => {
        trace!(bytes, speed, "upload progress");
        self
          .live
          .publish(LiveFrame::TransfersUpdated(Box::new(record.transfer())));
      }
      // the record left `InProgress` in the database meanwhile
      Ok(false) => {}
      Err(e) => {
        if let Some(suppressed) = PROGRESS_UNWRITTEN.allow() {
          warn!(error = %e, suppressed, "upload progress not written");
        }
      }
    }
  }

  /// Writes an upload that succeeded, counts it and raises `UploadFileComplete`.
  async fn succeeded(&self, record: &mut Record, transfer: &Snapshot, located: &Located) {
    record.take_figures(transfer);
    if let Err(refused) = record.enter(State::Completed(TransferOutcome::Succeeded)) {
      return debug!(%refused, "upload ended already");
    }
    self.save(record, true).await;
    self
      .telemetry
      .metrics()
      .transfers()
      .uploads
      .succeeded(record.bytes_transferred, record.average_speed);
    info!(
      filename = record.filename,
      bytes = record.bytes_transferred,
      speed = record.average_speed,
      "upload completed"
    );
    let ended_at = record.ended_at.unwrap_or_else(Utc::now);
    self.events.raise_at(
      Event::UploadFileComplete(UploadFileComplete {
        version: 0,
        local_filename: located.resolved.path.display().to_string(),
        remote_filename: record.filename.clone(),
        transfer: record.transfer(),
      }),
      ended_at,
    );
  }

  /// Writes an upload that ended with `error` as completed with its outcome, unless its record
  /// ended already, and counts it as failed. The client's last snapshot, `ended`, gives the
  /// outcome and the times when there is one; without, the upload never started.
  async fn fail(
    &self,
    record: &mut Record,
    error: &ClientError,
    ended: Option<&(Snapshot, TransferOutcome)>,
  ) {
    let canceled_for = lock(&self.in_flight)
      .get(&record.id)
      .and_then(|entry| entry.reason);
    let outcome = ended.map_or_else(|| outcome(error), |(_, outcome)| *outcome);
    let exception = match canceled_for {
      Some(why) if outcome == TransferOutcome::Cancelled => why.to_string(),
      _ => reason(error),
    };
    if let Err(refused) = record.complete(outcome, Some(exception)) {
      return debug!(%refused, "upload ended already");
    }
    if let Some((snapshot, _)) = ended {
      record.take_figures(snapshot);
    }
    self.save(record, true).await;
    self.telemetry.metrics().transfers().uploads.failed();
    info!(
      filename = record.filename,
      state = %record.state,
      reason = record.exception,
      "upload failed"
    );
  }

  /// Lets go of the upload `id` once its task has written how it ended: it leaves the queue if
  /// it has not, its states are logged, and it is removed from the listings when that was
  /// asked for meanwhile.
  async fn forget(&self, id: Uuid) {
    if self.queue.complete(id) {
      debug!("upload taken out of the queue at its end");
    }
    let entry = lock(&self.in_flight).remove(&id);
    match self.db.read(move |conn| store::history(conn, id)).await {
      Ok(history) => {
        let states: Vec<String> = history.iter().map(|c| c.state.to_string()).collect();
        debug!(?states, "upload states");
      }
      Err(e) => debug!(error = %e, "upload states not read"),
    }
    if entry.is_some_and(|entry| entry.remove)
      && let Err(e) = removal::remove(&self.db, &self.live, TransferDirection::Upload, id).await
    {
      warn!(%id, error = %e, "ended upload not removed");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_input_whose_file_is_gone_or_late_aborts_or_times_out_its_upload() {
    use std::io::{Error, ErrorKind};
    let gone = ClientError::io("read", Error::new(ErrorKind::NotFound, "no such file"));
    assert_eq!(outcome(&gone), TransferOutcome::Aborted);
    assert_eq!(reason(&gone), "no such file");
    let late = ClientError::io("read", Error::new(ErrorKind::TimedOut, "too slow"));
    assert_eq!(outcome(&late), TransferOutcome::TimedOut);
    let other = ClientError::io("read", Error::other("disk"));
    assert_eq!(outcome(&other), TransferOutcome::Errored);
  }
}
