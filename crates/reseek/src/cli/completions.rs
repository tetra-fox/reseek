//! The shell completions `reseek completions <shell>` prints: every flag the help lists, the word
//! commands, and file names after a flag whose value is a path.

use cascade::Schema;

use super::help::{FlagDoc, flags};
use super::{COMPLETIONS, HASH_PASSWORD, MAN, Shell};

/// The words that may come first instead of a flag.
const WORDS: [&str; 3] = [HASH_PASSWORD, COMPLETIONS, MAN];

/// The completion script of `shell` for every flag in `schema` and the command line's own.
pub fn completions(shell: Shell, schema: &Schema) -> String {
  let flags = flags(schema);
  match shell {
    Shell::Bash => bash(&flags),
    Shell::Zsh => zsh(&flags),
    Shell::Fish => fish(&flags),
  }
}

/// Whether the flag's value names a file or directory.
fn takes_path(flag: &FlagDoc) -> bool {
  flag.value.as_deref() == Some("path")
}

/// Each spelling of the flag: its short form, if any, then its long form.
fn spellings(flag: &FlagDoc) -> Vec<String> {
  flag
    .short
    .map(|c| format!("-{c}"))
    .into_iter()
    .chain([flag.long.clone()])
    .collect()
}

/// The first sentence of the flag's help, which is what a completion menu has room for.
fn summary(flag: &FlagDoc) -> &str {
  match flag.doc.find(". ") {
    Some(end) => &flag.doc[..=end],
    None => &flag.doc,
  }
}

fn bash(flags: &[FlagDoc]) -> String {
  let join = |pick: &dyn Fn(&FlagDoc) -> bool| {
    flags
      .iter()
      .filter(|flag| pick(flag))
      .flat_map(spellings)
      .collect::<Vec<_>>()
      .join("|")
  };
  let paths = join(&takes_path);
  let values = join(&|flag| flag.value.is_some() && !takes_path(flag));
  let all = flags
    .iter()
    .flat_map(spellings)
    .collect::<Vec<_>>()
    .join(" ");
  format!(
    r#"# bash completion for reseek
_reseek() {{
  local cur="${{COMP_WORDS[COMP_CWORD]}}" prev="${{COMP_WORDS[COMP_CWORD-1]}}"
  if [[ $COMP_CWORD -eq 1 && $cur != -* ]]; then
    COMPREPLY=($(compgen -W "{words}" -- "$cur"))
    return
  fi
  if [[ ${{COMP_WORDS[1]}} == {COMPLETIONS} ]]; then
    [[ $COMP_CWORD -eq 2 ]] && COMPREPLY=($(compgen -W "{shells}" -- "$cur"))
    return
  fi
  case "$prev" in
    {paths}) COMPREPLY=($(compgen -f -- "$cur")); return ;;
    {values}) return ;;
  esac
  COMPREPLY=($(compgen -W "{all}" -- "$cur"))
}}
complete -o filenames -F _reseek reseek
"#,
    words = WORDS.join(" "),
    shells = Shell::NAMES.join(" "),
  )
}

/// `text` inside a zsh `_arguments` spec in single quotes: the quote closed and reopened around
/// an escaped one, and the brackets and colons the spec itself uses escaped.
fn zsh_escape(text: &str) -> String {
  text
    .replace('\'', r"'\''")
    .replace('[', r"\[")
    .replace(']', r"\]")
    .replace(':', r"\:")
}

fn zsh(flags: &[FlagDoc]) -> String {
  let mut specs: Vec<String> = flags
    .iter()
    .map(|flag| {
      let spellings = spellings(flag);
      let names = if spellings.len() > 1 {
        format!("'({})'{{{}}}", spellings.join(" "), spellings.join(","))
      } else {
        spellings[0].clone()
      };
      let value = match &flag.value {
        None => String::new(),
        Some(_) if takes_path(flag) => ":path:_files".to_string(),
        Some(kind) => format!(":{kind}: "),
      };
      format!("{names}'[{}]{value}'", zsh_escape(summary(flag)))
    })
    .collect();
  specs.push(format!("'1: :({})'", WORDS.join(" ")));
  specs.push(format!(
    "'2: :{{[[ $words[2] == {COMPLETIONS} ]] && compadd {}}}'",
    Shell::NAMES.join(" ")
  ));
  format!(
    "#compdef reseek\n\n_arguments -s \\\n  {}\n",
    specs.join(" \\\n  ")
  )
}

/// `text` as a fish single-quoted word.
fn fish_quote(text: &str) -> String {
  format!("'{}'", text.replace('\\', r"\\").replace('\'', r"\'"))
}

fn fish(flags: &[FlagDoc]) -> String {
  let mut lines = vec![
    "# fish completion for reseek".to_string(),
    "complete -c reseek -f".to_string(),
    format!(
      "complete -c reseek -n __fish_use_subcommand -a {}",
      fish_quote(&WORDS.join(" "))
    ),
    format!(
      "complete -c reseek -n {} -a {}",
      fish_quote(&format!("__fish_seen_subcommand_from {COMPLETIONS}")),
      fish_quote(&Shell::NAMES.join(" "))
    ),
  ];
  for flag in flags {
    let mut line = "complete -c reseek".to_string();
    if let Some(c) = flag.short {
      line.push_str(&format!(" -s {c}"));
    }
    line.push_str(&format!(" -l {}", flag.long.trim_start_matches("--")));
    match &flag.value {
      Some(_) if takes_path(flag) => line.push_str(" -r -F"),
      Some(_) => line.push_str(" -r"),
      None => {}
    }
    line.push_str(&format!(" -d {}", fish_quote(summary(flag))));
    lines.push(line);
  }
  lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn quotes_and_spec_characters_are_escaped() {
    assert_eq!(zsh_escape("a 'b' [c]: d"), r"a '\''b'\'' \[c\]\: d");
    assert_eq!(fish_quote(r"it's a\b"), r"'it\'s a\\b'");
  }

  /// Each script offers every flag and word, and parses in its shell where that shell is
  /// installed.
  #[test]
  fn every_flag_and_word_is_offered_in_a_script_the_shell_reads() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let schema = Schema::of::<crate::config::Options>();
    for (shell, program, check) in [
      (Shell::Bash, "bash", "-n"),
      (Shell::Zsh, "zsh", "-n"),
      (Shell::Fish, "fish", "--no-execute"),
    ] {
      let script = completions(shell, &schema);
      for flag in ["--http-port", "--app-dir", "--config-file", "--debug"] {
        let long = if shell == Shell::Fish {
          flag.trim_start_matches("--")
        } else {
          flag
        };
        assert!(script.contains(long), "{program} lacks {flag}");
      }
      for word in WORDS {
        assert!(script.contains(word), "{program} lacks {word}");
      }
      // a shell that is not installed has nothing to say about the script
      let Ok(mut child) = Command::new(program)
        .arg(check)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
      else {
        continue;
      };
      child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
      let output = child.wait_with_output().unwrap();
      assert!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
      );
    }
  }
}
