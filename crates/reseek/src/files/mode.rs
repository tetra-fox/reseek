//! Unix permission modes as the configuration writes them, such as `"0644"`, for the files and
//! directories downloads create.

/// The mode an octal text of three or four digits gives; a fourth leading digit carries the
/// setuid, setgid and sticky bits.
pub fn to_unix_file_mode(text: &str) -> Result<u32, String> {
  if text.trim().is_empty() {
    return Err("a permission mode may not be empty".to_string());
  }
  let digits = text.len();
  if !(3..=4).contains(&digits) || !text.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
    return Err(format!(
      "The provided permissions are not a valid, expected 0-7 repeated 3 or 4 times (provided: {text})"
    ));
  }
  u32::from_str_radix(text, 8).map_err(|e| e.to_string())
}

/// `mode` with the execute bit of the owner, the group and others set wherever their read bit
/// is, as a directory needs to be entered.
pub fn with_execute_for_each_read(mode: u32) -> u32 {
  let mut mode = mode;
  for (read, execute) in [(0o400, 0o100), (0o040, 0o010), (0o004, 0o001)] {
    if mode & read != 0 {
      mode |= execute;
    }
  }
  mode
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn modes_are_three_or_four_octal_digits() {
    assert_eq!(to_unix_file_mode("0644").unwrap(), 0o644);
    assert_eq!(to_unix_file_mode("644").unwrap(), 0o644);
    assert_eq!(to_unix_file_mode("0777").unwrap(), 0o777);
    assert_eq!(to_unix_file_mode("1000").unwrap(), 0o1000, "sticky");
    assert_eq!(to_unix_file_mode("7777").unwrap(), 0o7777);
    assert!(to_unix_file_mode("").is_err());
    for bad in ["0", "00", "00000", "foo bar baz", "0648"] {
      let error = to_unix_file_mode(bad).unwrap_err();
      assert!(
        error.contains("expected 0-7 repeated 3 or 4 times"),
        "{bad}: {error}"
      );
    }
  }

  #[test]
  fn readable_means_enterable() {
    assert_eq!(with_execute_for_each_read(0o644), 0o755);
    assert_eq!(with_execute_for_each_read(0o222), 0o222);
    assert_eq!(with_execute_for_each_read(0), 0);
    assert_eq!(with_execute_for_each_read(0o4640), 0o4750);
  }
}
