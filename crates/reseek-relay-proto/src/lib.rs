//! The relay protocol between a reseek controller and its agents, as both sides speak it: the
//! socket's frames, close codes and timings ([`frames`]), the credentials an agent proves its
//! secret with ([`credential`]), which versions may relay to each other ([`version`]), what an
//! agent may be named ([`check_instance_name`], [`LOCAL_HOST`]), and how a loop that keeps failing waits before
//! it tries again ([`backoff`]).
//!
//! The messages themselves are in `api_types::relay`, with the rest of the API's types.

#![warn(missing_docs)]

pub mod backoff;
pub mod credential;
pub mod frames;
pub mod version;

/// The host name of the controller's own shares, which no agent may take: a file found under
/// it is read from the controller's disk.
pub const LOCAL_HOST: &str = "local";

/// The characters an instance name may not hold, besides control characters and anything
/// outside printable ASCII: a controller names a file after each agent, on any system.
const NOT_IN_A_NAME: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Whether the agent instance name `name`, the value of the option `key`, can name a file on
/// any system and is free to be an agent's: printable ASCII without separators or the
/// characters Windows refuses, not `.` or `..`, and not [`LOCAL_HOST`]. Says what is wrong when
/// it is not.
pub fn check_instance_name(key: &str, name: &str) -> Result<(), String> {
  let unfit = name
    .chars()
    .any(|c| !(' '..='~').contains(&c) || NOT_IN_A_NAME.contains(&c));
  if unfit || name == "." || name == ".." {
    return Err(format!(
      "The field {key} must be printable ASCII without / \\ : * ? \" < > | and must not be '.' or '..'."
    ));
  }
  if name == LOCAL_HOST {
    return Err(format!(
      "The field {key} must not be '{LOCAL_HOST}', which names the controller's own shares."
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_instance_name_must_name_a_file_and_not_the_controller() {
    for name in ["nas", "the agent", "a.b-c_d"] {
      assert!(check_instance_name("n", name).is_ok(), "{name}");
    }
    for name in [
      ".",
      "..",
      "a/b",
      "a\\b",
      "c:",
      "nul\0",
      "tab\there",
      "naïve",
      "a|b",
      LOCAL_HOST,
    ] {
      assert!(check_instance_name("n", name).is_err(), "{name:?}");
    }
  }
}
