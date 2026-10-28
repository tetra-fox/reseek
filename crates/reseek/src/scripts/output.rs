//! What a run printed: both of its streams are read to their end while it runs, so it never
//! stops on a full pipe, and only the end of what they said together is kept.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::sync::lock;

/// How many lines of output are kept.
pub const LINES: usize = 20;

/// How many bytes of output are kept.
pub const BYTES: usize = 4096;

/// The last lines of a run's output, both streams together in the order their lines were read,
/// at most [`LINES`] of them and [`BYTES`] long, the newest kept.
#[derive(Debug, Default)]
pub struct Tail {
  lines: VecDeque<String>,
  bytes: usize,
}

impl Tail {
  /// Adds one line, dropping the oldest past the limits. A line longer than all of them keeps
  /// its end.
  pub fn push(&mut self, mut line: String) {
    if line.len() > BYTES {
      let mut cut = line.len() - BYTES;
      while !line.is_char_boundary(cut) {
        cut += 1;
      }
      line.drain(..cut);
    }
    self.bytes += line.len();
    self.lines.push_back(line);
    while self.lines.len() > LINES || self.bytes + self.lines.len() - 1 > BYTES {
      let dropped = self.lines.pop_front().expect("a line was just added");
      self.bytes -= dropped.len();
    }
  }

  /// The lines kept, joined by newlines.
  pub fn text(&self) -> String {
    Vec::from(self.lines.clone()).join("\n")
  }
}

/// Reads `stream` to its end into `tail` line by line, and returns how many bytes it gave.
pub async fn drain(mut stream: impl AsyncRead + Unpin, tail: Arc<Mutex<Tail>>) -> u64 {
  let mut chunk = vec![0; 8192];
  let mut line = Vec::new();
  let mut read = 0u64;
  loop {
    let n = match stream.read(&mut chunk).await {
      // a pipe that fails to read has nothing more to give, as at its end
      Ok(0) | Err(_) => break,
      Ok(n) => n,
    };
    read += n as u64;
    for &byte in &chunk[..n] {
      if byte == b'\n' {
        lock(&tail).push(text(&line));
        line.clear();
      } else {
        line.push(byte);
      }
    }
    // a line that never ends keeps only what the tail could show of it
    if line.len() > 2 * BYTES {
      line.drain(..line.len() - BYTES);
    }
  }
  if !line.is_empty() {
    lock(&tail).push(text(&line));
  }
  read
}

/// A line as text, without the carriage return of a Windows line end.
fn text(line: &[u8]) -> String {
  let line = line.strip_suffix(b"\r").unwrap_or(line);
  String::from_utf8_lossy(line).into_owned()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_tail_keeps_the_newest_twenty_lines_within_four_kibibytes() {
    let mut tail = Tail::default();
    for n in 0..25 {
      tail.push(format!("line {n}"));
    }
    let text = tail.text();
    assert!(
      text.starts_with("line 5\n") && text.ends_with("\nline 24"),
      "{text}"
    );
    for _ in 0..3 {
      tail.push("x".repeat(1500));
    }
    assert_eq!(tail.text(), ["x".repeat(1500), "x".repeat(1500)].join("\n"));
    tail.push(format!("{}é{}", "a".repeat(5000), "b".repeat(100)));
    let text = tail.text();
    assert!(
      text.len() <= BYTES && text.ends_with(&"b".repeat(100)),
      "the end of a long line"
    );
  }

  #[tokio::test]
  async fn both_streams_are_read_line_by_line_to_their_end() {
    let tail = Arc::new(Mutex::new(Tail::default()));
    let out: &[u8] = b"one\r\ntwo\nthree";
    let read = drain(out, tail.clone()).await;
    assert_eq!(read, 14);
    let err: &[u8] = b"oops\n";
    drain(err, tail.clone()).await;
    assert_eq!(lock(&tail).text(), "one\ntwo\nthree\noops");
  }
}
