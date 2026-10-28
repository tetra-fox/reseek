//! Taking a lock whose last holder panicked.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Takes `mutex`, also after a thread panicked while holding it. Every lock of the daemon is
/// changed whole before it is let go, so what a panic leaves behind is a value as sound as any.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
