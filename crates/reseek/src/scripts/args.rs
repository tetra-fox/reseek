//! A script's arguments written as one string, split into the program's arguments: spaces and
//! tabs separate arguments outside double quotes, a double quote opens or closes a quoted part,
//! two double quotes inside one stand for a literal quote, and backslashes are literal except
//! before a double quote, where each pair stands for one backslash and an odd one left over
//! makes the quote literal.

/// The arguments of `line`.
pub fn split(line: &str) -> Vec<String> {
  let chars: Vec<char> = line.chars().collect();
  let mut arguments = Vec::new();
  let mut i = 0;
  loop {
    while chars.get(i).is_some_and(|c| is_separator(*c)) {
      i += 1;
    }
    if i == chars.len() {
      return arguments;
    }
    arguments.push(next(&chars, &mut i));
  }
}

fn is_separator(c: char) -> bool {
  c == ' ' || c == '\t'
}

/// The argument starting at `i`, leaving `i` past it.
fn next(chars: &[char], i: &mut usize) -> String {
  let mut argument = String::new();
  let mut quoted = false;
  while let Some(&c) = chars.get(*i) {
    if c == '\\' {
      let start = *i;
      while chars.get(*i) == Some(&'\\') {
        *i += 1;
      }
      let backslashes = *i - start;
      if chars.get(*i) == Some(&'"') {
        argument.extend(std::iter::repeat_n('\\', backslashes / 2));
        if backslashes % 2 == 1 {
          argument.push('"');
          *i += 1;
        }
      } else {
        argument.extend(std::iter::repeat_n('\\', backslashes));
      }
      continue;
    }
    if c == '"' {
      if quoted && chars.get(*i + 1) == Some(&'"') {
        argument.push('"');
        *i += 1;
      } else {
        quoted = !quoted;
      }
      *i += 1;
      continue;
    }
    if is_separator(c) && !quoted {
      break;
    }
    argument.push(c);
    *i += 1;
  }
  argument
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn arguments_split_on_blanks_outside_quotes() {
    assert_eq!(split("  -v\t--name  x "), ["-v", "--name", "x"]);
    assert_eq!(split(r#"-c "echo hi""#), ["-c", "echo hi"]);
    assert_eq!(split(r#"a"b c"d"#), ["ab cd"], "quotes join, not split");
    assert_eq!(split(r#""" x"#), ["", "x"], "an empty quoted argument");
    assert!(split("   ").is_empty());
  }

  #[test]
  fn backslashes_are_literal_but_before_a_quote() {
    assert_eq!(split(r"C:\dir\file a\\b"), [r"C:\dir\file", r"a\\b"]);
    assert_eq!(split(r#"say \"hi\""#), ["say", r#""hi""#]);
    assert_eq!(
      split(r#"a\\"b c""#),
      [r"a\b c"],
      "a pair is one backslash, the quote opens"
    );
    assert_eq!(
      split(r#"a\\\"b"#),
      [r#"a\"b"#],
      "an odd one makes the quote literal"
    );
    assert_eq!(
      split(r#""say ""hi""""#),
      [r#"say "hi""#],
      "doubled inside quotes"
    );
  }

  #[test]
  fn quotes_inside_a_quoted_part_close_it_and_open_the_next() {
    assert_eq!(
      split(r#"-c "echo "new event!" >> event_log.txt""#),
      ["-c", "echo new", "event! >> event_log.txt"]
    );
  }
}
