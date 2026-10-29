//! A report's spectrogram and waveform on disk, one file a report named after it: the overviews
//! and the window spectra as they are served, then every window's columns and buckets at full
//! detail, from which tiles are cut when asked for. The file is written whole under another name
//! and renamed, so a reader never sees half of one.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use api_types::analysis::{SpectrogramTile, WaveformTile, WindowSpectra};
use serde::Serialize;
use tracing::{debug, info, warn};

use super::analyzer::measure::Window;
use super::analyzer::picture::{BINS, CEILING_DB, COLUMNS_PER_SECOND, FLOOR_DB, PEAKS_PER_SECOND};
use super::tiles::{self, Cut, Kind, Second};

/// What the file starts with: its kind and the version of its layout.
const MAGIC: &[u8; 8] = b"reseek\x00\x01";
/// The bytes before the window table: the magic, the window count, the length and the top of
/// the band, and the lengths of the three answers kept whole.
const HEAD: usize = 8 + 4 + 8 + 8 + 4 * 3;
/// The bytes of one window's entry in the table: the second it starts at, and how many columns
/// and buckets it has.
const ENTRY: usize = 8;
/// The bytes one window's columns take, the last window's padded to a whole second.
const COLUMN_SLOT: usize = COLUMNS_PER_SECOND * BINS;
/// The bytes one window's buckets take, the last window's padded to a whole second.
const PEAK_SLOT: usize = PEAKS_PER_SECOND * 2;
/// What a file a report's pictures are kept in ends with.
const EXTENSION: &str = "spectra";

/// The file a report's pictures are kept in under `dir`.
pub(super) fn path(dir: &Path, name: &str) -> PathBuf {
  dir.join(format!("{name}.{EXTENSION}"))
}

/// An answer that is binary: the header's length as four little-endian bytes, the header as
/// JSON, then `body`.
pub(super) fn framed(header: &impl Serialize, body: &[u8]) -> Vec<u8> {
  // the headers are numbers and lists of numbers, which always serialize
  let header = serde_json::to_vec(header).expect("a header serializes");
  let mut out = Vec::with_capacity(4 + header.len() + body.len());
  out.extend((header.len() as u32).to_le_bytes());
  out.extend(header);
  out.extend(body);
  out
}

/// The bytes after the header of an answer [`framed`] made.
pub(super) fn body(answer: &[u8]) -> Option<&[u8]> {
  let length = u32::from_le_bytes(answer.get(..4)?.try_into().ok()?) as usize;
  answer.get(4 + length..)
}

fn spectrogram_answer(cut: &Cut, max_hz: f64) -> Vec<u8> {
  let header = SpectrogramTile {
    from_seconds: cut.from,
    to_seconds: cut.to,
    columns: cut.count as u32,
    bins: BINS as u32,
    max_hz,
    min_db: FLOOR_DB,
    max_db: CEILING_DB,
    unread: cut.unread.clone(),
  };
  framed(&header, &cut.bytes)
}

fn waveform_answer(cut: &Cut) -> Vec<u8> {
  let header = WaveformTile {
    from_seconds: cut.from,
    to_seconds: cut.to,
    peaks: cut.count as u32,
    unread: cut.unread.clone(),
  };
  framed(&header, &cut.bytes)
}

/// The file's contents for `windows` of audio `duration` seconds long whose band ends at
/// `max_hz`.
pub(super) fn contents(windows: &[Window], duration: f64, max_hz: f64) -> Vec<u8> {
  let seconds: Vec<Second<'_>> = windows
    .iter()
    .map(|w| Second {
      at: w.t as u32,
      columns: &w.shown.columns,
      peaks: &w.shown.peaks,
    })
    .collect();
  let spectrogram = spectrogram_answer(
    &tiles::overview(Kind::Spectrogram, &seconds, duration),
    max_hz,
  );
  let waveform = waveform_answer(&tiles::overview(Kind::Waveform, &seconds, duration));
  let spectra_header = WindowSpectra {
    windows: windows.len() as u32,
    bins: BINS as u32,
    max_hz,
    min_db: FLOOR_DB,
    max_db: CEILING_DB,
  };
  let spectra: Vec<u8> = windows
    .iter()
    .flat_map(|w| w.shown.spectrum.iter().copied())
    .collect();
  let spectra = framed(&spectra_header, &spectra);

  let mut out = Vec::new();
  out.extend(MAGIC);
  out.extend((windows.len() as u32).to_le_bytes());
  out.extend(duration.to_le_bytes());
  out.extend(max_hz.to_le_bytes());
  for answer in [&spectrogram, &waveform, &spectra] {
    out.extend((answer.len() as u32).to_le_bytes());
  }
  for second in &seconds {
    out.extend(second.at.to_le_bytes());
    out.extend(((second.columns.len() / BINS) as u16).to_le_bytes());
    out.extend(((second.peaks.len() / 2) as u16).to_le_bytes());
  }
  for answer in [&spectrogram, &waveform, &spectra] {
    out.extend(answer.iter());
  }
  for second in &seconds {
    out.extend(second.columns);
    out.extend(std::iter::repeat_n(0, COLUMN_SLOT - second.columns.len()));
  }
  for second in &seconds {
    out.extend(second.peaks);
    out.extend(std::iter::repeat_n(0, PEAK_SLOT - second.peaks.len()));
  }
  out
}

