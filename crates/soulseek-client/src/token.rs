//! Tokens: the `u32` values that tie a search, a transfer or a connection request to the replies
//! and connections it causes. One counter per client hands them out in order, from a random start
//! unless told otherwise, so a restarted client does not reuse tokens peers may still hold for its
//! previous run.

use std::sync::atomic::{AtomicU32, Ordering};

use uuid::Uuid;

/// The largest token handed out; the count starts again at 0 after it, which keeps every token
/// in the range all clients read correctly
pub const MAX_TOKEN: u32 = i32::MAX as u32;

/// Hands out tokens in sequence from a starting value, wrapping from [`MAX_TOKEN`] to 0
#[derive(Debug)]
pub struct TokenGenerator {
  next: AtomicU32,
}

impl TokenGenerator {
  /// a generator whose first token is `start`, or a random one when `None`
  pub fn new(start: Option<u32>) -> Self {
    // a v4 uuid is 122 random bits, of which 31 make the start
    let start = start.unwrap_or_else(|| (Uuid::new_v4().as_u128() as u32) & MAX_TOKEN);
    Self {
      next: AtomicU32::new(start),
    }
  }

  /// the next token
  pub fn next(&self) -> u32 {
    self
      .next
      .update(Ordering::Relaxed, Ordering::Relaxed, |token| {
        if token >= MAX_TOKEN { 0 } else { token + 1 }
      })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tokens_count_up_from_the_start_and_wrap_at_the_largest() {
    let tokens = TokenGenerator::new(Some(MAX_TOKEN - 1));
    assert_eq!(tokens.next(), MAX_TOKEN - 1);
    assert_eq!(tokens.next(), MAX_TOKEN);
    assert_eq!(tokens.next(), 0);
  }

  #[test]
  fn a_random_start_stays_in_range() {
    for _ in 0..100 {
      assert!(TokenGenerator::new(None).next() <= MAX_TOKEN);
    }
  }
}
