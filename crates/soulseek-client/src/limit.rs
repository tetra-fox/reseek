//! A bound on how many of something run at once, whose size follows an option that can change
//! while the client runs. Growing it frees permits at once; shrinking it takes free permits away
//! at once and the rest as their holders finish, so what already runs keeps running and nothing
//! new starts until the count is under the new size.

use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::error::ClientError;
use crate::lock;

struct Sizing {
  size: u32,
  /// permits still to be taken away from a shrink, as their holders give them back
  owed: u32,
}

struct Shared {
  semaphore: Arc<Semaphore>,
  sizing: Mutex<Sizing>,
}

pub(crate) struct Limiter {
  shared: Arc<Shared>,
}

/// One of a limiter's permits, given back when dropped
pub(crate) struct Permit {
  permit: Option<OwnedSemaphorePermit>,
  shared: Arc<Shared>,
}

impl Drop for Permit {
  fn drop(&mut self) {
    let mut sizing = lock(&self.shared.sizing);
    if sizing.owed > 0
      && let Some(permit) = self.permit.take()
    {
      sizing.owed -= 1;
      permit.forget();
    }
  }
}

impl Limiter {
  pub(crate) fn new(size: u32) -> Self {
    Self {
      shared: Arc::new(Shared {
        semaphore: Arc::new(Semaphore::new(size as usize)),
        sizing: Mutex::new(Sizing { size, owed: 0 }),
      }),
    }
  }

  /// makes the bound `size`
  pub(crate) fn resize(&self, size: u32) {
    let mut sizing = lock(&self.shared.sizing);
    if size > sizing.size {
      let grow = size - sizing.size;
      let repaid = grow.min(sizing.owed);
      sizing.owed -= repaid;
      self.shared.semaphore.add_permits((grow - repaid) as usize);
    } else if size < sizing.size {
      let shrink = sizing.size - size;
      let taken = self.shared.semaphore.forget_permits(shrink as usize) as u32;
      sizing.owed += shrink - taken;
    }
    sizing.size = size;
  }

  fn permit(&self, permit: OwnedSemaphorePermit) -> Permit {
    Permit {
      permit: Some(permit),
      shared: self.shared.clone(),
    }
  }

  /// waits for a permit under a bound of `size`, or fails with `Canceled`
  pub(crate) async fn acquire(
    &self,
    size: u32,
    cancel: &CancellationToken,
  ) -> Result<Permit, ClientError> {
    self.resize(size);
    let semaphore = self.shared.semaphore.clone();
    tokio::select! {
      biased;
      () = cancel.cancelled() => Err(ClientError::Canceled),
      permit = semaphore.acquire_owned() => {
        Ok(self.permit(permit.expect("the limiter never closes its semaphore")))
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use tokio::sync::TryAcquireError;

  use super::*;

  impl Limiter {
    /// a permit under a bound of `size` if one is free now
    fn try_acquire(&self, size: u32) -> Option<Permit> {
      self.resize(size);
      match self.shared.semaphore.clone().try_acquire_owned() {
        Ok(permit) => Some(self.permit(permit)),
        Err(TryAcquireError::NoPermits) => None,
        Err(TryAcquireError::Closed) => unreachable!("the limiter never closes its semaphore"),
      }
    }
  }

  #[test]
  fn permits_run_out_at_the_size() {
    let limiter = Limiter::new(2);
    let first = limiter.try_acquire(2).unwrap();
    let _second = limiter.try_acquire(2).unwrap();
    assert!(limiter.try_acquire(2).is_none());
    drop(first);
    assert!(limiter.try_acquire(2).is_some());
  }

  #[test]
  fn growing_frees_permits_at_once() {
    let limiter = Limiter::new(1);
    let _held = limiter.try_acquire(1).unwrap();
    assert!(limiter.try_acquire(1).is_none());
    let _more = limiter.try_acquire(2).unwrap();
    assert!(limiter.try_acquire(2).is_none());
  }

  #[test]
  fn shrinking_waits_for_holders_to_finish() {
    let limiter = Limiter::new(3);
    let first = limiter.try_acquire(3).unwrap();
    let second = limiter.try_acquire(3).unwrap();
    // one permit is free and goes at once; one more goes when a holder gives it back
    limiter.resize(1);
    assert!(limiter.try_acquire(1).is_none());
    drop(first);
    assert!(limiter.try_acquire(1).is_none(), "still one running");
    drop(second);
    let only = limiter.try_acquire(1).unwrap();
    assert!(limiter.try_acquire(1).is_none());
    drop(only);
    // growing again first cancels what is still owed
    limiter.resize(2);
    let _a = limiter.try_acquire(2).unwrap();
    let _b = limiter.try_acquire(2).unwrap();
    assert!(limiter.try_acquire(2).is_none());
  }

  #[test]
  fn growing_cancels_a_shrink_still_owed() {
    let limiter = Limiter::new(2);
    let first = limiter.try_acquire(2).unwrap();
    let second = limiter.try_acquire(2).unwrap();
    limiter.resize(1);
    limiter.resize(2);
    drop(first);
    drop(second);
    let _a = limiter.try_acquire(2).unwrap();
    let _b = limiter.try_acquire(2).unwrap();
    assert!(limiter.try_acquire(2).is_none());
  }
}
