//! The user's scripts: every raised event runs each script whose `on` names its type. A script
//! runs once at a time, in the order its events were raised, from a bounded queue that drops the
//! oldest waiting run when it is full, so a burst of events never piles up processes and a
//! script never holds up an event. Every run is recorded as `ScriptRunCompleted`, which itself
//! runs no script.

// on Windows a program splits its argument string itself
#[cfg(not(windows))]
mod args;
mod output;
mod run;

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use api_types::events::{Event, EventRecord, EventType};
use cascade::Watcher;
use soulseek_client::LogLimiter;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, info_span, warn};

pub use run::Shell;

use crate::config::{Options, ScriptRun};
use crate::events::EventBus;
use crate::sync::lock;
use run::{Context, Job};

/// The names in `on` that match every event a script can run for.
pub const WILDCARDS: [&str; 3] = ["Any", "All", "*"];

/// How many runs of one script may wait their turn; past it the oldest waiting is dropped.
const WAITING: usize = 256;

/// The shell a script's command runs in: `cmd.exe` on Windows, else the program `shell` names,
/// the daemon's `$SHELL`. Logs which, or that there is none.
pub fn shell(shell: Option<OsString>) -> Option<Shell> {
  let found = if cfg!(windows) {
    Some(Shell {
      program: "cmd.exe".to_string(),
      prefix: "/c",
    })
  } else {
    shell
      .map(|program| program.to_string_lossy().trim().to_string())
      .filter(|program| !program.is_empty())
      .map(|program| Shell {
        program,
        prefix: "-c",
      })
  };
  match &found {
    Some(shell) => info!(executable = shell.program, "Set default script executable"),
    None => warn!(
      "Unable to determine default script executable ($SHELL is missing or blank); any \
       user-defined scripts that do not specify an executable will fail"
    ),
  }
  found
}

/// Whether a script whose `on` is `on` runs for an event of type `kind`.
fn runs_on(on: &[String], kind: EventType) -> bool {
  kind != EventType::ScriptRunCompleted
    && on.iter().any(|name| {
      name.eq_ignore_ascii_case(kind.name())
        || WILDCARDS
          .iter()
          .any(|wildcard| name.eq_ignore_ascii_case(wildcard))
    })
}

/// The scripts of the options in effect, run for the events raised on the bus.
pub struct Scripts {
  config: Watcher<Options>,
  events: EventBus,
  context: Arc<Context>,
  /// each script's runs waiting their turn, by name
  queues: Mutex<HashMap<String, Arc<Queue>>>,
  /// the scripts told once that an argument list is the safer form
  advised: Mutex<HashSet<String>>,
  shutdown: CancellationToken,
}

impl Scripts {
  /// Runs, in `directory`, the scripts `config` holds when each event is raised on `events`,
  /// with `shell` for the scripts that name no program, until `shutdown`, which kills the runs
  /// under way.
  pub fn start(
    config: Watcher<Options>,
    events: &EventBus,
    directory: PathBuf,
    shell: Option<Shell>,
    shutdown: CancellationToken,
  ) {
    let scripts = Arc::new(Self {
      config,
      events: events.clone(),
      context: Arc::new(Context { directory, shell }),
      queues: Mutex::new(HashMap::new()),
      advised: Mutex::new(HashSet::new()),
      shutdown,
    });
    events.subscribe("scripts", None, move |record| {
      scripts.take(record);
      async { Ok(()) }
    });
  }

  /// Queues a run of every script `record` is for. A script no longer in the options gets no
  /// more runs; those it has waiting still run.
  fn take(&self, record: Arc<EventRecord>) {
    let kind = record.event.event_type();
    let options = self.config.current();
    let scripts = &options.values.integrations.scripts;
    let mut queues = lock(&self.queues);
    queues.retain(|name, queue| {
      let kept = scripts.contains_key(name);
      if !kept {
        queue.close();
      }
      kept
    });
    for (name, script) in scripts {
      if !runs_on(&script.on, kind) {
        continue;
      }
      self.advise(name, &script.run);
      let queue = queues
        .entry(name.clone())
        .or_insert_with(|| self.queue(name));
      queue.push(Job {
        name: name.clone(),
        script: script.clone(),
        record: record.clone(),
      });
    }
  }

  /// A new queue for the script `name`, with the task that runs what it holds.
  fn queue(&self, name: &str) -> Arc<Queue> {
    let queue = Arc::new(Queue::default());
    tokio::spawn(
      work(
        queue.clone(),
        self.context.clone(),
        self.events.clone(),
        self.shutdown.clone(),
      )
      .instrument(info_span!("script", name)),
    );
    queue
  }

  /// Says once per script that one passing its arguments as a string would be better off with
  /// a list.
  fn advise(&self, name: &str, run: &ScriptRun) {
    if run.arglist.is_some() || !lock(&self.advised).insert(name.to_string()) {
      return;
    }
    info!(
      script = name,
      "the script's arguments are one string split by its quotes; `run.arglist` passes each \
       argument as it is"
    );
  }
}

/// One script's runs waiting their turn.
#[derive(Default)]
struct Queue {
  jobs: Mutex<VecDeque<Job>>,
  /// a run was added or the queue closed
  ready: Notify,
  /// the script left the options; its task ends once the queue is empty
  closed: AtomicBool,
  /// runs dropped because too many waited
  dropped: AtomicU64,
  crowded: LogLimiter,
}

impl Queue {
  /// Adds a run, dropping the oldest waiting one when too many wait.
  fn push(&self, job: Job) {
    let mut jobs = lock(&self.jobs);
    if jobs.len() >= WAITING
      && let Some(oldest) = jobs.pop_front()
    {
      let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
      if let Some(suppressed) = self.crowded.allow() {
        warn!(
          script = oldest.name,
          event = oldest.record.event.event_type().name(),
          waiting = WAITING,
          dropped,
          suppressed,
          "too many script runs waiting; the oldest dropped"
        );
      }
    }
    jobs.push_back(job);
    drop(jobs);
    self.ready.notify_one();
  }

  fn close(&self) {
    self.closed.store(true, Ordering::Release);
    self.ready.notify_one();
  }
}

/// Runs what `queue` holds, one run at a time, raising `ScriptRunCompleted` after each, until
/// the queue is closed and empty or the daemon stops.
async fn work(
  queue: Arc<Queue>,
  context: Arc<Context>,
  events: EventBus,
  shutdown: CancellationToken,
) {
  while !shutdown.is_cancelled() {
    let job = lock(&queue.jobs).pop_front();
    match job {
      Some(job) => {
        let completed = run::run(&job, &context, &shutdown).await;
        events.raise(Event::ScriptRunCompleted(completed));
      }
      None if queue.closed.load(Ordering::Acquire) => return,
      None => tokio::select! {
        () = queue.ready.notified() => {}
        () = shutdown.cancelled() => return,
      },
    }
  }
}

#[cfg(test)]
mod tests;