/// Writes a report's pictures to their file under `dir`, creating it; returns the bytes the file
/// takes.
pub(super) fn write(dir: &Path, name: &str, contents: &[u8]) -> Result<u64, String> {
  std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  let path = path(dir, name);
  let part = dir.join(format!("{name}.{EXTENSION}.part"));
  std::fs::write(&part, contents).map_err(|e| format!("{}: {e}", part.display()))?;
  std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))?;
  Ok(contents.len() as u64)
}

/// A piece of the file a viewer asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asked {
  /// the overview of one kind
  Overview(Kind),
  /// a tile of one kind at full detail
  Tile(Kind, u32),
  /// every window's spectrum
  Spectra,
}

/// What reading the file found wrong with it.
#[derive(Debug)]
pub(super) enum Unkept {
  /// there is no file
  Missing,
  /// the file is not one this version wrote, or not whole
  Broken(String),
}

/// One window's entry in the table.
struct Entry {
  /// the whole second it starts at
  at: u32,
  /// how many of its slot's columns it fills
  columns: usize,
  /// how many of its slot's buckets it fills
  peaks: usize,
}

/// The head of an open file, with its window table.
struct Head {
  windows: usize,
  duration: f64,
  max_hz: f64,
  /// the lengths of the overviews and the window spectra
  kept: [usize; 3],
  entries: Vec<Entry>,
}

impl Head {
  fn table(&self) -> u64 {
    HEAD as u64
  }

  fn answers(&self) -> u64 {
    self.table() + (self.windows * ENTRY) as u64
  }

  fn columns(&self) -> u64 {
    self.answers() + self.kept.iter().sum::<usize>() as u64
  }

  fn peaks(&self) -> u64 {
    self.columns() + (self.windows * COLUMN_SLOT) as u64
  }

  fn end(&self) -> u64 {
    self.peaks() + (self.windows * PEAK_SLOT) as u64
  }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
  u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn read_at(file: &mut File, at: u64, length: usize) -> std::io::Result<Vec<u8>> {
  file.seek(SeekFrom::Start(at))?;
  let mut bytes = vec![0; length];
  file.read_exact(&mut bytes)?;
  Ok(bytes)
}

fn open(path: &Path) -> Result<(File, Head), Unkept> {
  let broken = |e: std::io::Error| Unkept::Broken(e.to_string());
  let mut file = match File::open(path) {
    Ok(file) => file,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Unkept::Missing),
    Err(e) => return Err(broken(e)),
  };
  let head = read_at(&mut file, 0, HEAD).map_err(broken)?;
  if &head[..8] != MAGIC {
    return Err(Unkept::Broken("not a file this version keeps".to_string()));
  }
  let f64_at = |at: usize| f64::from_le_bytes(head[at..at + 8].try_into().expect("eight bytes"));
  let mut head = Head {
    windows: u32_at(&head, 8) as usize,
    duration: f64_at(12),
    max_hz: f64_at(20),
    kept: [28, 32, 36].map(|at| u32_at(&head, at) as usize),
    entries: Vec::new(),
  };
  let length = file.metadata().map_err(broken)?.len();
  if length != head.end() {
    return Err(Unkept::Broken(format!(
      "{length} bytes where its head says {}",
      head.end()
    )));
  }
  let table = read_at(&mut file, head.table(), head.windows * ENTRY).map_err(broken)?;
  for entry in table.as_chunks::<ENTRY>().0 {
    let entry = Entry {
      at: u32_at(entry, 0),
      columns: usize::from(u16::from_le_bytes([entry[4], entry[5]])),
      peaks: usize::from(u16::from_le_bytes([entry[6], entry[7]])),
    };
    // tiles slice each slot by these counts and find windows by their seconds in order
    let in_order = head
      .entries
      .last()
      .is_none_or(|before| before.at < entry.at);
    if entry.columns > COLUMNS_PER_SECOND || entry.peaks > PEAKS_PER_SECOND || !in_order {
      return Err(Unkept::Broken(format!(
        "its window table has a window at {} s with {} columns and {} buckets",
        entry.at, entry.columns, entry.peaks
      )));
    }
    head.entries.push(entry);
  }
  Ok((file, head))
}

