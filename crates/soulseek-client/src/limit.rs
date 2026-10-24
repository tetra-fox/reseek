//! A bound on how many of something run at once, whose size follows an option that can change
//! while the client runs. A new size takes effect for what starts afterwards; what already holds
//! a permit keeps it.

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio_util::sync::CancellationToken;

use crate::error::ClientError;

pub(crate) struct Limiter {
  /// the semaphore and the size it was made with
  current: Mutex<(u32, Arc<Semaphore>)>,
}

impl Limiter {
  pub(crate) fn new(size: u32) -> Self {
    Self {
      current: Mutex::new((size, Arc::new(Semaphore::new(size as usize)))),
    }
  }

  fn semaphore(&self, size: u32) -> Arc<Semaphore> {
    let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
    if current.0 != size {
      *current = (size, Arc::new(Semaphore::new(size as usize)));
    }
    current.1.clone()
  }

  /// waits for a permit under a bound of `size`, or fails with `Cancelled`
  pub(crate) async fn acquire(
    &self,
    size: u32,
    cancel: &CancellationToken,
  ) -> Result<OwnedSemaphorePermit, ClientError> {
    let semaphore = self.semaphore(size);
    tokio::select! {
      biased;
      () = cancel.cancelled() => Err(ClientError::Cancelled),
      permit = semaphore.acquire_owned() => {
        Ok(permit.expect("the limiter never closes its semaphores"))
      }
    }
  }

  /// a permit under a bound of `size` if one is free now
  pub(crate) fn try_acquire(&self, size: u32) -> Option<OwnedSemaphorePermit> {
    match self.semaphore(size).try_acquire_owned() {
      Ok(permit) => Some(permit),
      Err(TryAcquireError::NoPermits) => None,
      Err(TryAcquireError::Closed) => unreachable!("the limiter never closes its semaphores"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
  fn a_new_size_applies_to_what_starts_afterwards() {
    let limiter = Limiter::new(1);
    let held = limiter.try_acquire(1).unwrap();
    assert!(limiter.try_acquire(1).is_none());
    let _more = limiter.try_acquire(2).unwrap();
    let _again = limiter.try_acquire(2).unwrap();
    assert!(limiter.try_acquire(2).is_none());
    drop(held);
  }

  #[tokio::test]
  async fn waiting_for_a_permit_ends_on_cancel() {
    let limiter = Limiter::new(1);
    let _held = limiter.try_acquire(1).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
      limiter.acquire(1, &cancel).await,
      Err(ClientError::Cancelled)
    ));
  }
}
