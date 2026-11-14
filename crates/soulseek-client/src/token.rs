//! Tokens: the `u32` values that tie a search or a transfer to the replies and connections it
//! causes. One counter per client hands them out in order, from a random start unless told
//! otherwise, so a restarted client does not reuse tokens peers may still hold for its previous
//! run. Connection requests take a random token each instead, since a search token goes out to
//! every user and the next count after it is easy to guess.

use std::sync::atomic::{AtomicU32, Ordering};

use uuid::Uuid;

/// The largest token handed out; the count starts again at 0 after it, which keeps every token
/// in the range all clients read correctly
pub const MAX_TOKEN: u32 = i32::MAX as u32;

/// a random token in the range every client reads correctly
pub(crate) fn random_token() -> u32 {
  // a v4 uuid is 122 random bits, of which 31 make the token
  (Uuid::new_v4().as_u128() as u32) & MAX_TOKEN
}

/// Hands out tokens in sequence from a starting value, wrapping from [`MAX_TOKEN`] to 0
#[derive(Debug)]
pub struct TokenGenerator {
  next: AtomicU32,
}

impl TokenGenerator {
  /// a generator whose first token is `start`, or a random one when `None`
  pub fn new(start: Option<u32>) -> Self {
    let start = start.unwrap_or_else(random_token);
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

  /// The next token `taken` does not claim, skipping the ones still in use, so a token that
  /// wrapped around never lands on a search or transfer that outlived a full cycle
  pub fn next_free(&self, taken: impl Fn(u32) -> bool) -> u32 {
    loop {
      let token = self.next();
      if !taken(token) {
        return token;
      }
      tracing::debug!(token, "token still in use, skipped");
    }
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
  fn tokens_in_use_are_skipped() {
    let tokens = TokenGenerator::new(Some(5));
    assert_eq!(tokens.next_free(|token| token == 5 || token == 6), 7);
    assert_eq!(tokens.next(), 8);
  }
}
