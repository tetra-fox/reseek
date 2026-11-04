//! Taking a lock whose last holder panicked, and holding one of something at a time per
//! principal.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub use reseek_shares::lock;

/// The principals each holding the one slot they may, such as for an archive being sent.
#[derive(Clone, Debug, Default)]
pub struct Slots(Arc<Mutex<HashSet<String>>>);

impl Slots {
  /// The slot of `subject`, free again when dropped; `None` while it holds it already.
  pub fn take(&self, subject: &str) -> Option<Slot> {
    lock(&self.0).insert(subject.to_string()).then(|| Slot {
      slots: self.clone(),
      subject: subject.to_string(),
    })
  }
}

/// A principal's slot, held until dropped.
#[derive(Debug)]
pub struct Slot {
  slots: Slots,
  subject: String,
}

impl Drop for Slot {
  fn drop(&mut self) {
    lock(&self.slots.0).remove(&self.subject);
  }
}
