//! Which agents a controller takes by their version. An agent says its version when it logs in,
//! and the controller refuses one outside its release line: the versions that agree on the
//! major number and, before 1.0, on the minor number too, which is what semantic versioning
//! calls compatible. The patch number and any pre-release or build part may differ. The frame
//! protocol number, checked on every frame, is the wire's own contract; the release line also
//! keeps the share index, the HTTP calls and their behavior in step.

/// Whether an agent of version `agent` may relay through a controller of version `controller`:
/// both read as `major.minor.patch`, and they are on the same release line. A version that does
/// not read is on no line.
pub fn compatible(controller: &str, agent: &str) -> bool {
  match (release_line(controller), release_line(agent)) {
    (Some(ours), Some(theirs)) => ours == theirs,
    _ => false,
  }
}

/// The release line of `version`: its major number, and its minor number while the major one
/// is 0; `None` when it is not `major.minor.patch` with an optional `-pre` or `+build` part.
fn release_line(version: &str) -> Option<(u64, Option<u64>)> {
  let core = version.split(['-', '+']).next()?;
  let mut numbers = core.split('.').map(|part| {
    // a leading zero or a sign is not a version number
    let plain = !part.is_empty()
      && part.bytes().all(|b| b.is_ascii_digit())
      && (part == "0" || !part.starts_with('0'));
    plain.then(|| part.parse::<u64>().ok()).flatten()
  });
  let (major, minor, _patch) = (numbers.next()??, numbers.next()??, numbers.next()??);
  if numbers.next().is_some() {
    return None;
  }
  Some((major, (major == 0).then_some(minor)))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_agent_relays_within_its_controllers_release_line() {
    assert!(compatible("0.1.0", "0.1.7"));
    assert!(compatible("0.1.3", "0.1.0-rc1"));
    assert!(compatible("1.2.0", "1.9.3+abc"));
    assert!(
      !compatible("0.1.0", "0.2.0"),
      "before 1.0 the minor is the line"
    );
    assert!(!compatible("1.4.0", "2.4.0"));
    for unreadable in ["test", "", "0.1", "0.1.0.0", "v0.1.0", "0.01.0", "0.1.x"] {
      assert!(!compatible("0.1.0", unreadable), "{unreadable:?}");
      assert!(!compatible(unreadable, "0.1.0"), "{unreadable:?}");
    }
  }
}
