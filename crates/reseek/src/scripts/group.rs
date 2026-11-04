//! A started script together with whatever it starts, so a run past its timeout is killed with
//! everything it left running: its process group on Unix, a job object on Windows.

use tokio::process::{Child, Command};
use tracing::{debug, warn};

/// Makes the run lead a process group of its own, which its children join.
pub(super) fn prepare(command: &mut Command) {
  #[cfg(unix)]
  command.process_group(0);
  // no console window opens for it
  #[cfg(windows)]
  command.creation_flags(0x0800_0000);
}

/// A started run and the processes it starts.
pub(super) struct Group {
  /// the job the run was put in; a process the script starts before it joined is not in it
  #[cfg(windows)]
  job: Option<win32job::Job>,
}

impl Group {
  /// The group of `child`, just started from a command [`prepare`]d for it.
  pub(super) fn of(#[cfg_attr(not(windows), expect(unused_variables))] child: &Child) -> Self {
    Self {
      #[cfg(windows)]
      job: job_of(child),
    }
  }

  /// Kills the run and, where it has them, every process in its group.
  pub(super) fn kill(self, child: &mut Child) {
    #[cfg(unix)]
    if let Some(group) = child
      .id()
      .and_then(|id| i32::try_from(id).ok())
      .and_then(rustix::process::Pid::from_raw)
    {
      match rustix::process::kill_process_group(group, rustix::process::Signal::KILL) {
        Ok(()) => return,
        Err(e) => warn!(error = %e, "a script's process group was not killed; only the script is"),
      }
    }
    // the job ends every process in it once its last handle closes, which this one is
    #[cfg(windows)]
    if self.job.is_some() {
      drop(self);
      return;
    }
    if let Err(e) = child.start_kill() {
      debug!(error = %e, "a script was not killed; it ended already");
    }
  }

  /// Lets go of the group of a run that ended by itself, leaving what it started running.
  pub(super) fn release(self) {
    #[cfg(windows)]
    if let Some(job) = self.job {
      // without the limit, closing the job's handle ends nothing
      if let Err(e) = job.set_extended_limit_info(&win32job::ExtendedLimitInfo::new()) {
        debug!(error = %e, "a script's job kept its kill limit; what it left running ends");
      }
    }
  }
}

/// A job holding `child`, which ends every process in it when its handle closes. `None` when
/// it could not be made; then only the script itself can be killed.
#[cfg(windows)]
fn job_of(child: &Child) -> Option<win32job::Job> {
  let Some(handle) = child.raw_handle() else {
    debug!("a script ended before it could be put in a job");
    return None;
  };
  let mut limits = win32job::ExtendedLimitInfo::new();
  limits.limit_kill_on_job_close();
  let joined = win32job::Job::create_with_limit_info(&limits)
    .and_then(|job| job.assign_process(handle as isize).map(|()| job));
  match joined {
    Ok(job) => Some(job),
    Err(e) => {
      warn!(error = %e, "a script was not put in a job; only the script can be killed");
      None
    }
  }
}