/// Reads the piece `asked` of the file at `path` as it is served; `Ok(None)` for a tile past the
/// end of the file.
pub(super) fn read(path: &Path, asked: Asked) -> Result<Option<Vec<u8>>, Unkept> {
  let (mut file, head) = open(path)?;
  let broken = |e: std::io::Error| Unkept::Broken(e.to_string());
  let kept = |index: usize| {
    let at = head.answers() + head.kept[..index].iter().sum::<usize>() as u64;
    (at, head.kept[index])
  };
  let (at, length) = match asked {
    Asked::Overview(Kind::Spectrogram) => kept(0),
    Asked::Overview(Kind::Waveform) => kept(1),
    Asked::Spectra => kept(2),
    Asked::Tile(kind, tile) => return cut_tile(&mut file, &head, kind, tile).map_err(broken),
  };
  read_at(&mut file, at, length).map(Some).map_err(broken)
}

/// Tile `tile` of `kind`, cut from the windows in its stretch of time.
fn cut_tile(
  file: &mut File,
  head: &Head,
  kind: Kind,
  tile: u32,
) -> std::io::Result<Option<Vec<u8>>> {
  let entries = &head.entries;
  let span = tiles::TILE_SECONDS as u32;
  let from = tile.saturating_mul(span);
  let first = entries.partition_point(|entry| entry.at < from);
  let last = entries.partition_point(|entry| entry.at < from.saturating_add(span));
  let (slots, slot) = match kind {
    Kind::Spectrogram => (head.columns(), COLUMN_SLOT),
    Kind::Waveform => (head.peaks(), PEAK_SLOT),
  };
  let bytes = read_at(file, slots + (first * slot) as u64, (last - first) * slot)?;
  let seconds: Vec<Second<'_>> = entries[first..last]
    .iter()
    .zip(bytes.chunks_exact(slot))
    .map(|(entry, slot)| {
      // the counts were checked against the slots when the file was opened
      let (columns, peaks) = match kind {
        Kind::Spectrogram => (&slot[..entry.columns * BINS], &[][..]),
        Kind::Waveform => (&[][..], &slot[..entry.peaks * 2]),
      };
      Second {
        at: entry.at,
        columns,
        peaks,
      }
    })
    .collect();
  let cut = tiles::tile(kind, &seconds, head.duration, tile as usize);
  Ok(cut.map(|cut| match kind {
    Kind::Spectrogram => spectrogram_answer(&cut, head.max_hz),
    Kind::Waveform => waveform_answer(&cut),
  }))
}

