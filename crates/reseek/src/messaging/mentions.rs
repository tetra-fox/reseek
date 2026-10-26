//! Whether a room message mentions us: a word of it is our username in any case, or it starts
//! with `username:` or `@username`. Words are split on whitespace and on punctuation other than
//! `_`, `-`, `.`, `[` and `]`, which usernames often hold; a username with a space in it is also
//! found anywhere in the text.

/// Whether `c` ends a word of a message.
fn separates(c: char) -> bool {
  c.is_whitespace()
    || (c.is_ascii_punctuation() && !matches!(c, '_' | '-' | '.' | '[' | ']'))
    || (!c.is_ascii() && !c.is_alphanumeric())
}

/// Whether `message` mentions `us`.
pub fn mentions(message: &str, us: &str) -> bool {
  if us.is_empty() {
    return false;
  }
  let us_folded = us.to_lowercase();
  let text = message.to_lowercase();
  if text.starts_with(&format!("{us_folded}:")) || text.starts_with(&format!("@{us_folded}")) {
    return true;
  }
  if us.chars().any(char::is_whitespace) && text.contains(&us_folded) {
    return true;
  }
  text.split(separates).any(|word| word == us_folded)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_word_of_our_name_in_any_case_mentions_us() {
    assert!(mentions("hey Alice, look", "alice"));
    assert!(mentions("ALICE!", "alice"));
    assert!(mentions("thanks (alice)", "alice"));
    assert!(!mentions("malice aforethought", "alice"));
    assert!(!mentions("alice_b is here", "alice"), "_ is part of a name");
    assert!(mentions("ping [dj]-x.", "[dj]-x."));
    assert!(!mentions("ping [dj]-xy", "[dj]-x"));
    assert!(
      mentions("bob!: are you there", "bob!"),
      "a name with punctuation at the start"
    );
    assert!(mentions("@bob! hi", "bob!"));
    assert!(
      mentions("is the big cheese here?", "Big Cheese"),
      "a name with a space"
    );
    assert!(!mentions("anything", ""));
  }
}
