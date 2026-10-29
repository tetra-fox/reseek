//! Starting again after a restart was asked for: the daemon stops in order and exits with
//! [`EXIT_RESTART`] for a supervisor to start it, or, unsupervised, runs itself again once it
//! has stopped and let go of its instance lock.

use tracing::{error, info};

use super::EXIT_FATAL;

/// The daemon stopped to be started again.
pub const EXIT_RESTART: u8 = 75;

/// The init programs a container runs its process under.
#[cfg(target_os = "linux")]
const CONTAINER_INITS: &[&str] = &["tini", "docker-init", "dumb-init", "catatonit"];

/// Whether something restarts the daemon when it exits with [`EXIT_RESTART`]: systemd, which
/// sets `INVOCATION_ID`, or a container, where the daemon is the first process or a child of
/// the container's init.
pub(super) fn supervised_by_default() -> bool {
  std::env::var_os("INVOCATION_ID").is_some() || std::process::id() == 1 || under_container_init()
}

#[cfg(target_os = "linux")]
fn under_container_init() -> bool {
  if std::os::unix::process::parent_id() != 1 {
    return false;
  }
  // the first process's name; one that cannot be read is no container init
  std::fs::read_to_string("/proc/1/comm").is_ok_and(|name| CONTAINER_INITS.contains(&name.trim()))
}

#[cfg(not(target_os = "linux"))]
fn under_container_init() -> bool {
  false
}

/// Runs this program again with the arguments it was started with: in place of this process
/// on Unix, as a new process elsewhere. Returns the exit code to end with when that fails, or
/// once the new process is started.
pub(super) fn start_again() -> u8 {
  let program = match std::env::current_exe() {
    Ok(program) => program,
    Err(e) => {
      error!(error = %e, "not started again; the program's path is unknown");
      return EXIT_FATAL;
    }
  };
  let mut command = std::process::Command::new(&program);
  command.args(std::env::args_os().skip(1));
  info!(program = %program.display(), "starting again");
  #[cfg(unix)]
  {
    use std::os::unix::process::CommandExt;
    let e = command.exec();
    error!(error = %e, "not started again");
    EXIT_FATAL
  }
  #[cfg(not(unix))]
  match command.spawn() {
    Ok(_) => 0,
    Err(e) => {
      error!(error = %e, "not started again");
      EXIT_FATAL
    }
  }
}
