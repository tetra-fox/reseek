//! Tokens: the `u32` values that tie a search, a transfer or a connection request to the replies
//! and connections it causes. One counter per client hands them out in order.

use std::sync::atomic::{AtomicU32, Ordering};

/// Hands out tokens in sequence from a starting value, wrapping from `u32::MAX` to 0
#[derive(Debug)]
pub struct TokenGenerator {
  next: AtomicU32,
}

impl TokenGenerator {
  /// a generator whose first token is `start`
  pub fn new(start: u32) -> Self {
    Self {
      next: AtomicU32::new(start),
    }
  }

  /// the next token
  pub fn next(&self) -> u32 {
    // fetch_add wraps on overflow
    self.next.fetch_add(1, Ordering::Relaxed)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tokens_count_up_from_the_start_and_wrap() {
    let tokens = TokenGenerator::new(u32::MAX - 1);
    assert_eq!(tokens.next(), u32::MAX - 1);
    assert_eq!(tokens.next(), u32::MAX);
    assert_eq!(tokens.next(), 0);
  }
}
