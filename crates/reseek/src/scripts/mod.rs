//! The user's scripts: every raised event runs each script whose `on` names its type. A script
//! runs once at a time, in the order its events were raised, from a bounded queue that drops the
//! oldest waiting run when it is full, so a burst of events never piles up processes and a
//! script never holds up an event. Every run is recorded as `ScriptRunCompleted`, which itself
//! runs no script.

// on Windows a program splits its argument string itself
#[cfg(not(windows))]
mod args;
mod group;
mod output;
mod run;

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use api_types::events::{Event, EventRecord, EventType};
use cascade::Watcher;
use soulseek_client::LogLimiter;
use tokio::sync::Notify;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
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
  /// the queues' tasks and the one following the options
  tasks: TaskTracker,
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
  ) -> Arc<Self> {
    let scripts = Arc::new(Self {
      config,
      events: events.clone(),
      context: Arc::new(Context { directory, shell }),
      queues: Mutex::new(HashMap::new()),
      advised: Mutex::new(HashSet::new()),
      tasks: TaskTracker::new(),
      shutdown,
    });
    let taker = scripts.clone();
    events.subscribe("scripts", None, move |record| {
      taker.take(record);
      async { Ok(()) }
    });
    scripts
      .tasks
      .spawn(follow_options(scripts.clone()).instrument(info_span!("script_options")));
    scripts
  }

  /// Waits, at shutdown, until every script's task has ended: a run under way is killed by the
  /// shutdown, and its `ScriptRunCompleted` raised before its task ends.
  pub async fn stop(&self) {
    self.tasks.close();
    self.tasks.wait().await;
  }

  /// Closes the queues of the scripts no longer in the options, so their tasks end once what
  /// they hold has run, and forgets the queues whose tasks have ended. A queue is kept until its
  /// task ends, so a script added back meanwhile still runs one at a time.
  fn close_removed(&self, queues: &mut HashMap<String, Arc<Queue>>) {
    let options = self.config.current();
    let scripts = &options.values.integrations.scripts;
    queues.retain(|name, queue| {
      if !scripts.contains_key(name) {
        queue.close();
      }
      !queue.ended()
    });
  }

  /// Queues a run of every script `record` is for.
  fn take(&self, record: Arc<EventRecord>) {
    let kind = record.event.event_type();
    let options = self.config.current();
    let scripts = &options.values.integrations.scripts;
    let mut queues = lock(&self.queues);
    self.close_removed(&mut queues);
    for (name, script) in scripts {
      if !runs_on(&script.on, kind) {
        continue;
      }
      self.advise(name, &script.run);
      let job = Job {
        name: name.clone(),
        script: Box::new(script.clone()),
        record: record.clone(),
      };
      let refused = match queues.get(name) {
        Some(queue) => queue.push(job),
        None => Err(job),
      };
      // none yet, or the task of one closed and emptied ended since it was last looked at
      if let Err(job) = refused {
        queues.insert(name.clone(), self.run_queue(name, Queue::holding(job)));
      }
    }
  }

  /// `queue`, of the script `name`, with the task that runs what it holds.
  fn run_queue(&self, name: &str, queue: Queue) -> Arc<Queue> {
    let queue = Arc::new(queue);
    self.tasks.spawn(
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
    let one_string = run::given(&run.command).is_some()
      || (run.arglist.is_none() && run::given(&run.args).is_some());
    if !one_string || !lock(&self.advised).insert(name.to_string()) {
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
  waiting: Mutex<Waiting>,
  /// a run was added or the queue closed
  ready: Notify,
  /// runs dropped because too many waited
  dropped: AtomicU64,
  crowded: LogLimiter,
}

/// The runs waiting, and where the queue's task stands, changed together.
#[derive(Default)]
struct Waiting {
  jobs: VecDeque<Job>,
  stage: Stage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
  /// the script is in the options
  #[default]
  Open,
  /// the script left the options; the task ends once the queue is empty
  Closed,
  /// the task ended; the queue takes no more runs
  Ended,
}

impl Queue {
  /// A queue holding `job`.
  fn holding(job: Job) -> Self {
    let queue = Self::default();
    lock(&queue.waiting).jobs.push_back(job);
    queue
  }

  /// Adds a run, opening the queue again when it was closed and dropping the oldest waiting run
  /// when too many wait; gives the run back when the queue's task has ended.
  fn push(&self, job: Job) -> Result<(), Job> {
    let mut waiting = lock(&self.waiting);
    if waiting.stage == Stage::Ended {
      return Err(job);
    }
    waiting.stage = Stage::Open;
    let jobs = &mut waiting.jobs;
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
    drop(waiting);
    self.ready.notify_one();
    Ok(())
  }

  fn close(&self) {
    let mut waiting = lock(&self.waiting);
    if waiting.stage == Stage::Open {
      waiting.stage = Stage::Closed;
    }
    drop(waiting);
    self.ready.notify_one();
  }

  fn ended(&self) -> bool {
    lock(&self.waiting).stage == Stage::Ended
  }

  /// What the queue's task does next; ending marks the queue so it takes no more runs.
  fn next(&self) -> Next {
    let mut waiting = lock(&self.waiting);
    match waiting.jobs.pop_front() {
      Some(job) => Next::Run(job),
      None if waiting.stage == Stage::Closed => {
        waiting.stage = Stage::Ended;
        Next::End
      }
      None => Next::Wait,
    }
  }
}

/// Closes the queues of removed scripts as soon as the options change, until the daemon stops.
async fn follow_options(scripts: Arc<Scripts>) {
  let mut changes = scripts.config.changes();
  loop {
    let change = tokio::select! {
      () = scripts.shutdown.cancelled() => return,
      change = changes.recv() => change,
    };
    match change {
      // a missed change is caught up by reading the options in effect
      Ok(_) | Err(RecvError::Lagged(_)) => scripts.close_removed(&mut lock(&scripts.queues)),
      Err(RecvError::Closed) => return,
    }
  }
}

/// What a queue's task does next.
enum Next {
  /// runs this
  Run(Job),
  /// waits for a run
  Wait,
  /// ends: the script left the options and nothing waits
  End,
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
    match queue.next() {
      Next::Run(job) => {
        let completed = run::run(&job, &context, &shutdown).await;
        events.raise(Event::ScriptRunCompleted(completed));
      }
      Next::Wait => tokio::select! {
        () = queue.ready.notified() => {}
        () = shutdown.cancelled() => return,
      },
      Next::End => return,
    }
  }
}

#[cfg(test)]
mod tests;
