//! The manual page `reseek man` prints, in roff: the commands, every flag and environment
//! variable the help lists, where the files live and the exit codes.

use cascade::Schema;

use super::help::{flags, variables};
use super::{COMPLETIONS, HASH_PASSWORD, MAN, Shell};
use crate::app::{EXIT_CONFIG, EXIT_FATAL, EXIT_FILESYSTEM, EXIT_LOCKED, EXIT_RESTART, EXIT_USAGE};
use crate::version;

/// The exit codes and what each means.
const EXITS: [(u8, &str); 7] = [
  (0, "It stopped as asked, or a command finished."),
  (EXIT_FATAL, "It could not start, or stopped on an error."),
  (EXIT_USAGE, "The command line could not be understood."),
  (EXIT_CONFIG, "The configuration is not valid."),
  (EXIT_FILESYSTEM, "A directory could not be made or written."),
  (EXIT_LOCKED, "Another instance holds the data directory."),
  (
    EXIT_RESTART,
    "A restart was asked for and a supervisor, such as systemd, is to start it again.",
  ),
];

/// The manual page for every flag and environment variable in `schema`.
pub fn man(schema: &Schema) -> String {
  let mut page = format!(
    ".TH RESEEK 1 \"\" \"reseek {}\" \"User Commands\"\n",
    escape(&version::info().current)
  );
  page.push_str(
    ".SH NAME\nreseek \\- a Soulseek client daemon with a web UI\n.SH SYNOPSIS\n.B reseek\n\
     [\\fIarguments\\fR]\n.br\n",
  );
  page.push_str(&format!(
    ".B reseek {HASH_PASSWORD}\n.br\n.B reseek {COMPLETIONS}\n\\fIshell\\fR\n.br\n.B reseek \
     {MAN}\n"
  ));
  page.push_str(
    ".SH DESCRIPTION\nreseek connects to the Soulseek network, shares the directories it is \
     given, and serves an HTTP API and a web UI for searching, browsing, transfers and chat.\n\
     .PP\nEvery option is read from the configuration file, from an environment variable and \
     from a flag, in that order of precedence from lowest to highest. The configuration \
     reloads when its file changes and on SIGHUP.\n",
  );
  page.push_str(&format!(
    ".SH COMMANDS\n.TP\n.B {HASH_PASSWORD}\nReads a password, without echo from a terminal or \
     as the first line of stdin otherwise, and prints its argon2id hash for password_hash.\n\
     .TP\n.BI \"{COMPLETIONS} \" shell\nPrints the completions of the shell, one of {}.\n.TP\n\
     .B {MAN}\nPrints this page.\n",
    Shell::NAMES.join(", ")
  ));
  page.push_str(".SH OPTIONS\n");
  for flag in flags(schema) {
    let mut item = flag
      .short
      .map(|c| format!("\\fB\\-{c}\\fR, "))
      .unwrap_or_default();
    item.push_str(&format!("\\fB{}\\fR", escape(&flag.long)));
    if let Some(value) = &flag.value {
      item.push_str(&format!(" \\fI{}\\fR", escape(value)));
    }
    page.push_str(&format!(".TP\n{item}\n{}\n", text(&flag.doc)));
  }
  page.push_str(".SH ENVIRONMENT\n");
  page.push_str(
    ".TP\n.B RESEEK_APP_DIR\nThe same as \\-\\-app\\-dir.\n.TP\n.B RESEEK_CONFIG\nThe same as \
     \\-\\-config.\n.TP\n.B RESEEK_CONFIG_MANAGED_BY\nNames the tool that writes the \
     configuration files, such as nixos; they are then read-only through the API.\n",
  );
  for (name, kind, doc) in variables(schema) {
    page.push_str(&format!(
      ".TP\n\\fB{}\\fR \\fI{}\\fR\n{}\n",
      escape(&name),
      escape(&kind),
      text(&doc)
    ));
  }
  page.push_str(
    ".SH FILES\nWith \\-\\-app\\-dir, everything lives under that one directory: config.toml, \
     data/, cache/, logs/, downloads/ and incomplete/. Without it, the configuration is under \
     the user's configuration directory (on Linux ~/.config/reseek), the databases, downloads \
     and incomplete files under the local data directory (~/.local/share/reseek), the share \
     index under the cache directory (~/.cache/reseek) and the logs under the state directory \
     (~/.local/state/reseek/logs).\n",
  );
  page.push_str(".SH EXIT STATUS\n");
  for (code, meaning) in EXITS {
    page.push_str(&format!(".TP\n.B {code}\n{meaning}\n"));
  }
  page
}

/// `value` with roff's escape character and hyphens escaped.
fn escape(value: &str) -> String {
  value.replace('\\', "\\e").replace('-', "\\-")
}

/// `value` as a paragraph: escaped, and kept from starting with a control character.
fn text(value: &str) -> String {
  let escaped = escape(value);
  if escaped.starts_with(['.', '\'']) {
    format!("\\&{escaped}")
  } else {
    escaped
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn text_cannot_start_a_request_or_leave_an_escape() {
    assert_eq!(text(".config"), "\\&.config");
    assert_eq!(text("'a"), "\\&'a");
    assert_eq!(text("a\\b-c"), "a\\eb\\-c");
  }
}
