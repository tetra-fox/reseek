//! A directory as a zip archive, written while it is sent: every file stored as it is, in the
//! order the directory is walked, each followed by its checksum and size, with the ZIP64 forms
//! for a file, an offset or a count too large for the classic ones. Hidden entries and links
//! are left out, and so is a file that cannot be opened.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use axum::body::Bytes;
use chrono::{DateTime, Datelike, Timelike, Utc};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::entries;

/// The largest size, offset and count the classic zip fields hold.
const CLASSIC: u64 = u32::MAX as u64;

/// How much of a file one read takes.
const CHUNK: usize = 256 * 1024;

/// One file the archive holds.
#[derive(Debug)]
pub(super) struct Member {
  /// where it is
  pub path: PathBuf,
  /// its name in the archive, relative to the archived directory, `/`-separated
  pub name: String,
  /// its size when the directory was walked
  pub size: u64,
  /// when it last changed
  pub modified: Option<SystemTime>,
}

/// Every file under `dir` the archive holds, in walk order: each directory's files and
/// subdirectories by name, a subdirectory's contents where it comes.
pub(super) fn members(dir: &Path) -> io::Result<Vec<Member>> {
  let mut found = Vec::new();
  walk(dir, "", &mut found, true)?;
  Ok(found)
}

fn walk(dir: &Path, prefix: &str, found: &mut Vec<Member>, top: bool) -> io::Result<()> {
  let entries = match std::fs::read_dir(dir) {
    Ok(entries) => entries,
    Err(e) if !top => {
      debug!(dir = %dir.display(), error = %e, "unreadable directory left out of an archive");
      return Ok(());
    }
    Err(e) => return Err(e),
  };
  let mut entries: Vec<_> = entries
    .filter_map(|e| super::roots::entry_of(e, dir, "archive"))
    .collect();
  entries.sort_by_key(|entry| entry.file_name());
  for entry in entries {
    let path = entry.path();
    let Some(metadata) = super::roots::metadata_of(&path, "archive") else {
      continue;
    };
    if metadata.file_type().is_symlink() || entries::left_out(&entry.file_name(), Some(&metadata)) {
      continue;
    }
    let name = entry.file_name().to_string_lossy().into_owned();
    let inner = if prefix.is_empty() {
      name
    } else {
      format!("{prefix}/{name}")
    };
    if metadata.is_dir() {
      walk(&path, &inner, found, false)?;
    } else if metadata.is_file() {
      found.push(Member {
        path,
        name: inner,
        size: metadata.len(),
        modified: metadata.modified().ok(),
      });
    }
  }
  Ok(())
}

/// What the writer keeps of an entry until its central record is written.
struct Entry {
  name: Vec<u8>,
  offset: u64,
  time: u16,
  date: u16,
  crc: u32,
  size: u64,
  /// the entry's local header has the ZIP64 sizes
  large: bool,
}

/// The bytes of a zip archive, made one piece at a time.
pub(super) struct Writer {
  /// the largest value written in a classic field; ZIP64 above it
  classic: u64,
  /// bytes written so far
  offset: u64,
  entries: Vec<Entry>,
}

