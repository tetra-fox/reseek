//! One at a time per name, a user's or a room's: a caller takes the name's turn, and a second
//! caller for the same name waits until the first lets go of it. A name's entry is kept only
//! while someone holds or waits for its turn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reseek_shares::lock;
use tokio::sync::OwnedMutexGuard;

type Map = HashMap<String, Arc<tokio::sync::Mutex<()>>>;

/// Every name's turn. Cloning gives another handle to the same turns.
#[derive(Clone, Default)]
pub struct Turns {
  map: Arc<Mutex<Map>>,
}

impl Turns {
  /// Waits for `name`'s turn and holds it until the returned turn is dropped.
  pub async fn take(&self, name: &str) -> Turn {
    let mutex = lock(&self.map).entry(name.to_string()).or_default().clone();
    let guard = mutex.lock_owned().await;
    Turn {
      map: self.map.clone(),
      name: name.to_string(),
      guard: Some(guard),
    }
  }
}

/// One name's turn, held until dropped; the name's entry goes once nobody holds or waits for
/// it.
pub struct Turn {
  map: Arc<Mutex<Map>>,
  name: String,
  guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for Turn {
  fn drop(&mut self) {
    let mut map = lock(&self.map);
    let Some(mutex) = map.get(&self.name) else {
      return;
    };
    // the map's and the guard's references are the only ones when nobody waits
    let idle = Arc::strong_count(mutex) == 2;
    self.guard.take();
    if idle {
      map.remove(&self.name);
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
