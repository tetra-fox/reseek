//! What a configuration must satisfy beyond what its types enforce. Every check runs and every
//! problem is reported, each under the key it is about; a problem inside a user-keyed entry
//! is reported under the map's key, with the entry's name in the message.

mod analysis;
pub(crate) mod blacklist;
mod fields;
mod general;
mod paths;
mod relay;
mod scripts;
mod soulseek;
mod transfers;
mod web;

use std::path::PathBuf;

use cascade::Invalid;

use super::Options;

/// What validation needs to know besides the options.
pub struct Context {
  /// the default downloads directory for these options, exempt from the exists-and-writable
  /// check since it is created at startup
  pub default_downloads: PathBuf,
  /// the default incomplete directory, exempt like `default_downloads`
  pub default_incomplete: PathBuf,
  /// the directory of the running program, which `web.content_path` is relative to
  pub program_dir: PathBuf,
  /// whether the daemon started with case-sensitive patterns: the flag takes a restart, so every
  /// pattern is compiled with it until then, and checked the same way
  pub started_case_sensitive: bool,
}

/// Every problem with `options`, all of them rather than the first.
pub fn validate(options: &Options, context: &Context) -> Result<(), Vec<Invalid>> {
  let mut report = Report::default();
  general::check(options, context, &mut report);
  let downloads = options
    .directories
    .downloads
    .clone()
    .unwrap_or_else(|| context.default_downloads.clone());
  let case_sensitive = context.started_case_sensitive;
  report.0.extend(reseek_shares::validate::check(
    &options.shares,
    &downloads,
    case_sensitive,
  ));
  transfers::check(&options.transfers, case_sensitive, &mut report);
  web::check(&options.web, context, &mut report);
  soulseek::check(&options.soulseek, &mut report);
  scripts::check(&options.integrations, &mut report);
  analysis::check(&options.analysis, &mut report);
  if report.0.is_empty() {
    Ok(())
  } else {
    Err(report.0)
  }
}

/// The problems found so far.
#[derive(Debug, Default)]
pub(crate) struct Report(Vec<Invalid>);

impl Report {
  /// Records a problem with the value of `key`.
  pub(crate) fn add(&mut self, key: &str, message: impl Into<String>) {
    self.0.push(Invalid::new(key, message));
  }

  /// Records `result`'s problem, if any, under `key`.
  pub(crate) fn check(&mut self, key: &str, result: Result<(), String>) {
    if let Err(message) = result {
      self.add(key, message);
    }
  }

  #[cfg(test)]
  pub(crate) fn messages(&self) -> Vec<String> {
    self.0.iter().map(|i| i.message.clone()).collect()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn context(dir: &std::path::Path) -> Context {
    Context {
      default_downloads: dir.join("downloads"),
      default_incomplete: dir.join("incomplete"),
      program_dir: dir.to_path_buf(),
      started_case_sensitive: false,
    }
  }

  #[test]
  fn the_defaults_are_valid() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(validate(&Options::default(), &context(dir.path())), Ok(()));
  }

  #[test]
  fn patterns_are_checked_with_the_case_sensitivity_the_daemon_started_with() {
    // compiles matching case, and is too large to compile ignoring it
    let pattern = "[a-z]{50000}".to_string();
    let dir = tempfile::tempdir().unwrap();
    let mut options = Options::default();
    options.shares.filters = vec![pattern.clone()];
    options.transfers.groups.blacklisted.patterns = vec![pattern.clone()];
    options.filters.search.request = vec![pattern];
    let started_sensitive = Context {
      started_case_sensitive: true,
      ..context(dir.path())
    };
    assert_eq!(validate(&options, &started_sensitive), Ok(()));
    options.flags.case_sensitive_regex = true;
    let invalid = validate(&options, &context(dir.path())).unwrap_err();
    let keys: Vec<&str> = invalid.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(
      keys,
      [
        "filters.search.request",
        "shares.filters",
        "transfers.groups.blacklisted.patterns"
      ],
      "{invalid:?}"
    );
  }

  #[test]
  fn every_problem_is_reported_under_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = Options::default();
    options.web.https.port = 0;
    options.soulseek.listen_port = 80;
    options.shares.filters = vec!["(".into()];
    let invalid = validate(&options, &context(dir.path())).unwrap_err();
    let keys: Vec<&str> = invalid.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(
      keys,
      ["shares.filters", "web.https.port", "soulseek.listen_port"],
      "{invalid:?}"
    );
    assert_eq!(
      invalid[1].message,
      "The field web.https.port must be between 1 and 65535."
    );
  }
}
