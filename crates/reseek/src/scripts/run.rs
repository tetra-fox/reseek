//! One run of a script for one event: its program and arguments, the event in its environment,
//! the run waited for within the script's timeout with its process group killed past it, both
//! output streams read meanwhile, and how it went as a `ScriptRunCompleted`.

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api_types::events::{Event, EventRecord, ScriptRunCompleted};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::output::{self, Tail};
use crate::config::{Script, ScriptRun};
use crate::sync::lock;

/// Past this size the event goes to a file instead of the environment, where Linux takes no
/// single string over 128 KiB.
const INLINE_LIMIT: usize = 100 * 1024;

/// How long the output is still read after the script exits, for what it left running with its
/// pipes open.
const LINGER: Duration = Duration::from_secs(1);

/// The program a script without one of its own runs its command in.
#[derive(Clone, Debug)]
pub struct Shell {
  /// the program
  pub program: String,
  /// the argument that makes it run a command: `-c`, or `/c` for `cmd.exe`
  pub prefix: &'static str,
}

/// What every run uses: where scripts run and the shell.
pub struct Context {
  /// the working directory of every run, where a large event's file is written
  pub directory: PathBuf,
  /// the shell, `None` when the system names none
  pub shell: Option<Shell>,
}

/// A run waiting its turn: the event, and the script as it was defined when the event came.
pub struct Job {
  /// the script's name
  pub name: String,
  /// what it runs
  pub script: Box<Script>,
  /// the event it runs for
  pub record: Arc<EventRecord>,
}

/// How a run is started.
#[derive(Debug, PartialEq)]
struct Invocation {
  program: String,
  arguments: Arguments,
}

/// A run's arguments: a list passed as it is, or one string the system splits.
#[derive(Debug, PartialEq)]
enum Arguments {
  List(Vec<String>),
  Line(String),
}

/// How a run ended.
#[derive(Default)]
struct Ended {
  exit_code: Option<i32>,
  timed_out: bool,
  error: Option<String>,
  output: String,
  /// bytes read from stdout and stderr
  read: (u64, u64),
}

impl Ended {
  fn not_started(error: String) -> Self {
    Self {
      error: Some(error),
      ..Self::default()
    }
  }
}

/// `value` when it holds more than blanks.
pub(super) fn given(value: &Option<String>) -> Option<&str> {
  value.as_deref().filter(|v| !v.trim().is_empty())
}

impl Invocation {
  /// How `run` is started: a command in the shell, or its own program with an argument list or
  /// an argument string.
  fn of(run: &ScriptRun, shell: Option<&Shell>) -> Result<Self, String> {
    const NO_SHELL: &str =
      "no program to run: the script names no executable and $SHELL is missing or blank";
    if let Some(command) = given(&run.command) {
      let command = command.trim();
      let shell = shell.ok_or(NO_SHELL)?;
      let line = if command.starts_with(shell.prefix) {
        command.to_string()
      } else {
        format!("{} \"{}\"", shell.prefix, command.trim_matches('"'))
      };
      return Ok(Self {
        program: shell.program.clone(),
        arguments: Arguments::Line(line),
      });
    }
    let program = match given(&run.executable) {
      Some(program) => program.to_string(),
      None => shell.ok_or(NO_SHELL)?.program.clone(),
    };
    let arguments = match &run.arglist {
      Some(list) => Arguments::List(list.clone()),
      None => Arguments::Line(run.args.clone().unwrap_or_default()),
    };
    Ok(Self { program, arguments })
  }

  /// The command that starts it, with nothing of its environment yet.
  fn command(&self) -> Command {
    let mut command = Command::new(&self.program);
    match &self.arguments {
      Arguments::List(list) => {
        command.args(list);
      }
      // Windows programs split their own command line
      #[cfg(windows)]
      Arguments::Line(line) => {
        command.raw_arg(line);
      }
      #[cfg(not(windows))]
      Arguments::Line(line) => {
        command.args(super::args::split(line));
      }
    }
    command
  }
}

/// The variables a script finds the event in: the event as JSON, its type and id, and for a
/// download that ended, how it ended without the JSON.
fn variables(
  record: &EventRecord,
  data: String,
  file: Option<&Path>,
) -> Vec<(&'static str, String)> {
  let mut variables = vec![
    ("RESEEK_SCRIPT_DATA", data),
    (
      "RESEEK_SCRIPT_EVENT_TYPE",
      record.event.event_type().name().to_string(),
    ),
    ("RESEEK_SCRIPT_EVENT_ID", record.id.to_string()),
  ];
  if let Some(file) = file {
    variables.push(("RESEEK_SCRIPT_DATA_FILE", file.display().to_string()));
  }
  let outcome = match &record.event {
    Event::DownloadFileComplete(complete) => Some((
      "succeeded",
      String::new(),
      &complete.transfer.username,
      &complete.remote_filename,
      complete.local_filename.clone(),
    )),
    Event::DownloadFileFailed(failed) => Some((
      "failed",
      failed.reason.clone(),
      &failed.transfer.username,
      &failed.remote_filename,
      failed.local_filename.clone().unwrap_or_default(),
    )),
    _ => None,
  };
  if let Some((result, reason, username, remote, local)) = outcome {
    variables.extend([
      ("RESEEK_SCRIPT_RESULT", result.to_string()),
      ("RESEEK_SCRIPT_REASON", reason),
      ("RESEEK_SCRIPT_USERNAME", username.clone()),
      ("RESEEK_SCRIPT_REMOTE_PATH", remote.clone()),
      ("RESEEK_SCRIPT_LOCAL_PATH", local),
    ]);
  }
  variables
}

