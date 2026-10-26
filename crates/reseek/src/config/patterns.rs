//! The regular expressions users write in the configuration (share filters, search request
//! filters and blacklisted name patterns), all compiled one way so what validation accepts is
//! what the engines run.

use regex::{Regex, RegexBuilder};

/// How large a compiled pattern may grow.
const SIZE_LIMIT: usize = 10 << 20;

/// Compiles a user's pattern, matching ignoring case unless `case_sensitive`; a blank pattern
/// is refused.
pub fn compile(pattern: &str, case_sensitive: bool) -> Result<Regex, String> {
  if pattern.trim().is_empty() {
    return Err("the pattern is blank".to_string());
  }
  RegexBuilder::new(pattern)
    .case_insensitive(!case_sensitive)
    .size_limit(SIZE_LIMIT)
    .build()
    .map_err(|e| e.to_string())
}
