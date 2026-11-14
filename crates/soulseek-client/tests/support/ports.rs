//! Listening ports for tests: free ones no other test process can take between their pick and
//! the bind that uses them. Shared with the daemon's in-memory tests and the program tests.

use std::net::Ipv4Addr;
use std::sync::{Mutex, PoisonError};

/// where the tests take listening ports from: below the range Linux hands out for outgoing
/// connections, so no connection a test makes can take a port between its pick and its bind
const PORTS: std::ops::Range<u32> = 20_000..32_000;
/// ports each test process takes from before running into the next process's
const PORTS_PER_PROCESS: u32 = 10;

/// A port nothing listens on right now and no other test process holds. Each test process
/// takes them in order from a place of its own in [`PORTS`], so processes running side by side
/// rarely look at the same one, and claims each with a lock file, so two test runs alive at
/// once never both pick it.
pub fn free_port() -> u16 {
  static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicU32> = std::sync::OnceLock::new();
  let len = PORTS.end - PORTS.start;
  let next = NEXT.get_or_init(|| {
    let slot = process_slot() % (len / PORTS_PER_PROCESS);
    std::sync::atomic::AtomicU32::new(slot * PORTS_PER_PROCESS)
  });
  loop {
    let offset = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % len;
    let port = u16::try_from(PORTS.start + offset).expect("the range is below 65536");
    // a port something else holds is skipped
    if std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok() && claim(port) {
      return port;
    }
  }
}

/// Claims `port` for this process until it exits; false when another process holds it. The
/// claim is a lock on a file named after the port in a directory every test process on the
/// machine shares, which the system lets go of when the process ends, however it ends.
fn claim(port: u16) -> bool {
  static CLAIMS: Mutex<Vec<std::fs::File>> = Mutex::new(Vec::new());
  // a temporary directory of the machine's, not the shell's: nix develop gives every shell one
  // of its own
  let dir = if cfg!(unix) {
    std::path::PathBuf::from("/tmp")
  } else {
    std::env::temp_dir()
  }
  .join("soulseek-test-ports");
  std::fs::create_dir_all(&dir).unwrap();
  let file = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .write(true)
    .open(dir.join(port.to_string()))
    .unwrap();
  match file.try_lock() {
    Ok(()) => {
      CLAIMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(file);
      true
    }
    Err(std::fs::TryLockError::WouldBlock) => false,
    Err(std::fs::TryLockError::Error(e)) => panic!("port {port} could not be claimed: {e}"),
  }
}

/// The number that places this process in [`PORTS`]. nextest gives each test running at once a
/// slot no other running test has, though two nextest runs alive at once both start at slot 0;
/// under `cargo test` the process id stands in, which two live processes can share once it is
/// reduced to a place. The port claims keep such processes apart.
fn process_slot() -> u32 {
  match std::env::var("NEXTEST_TEST_GLOBAL_SLOT") {
    Ok(slot) => slot.parse().expect("nextest's slot is a number"),
    Err(_) => std::process::id(),
  }
}
