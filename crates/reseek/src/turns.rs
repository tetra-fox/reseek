//! One at a time per user: a caller takes the user's turn, and a second caller for the same
//! user waits until the first lets go of it. A user's entry is kept only while someone holds or
//! waits for their turn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::OwnedMutexGuard;

use crate::sync::lock;

type Map = HashMap<String, Arc<tokio::sync::Mutex<()>>>;

/// Every user's turn. Cloning gives another handle to the same turns.
#[derive(Clone, Default)]
pub struct Turns {
  map: Arc<Mutex<Map>>,
}

impl Turns {
  /// Waits for `username`'s turn and holds it until the returned turn is dropped.
  pub async fn take(&self, username: &str) -> Turn {
    let mutex = lock(&self.map)
      .entry(username.to_string())
      .or_default()
      .clone();
    let guard = mutex.lock_owned().await;
    Turn {
      map: self.map.clone(),
      username: username.to_string(),
      guard: Some(guard),
    }
  }
}

/// One user's turn, held until dropped; the user's entry goes once nobody holds or waits for
/// it.
pub struct Turn {
  map: Arc<Mutex<Map>>,
  username: String,
  guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for Turn {
  fn drop(&mut self) {
    let mut map = lock(&self.map);
    let Some(mutex) = map.get(&self.username) else {
      return;
    };
    // the map's and the guard's references are the only ones when nobody waits
    let idle = Arc::strong_count(mutex) == 2;
    self.guard.take();
    if idle {
      map.remove(&self.username);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  #[tokio::test]
  async fn a_second_caller_waits_and_the_entry_goes_with_the_last() {
    let turns = Turns::default();
    let first = turns.take("alice").await;
    let waiting = tokio::spawn({
      let turns = turns.clone();
      async move { turns.take("alice").await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!waiting.is_finished(), "alice's turn is held");
    let other = turns.take("bob").await;
    drop(first);
    let second = waiting.await.unwrap();
    assert_eq!(lock(&turns.map).len(), 2);
    drop((second, other));
    assert!(lock(&turns.map).is_empty());
  }
}