fn u16le(out: &mut Vec<u8>, value: u16) {
  out.extend_from_slice(&value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u32) {
  out.extend_from_slice(&value.to_le_bytes());
}

fn u64le(out: &mut Vec<u8>, value: u64) {
  out.extend_from_slice(&value.to_le_bytes());
}

/// `value` in a classic field: itself when it fits, else the mark that the ZIP64 field holds it.
fn classic(value: u64, limit: u64) -> u32 {
  if value >= limit {
    u32::MAX
  } else {
    value as u32
  }
}

/// A time as the zip format writes it, in its two DOS fields; before 1980 is 1980.
fn dos(at: Option<SystemTime>) -> (u16, u16) {
  let at: DateTime<Utc> = at.map(DateTime::from).unwrap_or_default();
  if at.year() < 1980 {
    return (0, (1 << 5) | 1);
  }
  let time = (at.hour() << 11) | (at.minute() << 5) | (at.second() / 2);
  let date = (((at.year() - 1980) as u32) << 9) | (at.month() << 5) | at.day();
  (time as u16, date as u16)
}

/// Flags of every entry: sizes after the data, names in UTF-8.
const FLAGS: u16 = 0x0808;

impl Writer {
  /// A writer of an archive using the ZIP64 forms above the classic fields' limits.
  pub fn new() -> Self {
    Self::with_limit(CLASSIC)
  }

  /// A writer using the ZIP64 forms for values of `limit` and above.
  fn with_limit(limit: u64) -> Self {
    Self {
      classic: limit,
      offset: 0,
      entries: Vec::new(),
    }
  }

  fn written(&mut self, bytes: Vec<u8>) -> Vec<u8> {
    self.offset += bytes.len() as u64;
    bytes
  }

  /// The local header of a file named `name` of about `size` bytes.
  pub fn start(&mut self, name: &str, size: u64, modified: Option<SystemTime>) -> Vec<u8> {
    let large = size >= self.classic;
    let (time, date) = dos(modified);
    let mut out = Vec::new();
    u32le(&mut out, 0x0403_4b50);
    u16le(&mut out, if large { 45 } else { 20 });
    u16le(&mut out, FLAGS);
    u16le(&mut out, 0);
    u16le(&mut out, time);
    u16le(&mut out, date);
    u32le(&mut out, 0);
    let unknown = if large { u32::MAX } else { 0 };
    u32le(&mut out, unknown);
    u32le(&mut out, unknown);
    u16le(&mut out, name.len() as u16);
    u16le(&mut out, if large { 20 } else { 0 });
    out.extend_from_slice(name.as_bytes());
    if large {
      u16le(&mut out, 0x0001);
      u16le(&mut out, 16);
      u64le(&mut out, 0);
      u64le(&mut out, 0);
    }
    self.entries.push(Entry {
      name: name.as_bytes().to_vec(),
      offset: self.offset,
      time,
      date,
      crc: 0,
      size: 0,
      large,
    });
    self.written(out)
  }

  /// Counts `bytes` of the current file's data.
  pub fn data(&mut self, bytes: usize) {
    self.offset += bytes as u64;
  }

  /// The data descriptor that ends the current file, whose data had the checksum `crc` and
  /// `size` bytes.
  pub fn end(&mut self, crc: u32, size: u64) -> Vec<u8> {
    let entry = self.entries.last_mut().expect("a file was started");
    entry.crc = crc;
    entry.size = size;
    let mut out = Vec::new();
    u32le(&mut out, 0x0807_4b50);
    u32le(&mut out, crc);
    if entry.large {
      u64le(&mut out, size);
      u64le(&mut out, size);
    } else {
      u32le(&mut out, size as u32);
      u32le(&mut out, size as u32);
    }
    self.written(out)
  }

  /// The central directory and the end records.
  pub fn finish(mut self) -> Vec<u8> {
    let limit = self.classic;
    let start = self.offset;
    let mut out = Vec::new();
    for entry in &self.entries {
      let sizes_large = entry.large || entry.size >= limit;
      let offset_large = entry.offset >= limit;
      let mut extra = Vec::new();
      if sizes_large {
        u64le(&mut extra, entry.size);
        u64le(&mut extra, entry.size);
      }
      if offset_large {
        u64le(&mut extra, entry.offset);
      }
      let large = sizes_large || offset_large;
      u32le(&mut out, 0x0201_4b50);
      u16le(&mut out, (3 << 8) | 45);
      u16le(&mut out, if large { 45 } else { 20 });
      u16le(&mut out, FLAGS);
      u16le(&mut out, 0);
      u16le(&mut out, entry.time);
      u16le(&mut out, entry.date);
      u32le(&mut out, entry.crc);
      let size = if sizes_large {
        u32::MAX
      } else {
        entry.size as u32
      };
      u32le(&mut out, size);
      u32le(&mut out, size);
      u16le(&mut out, entry.name.len() as u16);
      u16le(&mut out, if large { extra.len() as u16 + 4 } else { 0 });
      u16le(&mut out, 0);
      u16le(&mut out, 0);
      u16le(&mut out, 0);
      // a regular file readable by everyone, as the system that made it writes it
      u32le(&mut out, 0o100_644 << 16);
      u32le(&mut out, classic(entry.offset, limit));
      out.extend_from_slice(&entry.name);
      if large {
        u16le(&mut out, 0x0001);
        u16le(&mut out, extra.len() as u16);
        out.extend_from_slice(&extra);
      }
    }
    let size = out.len() as u64;
    let count = self.entries.len() as u64;
    let end_large = count >= 0xffff.min(limit) || start >= limit || size >= limit;
    if end_large {
      let record = start + size;
      u32le(&mut out, 0x0606_4b50);
      u64le(&mut out, 44);
      u16le(&mut out, (3 << 8) | 45);
      u16le(&mut out, 45);
      u32le(&mut out, 0);
      u32le(&mut out, 0);
      u64le(&mut out, count);
      u64le(&mut out, count);
      u64le(&mut out, size);
      u64le(&mut out, start);
      u32le(&mut out, 0x0706_4b50);
      u32le(&mut out, 0);
      u64le(&mut out, record);
      u32le(&mut out, 1);
    }
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    let classic_count = if end_large { 0xffff } else { count as u16 };
    u16le(&mut out, classic_count);
    u16le(&mut out, classic_count);
    u32le(&mut out, classic(size, if end_large { 0 } else { limit }));
    u32le(&mut out, classic(start, if end_large { 0 } else { limit }));
    u16le(&mut out, 0);
    self.written(out)
  }
}

/// Writes the archive of `members` into `sender` until it is done or the receiver is gone.
pub(super) async fn send(members: Vec<Member>, sender: mpsc::Sender<io::Result<Bytes>>) {
  let mut writer = Writer::new();
  let send = |bytes: Vec<u8>| sender.send(Ok(Bytes::from(bytes)));
  for member in members {
    let mut file = match tokio::fs::File::open(&member.path).await {
      Ok(file) => file,
      Err(e) => {
        debug!(path = %member.path.display(), error = %e, "file left out of an archive");
        continue;
      }
    };
    if send(writer.start(&member.name, member.size, member.modified))
      .await
      .is_err()
    {
      return;
    }
    let mut crc = crc32fast::Hasher::new();
    let mut read_total = 0u64;
    // read what the walk saw, so a file that grows does not outgrow the sizes written
    while read_total < member.size {
      let want = CHUNK.min(usize::try_from(member.size - read_total).unwrap_or(CHUNK));
      let mut buffer = vec![0; want];
      match file.read(&mut buffer).await {
        Ok(0) => break,
        Ok(read) => {
          buffer.truncate(read);
          crc.update(&buffer);
          read_total += read as u64;
          writer.data(read);
          if send(buffer).await.is_err() {
            return;
          }
        }
        Err(e) => {
          warn!(path = %member.path.display(), error = %e, "an archive stopped at a file that could not be read");
          // the archive can not be finished whole, so the client is told it broke off
          drop(sender.send(Err(e)).await);
          return;
        }
      }
    }
    if send(writer.end(crc.finalize(), read_total)).await.is_err() {
      return;
    }
  }
  drop(send(writer.finish()).await);
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Read;

  fn archive(writer: &mut Writer, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in files {
      out.extend(writer.start(name, data.len() as u64, None));
      out.extend_from_slice(data);
      writer.data(data.len());
      out.extend(writer.end(crc32fast::hash(data), data.len() as u64));
    }
    out
  }

  fn read_back(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..zip.len())
      .map(|i| {
        let mut entry = zip.by_index(i).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        (entry.name().to_string(), data)
      })
      .collect()
  }

  #[test]
  fn an_archive_stores_its_files_and_reads_back() {
    let mut writer = Writer::new();
    let mut bytes = archive(
      &mut writer,
      &[("01.flac", b"first"), ("Scans/cover.jpg", b"")],
    );
    bytes.extend(writer.finish());
    let read = read_back(bytes);
    assert_eq!(
      read,
      [
        ("01.flac".to_string(), b"first".to_vec()),
        ("Scans/cover.jpg".to_string(), Vec::new())
      ]
    );
  }

  #[test]
  fn large_sizes_offsets_and_counts_take_the_zip64_forms() {
    // with the limit at 4 bytes every field past it is written as ZIP64
    let mut writer = Writer::with_limit(4);
    let mut bytes = archive(
      &mut writer,
      &[("a", b"tiny"), ("b", b"larger than four"), ("c", b"x")],
    );
    bytes.extend(writer.finish());
    let read = read_back(bytes);
    let names: Vec<_> = read.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert_eq!(read[1].1, b"larger than four");
  }

  #[test]
  fn a_walk_leaves_out_hidden_files_and_links_in_name_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("b/c")).unwrap();
    std::fs::write(root.join("b/c/3"), b"").unwrap();
    std::fs::write(root.join("b/2"), b"").unwrap();
    std::fs::write(root.join("a"), b"").unwrap();
    std::fs::write(root.join(".hidden"), b"").unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/x"), b"").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("a"), root.join("link")).unwrap();
    let names: Vec<_> = members(root).unwrap().into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["a", "b/2", "b/c/3"]);
  }

  #[test]
  fn dos_times_start_in_1980() {
    assert_eq!(dos(None), (0, 33));
    let at: DateTime<Utc> = "2026-10-25T12:30:10Z".parse().unwrap();
    let (time, date) = dos(Some(at.into()));
    assert_eq!(time, (12 << 11) | (30 << 5) | 5);
    assert_eq!(date, ((2026 - 1980) << 9) | (10 << 5) | 25);
  }
}
