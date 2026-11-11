//! Who is sending us files unasked: how many files each sender is sending, counted by name, or
//! by the address they send from when names are not checked, and the job each sender's files go
//! to while they keep coming.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;
use uuid::Uuid;

use reseek_shares::lock;

/// How soon after a sender's last file their next one must come to join the same job.
pub(super) const GATHERING: Duration = Duration::from_secs(5);

/// The files users are sending us unasked, and the job each sender's files are gathered in.
#[derive(Default)]
pub(in crate::transfers::downloads) struct Senders {
  sending: HashMap<Sender, usize>,
  gathering: HashMap<String, Gathering>,
}

/// Who the files sent unasked are counted against.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Sender {
  /// a sender whose name is checked against their address
  Name(String),
  /// the address a sender sends from, when names are not checked and so are the sender's to pick
  Address(IpAddr),
}

/// The job a sender's files go to while they keep coming.
pub(super) struct Gathering {
  pub job: Uuid,
  /// set once the job is opened, to whether it could be listed, which the files joining it wait
  /// for
  ready: watch::Receiver<Option<bool>>,
  /// every file offered into it, which name it
  pub files: Vec<String>,
  /// how many files are being enqueued into it now
  enqueuing: usize,
  /// when the last file was enqueued into it
  last: Instant,
  /// whether any download was made in it
  pub made: bool,
  /// why each file not enqueued was not
  pub reasons: Vec<String>,
}

/// What joining a sender's job came to.
pub(super) enum Joined {
  /// a new job, which the caller lists and opens, then says on the sender whether it was listed
  Started(Uuid, watch::Sender<Option<bool>>),
  /// the job the sender's files go to already, whose start says whether it was listed
  Gathering(Uuid, watch::Receiver<Option<bool>>),
}

/// One file a user is sending us unasked, counted against the most they may send at once until
/// it is dropped, with its download.
pub(in crate::transfers::downloads) struct Sending {
  senders: Arc<Mutex<Senders>>,
  sender: Sender,
}

impl Senders {
  /// Counts one more file from `sender`; `None`, counting nothing, when they are sending `max`
  /// files already. `0` is no limit.
  pub(super) fn reserve(senders: &Arc<Mutex<Self>>, sender: Sender, max: u32) -> Option<Sending> {
    let mut this = lock(senders);
    let count = this.sending.get(&sender).copied().unwrap_or(0);
    if max != 0 && count >= max as usize {
      return None;
    }
    this.sending.insert(sender.clone(), count + 1);
    Some(Sending {
      senders: senders.clone(),
      sender,
    })
  }

  /// The job the file `filename` from `username` joins: the one their files go to while they
  /// keep coming, or the new job `fresh`.
  pub(super) fn join(&mut self, username: &str, filename: &str, fresh: Uuid) -> Joined {
    if let Some(gathering) = self.gathering.get_mut(username) {
      gathering.enqueuing += 1;
      gathering.files.push(filename.to_string());
      return Joined::Gathering(gathering.job, gathering.ready.clone());
    }
    let (started, ready) = watch::channel(None);
    self.gathering.insert(
      username.to_string(),
      Gathering {
        job: fresh,
        ready,
        files: vec![filename.to_string()],
        enqueuing: 1,
        last: Instant::now(),
        made: false,
        reasons: Vec::new(),
      },
    );
    Joined::Started(fresh, started)
  }

  /// Ends the enqueue of one file of `username` into the job their files go to: whether it
  /// made a download, else why not.
  pub(super) fn joined(&mut self, username: &str, made: Result<(), String>) {
    let gathering = self
      .gathering
      .get_mut(username)
      .expect("a sender's job is gathered until no file is being enqueued into it");
    gathering.enqueuing -= 1;
    gathering.last = Instant::now();
    match made {
      Ok(()) => gathering.made = true,
      Err(reason) => gathering.reasons.push(reason),
    }
  }

  /// Stops gathering the files of `username` into a job once none is being enqueued into it
  /// and the last came [`GATHERING`] ago; until then, how long to wait before asking again.
  pub(super) fn close(&mut self, username: &str, now: Instant) -> Result<Gathering, Duration> {
    let gathering = self
      .gathering
      .get(username)
      .expect("only the task that gathers a sender's files stops gathering them");
    let due = gathering.last + GATHERING;
    if gathering.enqueuing > 0 {
      return Err(GATHERING);
    }
    if now < due {
      return Err(due - now);
    }
    Ok(
      self
        .gathering
        .remove(username)
        .expect("the gathering was just read"),
    )
  }
}

impl Drop for Sending {
  fn drop(&mut self) {
    let mut senders = lock(&self.senders);
    let count = senders
      .sending
      .get_mut(&self.sender)
      .expect("a sender is counted while any of their files is");
    *count -= 1;
    if *count == 0 {
      senders.sending.remove(&self.sender);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_sender_is_held_to_the_files_allowed_at_once() {
    let senders = Arc::new(Mutex::new(Senders::default()));
    let alice = || Sender::Name("alice".to_string());
    let here = || Sender::Address([10, 0, 0, 1].into());
    let first = Senders::reserve(&senders, alice(), 2).unwrap();
    let second = Senders::reserve(&senders, alice(), 2).unwrap();
    assert!(Senders::reserve(&senders, alice(), 2).is_none());
    drop(first);
    let third = Senders::reserve(&senders, alice(), 2).unwrap();
    let from_here = Senders::reserve(&senders, here(), 1).unwrap();
    assert!(Senders::reserve(&senders, here(), 1).is_none());
    assert!(
      Senders::reserve(&senders, here(), 0).is_some(),
      "0 is no limit"
    );
    drop((second, third, from_here));
    assert!(lock(&senders).sending.is_empty(), "nothing counted is left");
  }

  #[test]
  fn a_senders_files_join_one_job_until_none_came_for_a_while() {
    let mut senders = Senders::default();
    let (job, other) = (Uuid::new_v4(), Uuid::new_v4());
    assert!(matches!(senders.join("alice", "a", job), Joined::Started(id, _) if id == job));
    assert!(matches!(senders.join("alice", "b", other), Joined::Gathering(id, _) if id == job));
    senders.joined("alice", Ok(()));
    assert_eq!(
      senders.close("alice", Instant::now()).err(),
      Some(GATHERING)
    );
    senders.joined("alice", Err("Skipped".into()));
    let later = Instant::now() + GATHERING;
    let gathered = senders.close("alice", later).ok().unwrap();
    assert_eq!((gathered.job, gathered.files.len()), (job, 2));
    assert!(gathered.made);
    assert_eq!(gathered.reasons, ["Skipped"]);
    assert!(matches!(senders.join("alice", "c", other), Joined::Started(id, _) if id == other));
  }
}