/// Removes what an older version kept under `dir` and a write cut short: spectrogram images, the
/// columns they were drawn from, and files never renamed into place.
pub(super) fn remove_older(dir: &Path) {
  let entries = match std::fs::read_dir(dir) {
    Ok(entries) => entries,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
    Err(e) => {
      warn!(dir = %dir.display(), error = %e, "the analysis directory not read");
      return;
    }
  };
  let mut removed = 0;
  for entry in entries.flatten() {
    let path = entry.path();
    let older = path
      .extension()
      .and_then(|e| e.to_str())
      .is_some_and(|e| matches!(e, "png" | "columns" | "part"));
    if !older {
      continue;
    }
    match std::fs::remove_file(&path) {
      Ok(()) => removed += 1,
      Err(e) => warn!(path = %path.display(), error = %e, "an older spectrogram file not removed"),
    }
  }
  if removed > 0 {
    info!(
      files = removed,
      "spectrogram files an older version kept removed"
    );
  } else {
    debug!("no spectrogram files of an older version to remove");
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::analysis::analyzer::picture::Shown;

  /// A window at `at` of `seconds`, its columns at `level` and its buckets at `peak`.
  fn window(at: u32, seconds: f64, level: u8, peak: i8) -> Window {
    let columns = (seconds * COLUMNS_PER_SECOND as f64).ceil() as usize;
    let peaks = (seconds * PEAKS_PER_SECOND as f64).ceil() as usize;
    Window {
      t: f64::from(at),
      seconds,
      rms_dbfs: -20.0,
      cutoff_hz: None,
      shelf_db: None,
      holes: None,
      inner_shelf: None,
      frame_score: None,
      energy: (0.0, 0.0),
      shown: Shown {
        spectrum: vec![level; BINS],
        columns: vec![level; columns * BINS],
        peaks: [(-peak) as u8, peak as u8].repeat(peaks),
      },
    }
  }

  /// The header and the bytes of an answer.
  fn split(answer: &[u8]) -> (serde_json::Value, &[u8]) {
    let length = u32_at(answer, 0) as usize;
    let header = serde_json::from_slice(&answer[4..4 + length]).unwrap();
    (header, body(answer).unwrap())
  }

  #[test]
  fn a_kept_file_serves_its_overviews_its_spectra_and_its_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let windows = [
      window(0, 1.0, 10, 5),
      window(1, 1.0, 20, 6),
      window(2, 0.5, 30, 7),
    ];
    let written = write(dir.path(), "a", &contents(&windows, 2.5, 22_050.0)).unwrap();
    let path = path(dir.path(), "a");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), written);
    let asked = |asked| read(&path, asked).unwrap();

    let overview = asked(Asked::Overview(Kind::Spectrogram)).unwrap();
    let (header, bytes) = split(&overview);
    // the header a viewer reads, as it is written
    assert_eq!(
      &overview[4..4 + u32_at(&overview, 0) as usize],
      br#"{"fromSeconds":0.0,"toSeconds":2.5,"columns":40,"bins":512,"maxHz":22050.0,"minDb":-160.0,"maxDb":0.0,"unread":[]}"#
    );
    assert_eq!(header["columns"], 40);
    assert_eq!(bytes.len(), 40 * BINS);
    assert_eq!((bytes[0], bytes[16 * BINS], bytes[39 * BINS]), (10, 20, 30));

    let tile = asked(Asked::Tile(Kind::Spectrogram, 0)).unwrap();
    assert_eq!(
      split(&tile).1,
      bytes,
      "a short file's one tile is its overview"
    );
    assert_eq!(asked(Asked::Tile(Kind::Spectrogram, 1)), None);

    let peaks = asked(Asked::Tile(Kind::Waveform, 0)).unwrap();
    let (header, bytes) = split(&peaks);
    assert_eq!(header["peaks"], 500);
    assert_eq!(&bytes[..2], &[(-5i8) as u8, 5]);
    assert_eq!(&bytes[998..], &[(-7i8) as u8, 7]);
    let (header, _) = split(&asked(Asked::Overview(Kind::Waveform)).unwrap());
    assert_eq!(header["peaks"], 500);

    let spectra = asked(Asked::Spectra).unwrap();
    let (header, bytes) = split(&spectra);
    assert_eq!(header["windows"], 3);
    assert_eq!(bytes.len(), 3 * BINS);
    assert_eq!(bytes[2 * BINS], 30);
  }

  #[test]
  fn a_missing_or_cut_file_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path(), "a");
    assert!(matches!(read(&path, Asked::Spectra), Err(Unkept::Missing)));
    let whole = contents(&[window(0, 1.0, 10, 5)], 1.0, 22_050.0);
    std::fs::write(&path, &whole[..whole.len() - 1]).unwrap();
    assert!(matches!(
      read(&path, Asked::Spectra),
      Err(Unkept::Broken(_))
    ));
    std::fs::write(&path, b"\x89PNG and the rest of an older image").unwrap();
    assert!(matches!(
      read(&path, Asked::Spectra),
      Err(Unkept::Broken(_))
    ));
  }

  #[test]
  fn what_an_older_version_kept_is_removed_and_the_rest_left() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
      "a.log.png",
      "a.png",
      "a.columns",
      "b.spectra.part",
      "c.spectra",
    ] {
      std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    remove_older(dir.path());
    let left: Vec<_> = std::fs::read_dir(dir.path())
      .unwrap()
      .map(|e| e.unwrap().file_name())
      .collect();
    assert_eq!(left, ["c.spectra"]);
  }
}