/// The event as a script reads it, and the file it is in when it is too large for the
/// environment, where the variable then holds only its envelope.
async fn data(
  record: &EventRecord,
  directory: &Path,
  run: Uuid,
) -> Result<(String, Option<PathBuf>), String> {
  let json = serde_json::to_value(record).map_err(|e| format!("the event was not written: {e}"))?;
  let text = json.to_string();
  if text.len() <= INLINE_LIMIT {
    return Ok((text, None));
  }
  let file = directory.join(format!(".event-{run}.json"));
  tokio::fs::write(&file, &text).await.map_err(|e| {
    format!(
      "the event, too large for the environment, was not written to {}: {e}",
      file.display()
    )
  })?;
  let envelope = serde_json::json!({
    "id": json["id"],
    "timestamp": json["timestamp"],
    "type": json["type"],
    "version": json["data"]["version"],
    "truncated": true,
  });
  Ok((envelope.to_string(), Some(file)))
}

/// Runs `job` once and says how it went. A stop of the daemon kills the run.
pub async fn run(job: &Job, context: &Context, shutdown: &CancellationToken) -> ScriptRunCompleted {
  let run_id = Uuid::new_v4();
  let started = Instant::now();
  let kind = job.record.event.event_type();
  let ended = attempt(job, context, run_id, shutdown).await;
  let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
  let succeeded = !ended.timed_out && ended.exit_code == Some(0);
  let (stdout, stderr) = ended.read;
  if succeeded {
    info!(script = job.name, event = kind.name(), ms = duration_ms, run = %run_id, "script exited 0");
    if stderr > 0 {
      debug!(script = job.name, run = %run_id, output = ended.output, "script wrote to stderr");
    }
  } else {
    warn!(
      script = job.name,
      event = kind.name(),
      exit_code = ended.exit_code,
      error = ended.error,
      ms = duration_ms,
      stdout,
      stderr,
      output = ended.output,
      run = %run_id,
      "Failed to run script"
    );
  }
  ScriptRunCompleted {
    version: 0,
    script: job.name.clone(),
    run_id,
    event_id: job.record.id,
    event_type: kind,
    succeeded,
    exit_code: ended.exit_code,
    timed_out: ended.timed_out,
    error: ended.error,
    duration_ms,
    output: ended.output,
  }
}

/// Starts the run, waits for it within its timeout and reads its output.
async fn attempt(
  job: &Job,
  context: &Context,
  run_id: Uuid,
  shutdown: &CancellationToken,
) -> Ended {
  let invocation = match Invocation::of(&job.script.run, context.shell.as_ref()) {
    Ok(invocation) => invocation,
    Err(e) => return Ended::not_started(e),
  };
  let (data, file) = match data(&job.record, &context.directory, run_id).await {
    Ok(data) => data,
    Err(e) => return Ended::not_started(e),
  };
  debug!(
    script = job.name,
    program = invocation.program,
    arguments = ?invocation.arguments,
    run = %run_id,
    "Running script"
  );
  let mut command = invocation.command();
  command
    .current_dir(&context.directory)
    .envs(variables(&job.record, data, file.as_deref()))
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  // the script and whatever it starts can then be killed together
  #[cfg(unix)]
  command.process_group(0);
  // no console window opens for it
  #[cfg(windows)]
  command.creation_flags(0x0800_0000);
  let ended = match command.spawn() {
    Ok(child) => waited(child, job.script.timeout, shutdown).await,
    Err(e) => Ended::not_started(format!("{} could not be started: {e}", invocation.program)),
  };
  if let Some(file) = file
    && let Err(e) = tokio::fs::remove_file(&file).await
  {
    warn!(path = %file.display(), error = %e, "the file of a script's event not deleted");
  }
  ended
}

/// How a started run ended: its exit, or the timeout or the stop that killed it.
enum Exit {
  Status(ExitStatus),
  TimedOut(Duration),
  Stopped,
  Unknown(std::io::Error),
}

