//! Whether a room message mentions us: a word of it is our username in any case, or it starts
//! with `username:` or `@username`. Words are split on whitespace and on punctuation other than
//! `_`, `-`, `.`, `[` and `]`, which usernames often hold; a username with a space in it is also
//! found anywhere in the text. A message holding one of the words we watch for, in any case and
//! not as part of a longer word, mentions us too.

/// Whether `c` ends a word of a message.
fn separates(c: char) -> bool {
  c.is_whitespace()
    || (c.is_ascii_punctuation() && !matches!(c, '_' | '-' | '.' | '[' | ']'))
    || (!c.is_ascii() && !c.is_alphanumeric())
}

/// Whether `message` mentions `us`, or holds one of the `watched` words.
pub fn mentions(message: &str, us: &str, watched: &[String]) -> bool {
  let text = message.to_lowercase();
  names(&text, us) || watched.iter().any(|word| holds_word(&text, word))
}

/// Whether the lower-cased `text` names `us`.
fn names(text: &str, us: &str) -> bool {
  if us.is_empty() {
    return false;
  }
  let us_folded = us.to_lowercase();
  if text.starts_with(&format!("{us_folded}:")) || text.starts_with(&format!("@{us_folded}")) {
    return true;
  }
  if us.chars().any(char::is_whitespace) && text.contains(&us_folded) {
    return true;
  }
  text.split(separates).any(|word| word == us_folded)
}

/// Whether the lower-cased `text` holds `word` in any case, with no letter, digit or `_` right
/// before or after it. A blank word is in no text.
fn holds_word(text: &str, word: &str) -> bool {
  let word = word.trim().to_lowercase();
  if word.is_empty() {
    return false;
  }
  let joins = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
  text.match_indices(&word).any(|(at, found)| {
    !joins(text[..at].chars().next_back()) && !joins(text[at + found.len()..].chars().next())
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_word_of_our_name_in_any_case_mentions_us() {
    assert!(mentions("hey Alice, look", "alice", &[]));
    assert!(mentions("ALICE!", "alice", &[]));
    assert!(mentions("thanks (alice)", "alice", &[]));
    assert!(!mentions("malice aforethought", "alice", &[]));
    assert!(
      !mentions("alice_b is here", "alice", &[]),
      "_ is part of a name"
    );
    assert!(mentions("ping [dj]-x.", "[dj]-x.", &[]));
    assert!(!mentions("ping [dj]-xy", "[dj]-x", &[]));
    assert!(
      mentions("bob!: are you there", "bob!", &[]),
      "a name with punctuation at the start"
    );
    assert!(mentions("@bob! hi", "bob!", &[]));
    assert!(
      mentions("is the big cheese here?", "Big Cheese", &[]),
      "a name with a space"
    );
    assert!(!mentions("anything", "", &[]));
  }

  #[test]
  fn a_watched_word_on_its_own_mentions_us() {
    let watched = [
      "Floyd".to_string(),
      "dark side".into(),
      "c++".into(),
      " ".into(),
    ];
    let mention = |text| mentions(text, "alice", &watched);
    assert!(mention("anyone got FLOYD flacs?"));
    assert!(mention("floyd."), "punctuation ends a word");
    assert!(mention("the Dark Side of the moon"), "a phrase");
    assert!(mention("i write c++ all day"));
    assert!(!mention("floyds"), "part of a longer word");
    assert!(!mention("pinkfloyd"));
    assert!(!mention("floyd_x"), "_ joins a word");
    assert!(!mention("nothing here"), "a blank word is in no text");
    assert!(mention("«floyd»"), "any punctuation ends a word");
    assert!(
      !mention("überfloyd"),
      "a letter of any alphabet joins a word"
    );
  }
}