/// Waits for `child` within `timeout`, none when it is zero, reading its output meanwhile.
async fn waited(mut child: Child, timeout: Duration, shutdown: &CancellationToken) -> Ended {
  let tail = Arc::new(Mutex::new(Tail::default()));
  let readers = (
    child
      .stdout
      .take()
      .map(|out| tokio::spawn(output::drain(out, tail.clone()))),
    child
      .stderr
      .take()
      .map(|err| tokio::spawn(output::drain(err, tail.clone()))),
  );
  let limited = async {
    if timeout.is_zero() {
      return child.wait().await.map_or_else(Exit::Unknown, Exit::Status);
    }
    match tokio::time::timeout(timeout, child.wait()).await {
      Ok(waited) => waited.map_or_else(Exit::Unknown, Exit::Status),
      Err(_) => Exit::TimedOut(timeout),
    }
  };
  let exit = tokio::select! {
    exit = limited => exit,
    () = shutdown.cancelled() => Exit::Stopped,
  };
  if matches!(exit, Exit::TimedOut(_) | Exit::Stopped) {
    kill(&mut child);
    if let Err(e) = child.wait().await {
      debug!(error = %e, "a killed script was not waited for");
    }
  }
  let read = (finish(readers.0).await, finish(readers.1).await);
  let output = lock(&tail).text();
  let mut ended = Ended {
    output,
    read,
    ..Ended::default()
  };
  match exit {
    Exit::Status(status) => {
      ended.exit_code = status.code();
      if ended.exit_code.is_none() {
        ended.error = Some(format!("ended by a signal ({status})"));
      }
    }
    Exit::TimedOut(after) => {
      ended.timed_out = true;
      ended.error = Some(format!(
        "timed out after {}",
        humantime::format_duration(after)
      ));
    }
    Exit::Stopped => ended.error = Some("stopped with the daemon".to_string()),
    Exit::Unknown(e) => ended.error = Some(format!("its end could not be read: {e}")),
  }
  ended
}

/// The bytes a reader read once it is done, waiting a moment for output a process the script
/// left running still holds open; what is not done by then is stopped.
async fn finish(reader: Option<tokio::task::JoinHandle<u64>>) -> u64 {
  let Some(mut reader) = reader else {
    return 0;
  };
  match tokio::time::timeout(LINGER, &mut reader).await {
    Ok(Ok(read)) => read,
    Ok(Err(e)) => {
      debug!(error = %e, "a script's output reader ended badly");
      0
    }
    Err(_) => {
      debug!("a script left its output open; no longer read");
      reader.abort();
      0
    }
  }
}

/// Kills the run and, where it has one, every process in its group.
fn kill(child: &mut Child) {
  #[cfg(unix)]
  if let Some(group) = child
    .id()
    .and_then(|id| i32::try_from(id).ok())
    .and_then(rustix::process::Pid::from_raw)
  {
    match rustix::process::kill_process_group(group, rustix::process::Signal::KILL) {
      Ok(()) => return,
      Err(e) => debug!(error = %e, "a script's process group was not killed; only the script is"),
    }
  }
  if let Err(e) = child.start_kill() {
    debug!(error = %e, "a script was not killed; it ended already");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn shell() -> Shell {
    Shell {
      program: "/bin/sh".into(),
      prefix: "-c",
    }
  }

  fn run(
    command: Option<&str>,
    executable: Option<&str>,
    args: Option<&str>,
    arglist: Option<&[&str]>,
  ) -> ScriptRun {
    ScriptRun {
      command: command.map(Into::into),
      executable: executable.map(Into::into),
      args: args.map(Into::into),
      arglist: arglist.map(|list| list.iter().map(ToString::to_string).collect()),
    }
  }

  #[test]
  fn a_command_runs_quoted_in_the_shell_unless_it_starts_with_its_switch() {
    let shell = shell();
    let line =
      |command| Invocation::of(&run(Some(command), None, None, None), Some(&shell)).unwrap();
    assert_eq!(
      line("echo hi").arguments,
      Arguments::Line(r#"-c "echo hi""#.into())
    );
    assert_eq!(
      line(r#""echo hi""#).arguments,
      Arguments::Line(r#"-c "echo hi""#.into())
    );
    assert_eq!(
      line("-c 'echo hi'").arguments,
      Arguments::Line("-c 'echo hi'".into())
    );
    assert_eq!(
      line("  -c 'echo hi' ").arguments,
      Arguments::Line("-c 'echo hi'".into()),
      "blanks around it do not hide the switch"
    );
    assert_eq!(line("echo hi").program, "/bin/sh");
    let listed =
      Invocation::of(&run(None, Some("/usr/bin/env"), None, Some(&["a b"])), None).unwrap();
    assert_eq!(
      listed.arguments,
      Arguments::List(vec!["a b".into()]),
      "no switch is added"
    );
    let unshelled = Invocation::of(&run(Some("echo hi"), None, None, None), None);
    assert!(unshelled.unwrap_err().contains("$SHELL"));
    let bare = Invocation::of(&run(None, Some("/bin/true"), None, None), None).unwrap();
    assert_eq!(bare.arguments, Arguments::Line(String::new()));
  }
}
