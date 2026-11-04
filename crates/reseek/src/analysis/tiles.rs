//! The spectrogram and the waveform as a viewer is served them: an overview of the file's whole
//! length at a fixed number of cells, and tiles of a few seconds at full detail for a viewer
//! zoomed in. Both are cut from the windows the analysis read, which start at whole seconds; a
//! cell no window covers, between the windows of a sampled read or past a last window cut
//! short, is marked unread rather than shown as silence.

use super::analyzer::picture::{self, BINS, COLUMNS_PER_SECOND, PEAKS_PER_SECOND};

/// How many columns the spectrogram's overview has at most.
pub(super) const OVERVIEW_COLUMNS: usize = 1024;
/// How many buckets the waveform's overview has at most.
pub(super) const OVERVIEW_PEAKS: usize = 2048;
/// How many seconds a tile spans.
pub(super) const TILE_SECONDS: usize = 16;

/// One analyzed window as the tiles are cut from it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Second<'a> {
  /// the whole second the window starts at
  pub at: u32,
  /// its spectrogram columns, [`BINS`] bytes each
  pub columns: &'a [u8],
  /// its waveform buckets, the lowest and the highest sample as two signed bytes each
  pub peaks: &'a [u8],
}

/// What a tile is made of: spectrogram columns or waveform buckets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
  Spectrogram,
  Waveform,
}

impl Kind {
  /// How many cells a second holds at full detail.
  fn per_second(self) -> usize {
    match self {
      Kind::Spectrogram => COLUMNS_PER_SECOND,
      Kind::Waveform => PEAKS_PER_SECOND,
    }
  }

  /// How many bytes a cell takes.
  fn width(self) -> usize {
    match self {
      Kind::Spectrogram => BINS,
      Kind::Waveform => 2,
    }
  }

  /// How many cells an overview has at most.
  fn overview(self) -> usize {
    match self {
      Kind::Spectrogram => OVERVIEW_COLUMNS,
      Kind::Waveform => OVERVIEW_PEAKS,
    }
  }

  /// A second's cells of this kind.
  fn cells<'a>(self, second: &Second<'a>) -> &'a [u8] {
    match self {
      Kind::Spectrogram => second.columns,
      Kind::Waveform => second.peaks,
    }
  }
}

/// Cells cut for a viewer: the stretch of time they span, and the cells no window covered.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cut {
  /// when the first cell starts, in seconds
  pub from: f64,
  /// when the last cell ends, in seconds
  pub to: f64,
  /// how many cells there are
  pub count: usize,
  /// the cells one after another
  pub bytes: Vec<u8>,
  /// runs of cells no window covered, first and one past the last
  pub unread: Vec<[u32; 2]>,
}

/// How many cells of `kind` the file's length takes at full detail.
fn total(kind: Kind, duration: f64) -> usize {
  (duration * kind.per_second() as f64 - 1e-9).ceil().max(0.0) as usize
}

/// How many tiles the file's length takes.
pub(super) fn tile_count(duration: f64) -> usize {
  total(Kind::Spectrogram, duration).div_ceil(TILE_SECONDS * COLUMNS_PER_SECOND)
}

/// Cell `index` at full detail, from the start of the file, when a window covers it.
fn cell<'a>(kind: Kind, seconds: &[Second<'a>], index: usize) -> Option<&'a [u8]> {
  let per = kind.per_second();
  let at = u32::try_from(index / per).ok()?;
  let found = seconds.binary_search_by_key(&at, |s| s.at).ok()?;
  let width = kind.width();
  let start = index % per * width;
  kind.cells(&seconds[found]).get(start..start + width)
}

/// The runs of `false` in `read`, as first and one past the last.
fn runs(read: &[bool]) -> Vec<[u32; 2]> {
  let mut runs: Vec<[u32; 2]> = Vec::new();
  for (index, _) in read.iter().enumerate().filter(|(_, read)| !**read) {
    let index = index as u32;
    match runs.last_mut() {
      Some(run) if run[1] == index => run[1] += 1,
      _ => runs.push([index, index + 1]),
    }
  }
  runs
}

/// Tile `tile` of `kind` at full detail: [`TILE_SECONDS`] from `tile` times that, the last cut
/// at the end of the file; `None` past the end.
pub(super) fn tile(kind: Kind, seconds: &[Second<'_>], duration: f64, tile: usize) -> Option<Cut> {
  let per_tile = TILE_SECONDS * kind.per_second();
  let first = tile.checked_mul(per_tile)?;
  let end = (first + per_tile).min(total(kind, duration));
  if first >= end {
    return None;
  }
  let width = kind.width();
  let mut bytes = vec![0; (end - first) * width];
  let mut read = vec![false; end - first];
  for (index, (out, read)) in bytes.chunks_exact_mut(width).zip(&mut read).enumerate() {
    if let Some(cell) = cell(kind, seconds, first + index) {
      out.copy_from_slice(cell);
      *read = true;
    }
  }
  let per = kind.per_second() as f64;
  Some(Cut {
    from: first as f64 / per,
    to: end as f64 / per,
    count: end - first,
    bytes,
    unread: runs(&read),
  })
}

/// The cells at full detail from `first` up to `end` that a window covers, with the indices
/// they are at. Only the windows' own cells are visited, so the work is what was read, however
/// long a stretch the range spans.
fn covered<'a>(
  kind: Kind,
  seconds: &[Second<'a>],
  first: usize,
  end: usize,
) -> impl Iterator<Item = (usize, &'a [u8])> {
  let per = kind.per_second();
  let width = kind.width();
  // the windows are in time order; skip those that end before the range starts
  let start = seconds.partition_point(|s| (s.at as usize + 1) * per <= first);
  seconds[start..]
    .iter()
    .take_while(move |s| (s.at as usize) * per < end)
    .flat_map(move |s| {
      let base = s.at as usize * per;
      let cells = kind.cells(s);
      (base.max(first)..(base + (cells.len() / width).min(per)).min(end))
        .map(move |at| (at, &cells[(at - base) * width..(at - base + 1) * width]))
    })
}

/// The whole file's length in at most [`Kind::overview`] cells of `kind`, each made of the cells
/// at full detail it overlaps, so a file short enough has its overview at full detail:
/// spectrogram columns by their power mean, weighted by how much of each falls in it, and
/// waveform buckets by their lowest and highest sample.
pub(super) fn overview(kind: Kind, seconds: &[Second<'_>], duration: f64) -> Cut {
  let detail = total(kind, duration);
  let count = detail.min(kind.overview());
  let width = kind.width();
  // the overview spans the cells at full detail, the last of which may run a little past the end
  let span = detail as f64 / count.max(1) as f64;
  let power: Vec<f64> = (0..=255u8)
    .map(|byte| 10f64.powf(picture::byte_level(byte) / 10.0))
    .collect();
  let mut bytes = Vec::with_capacity(count * width);
  let mut read = Vec::with_capacity(count);
  for index in 0..count {
    let (from, to) = (index as f64 * span, (index + 1) as f64 * span);
    let overlapping = covered(
      kind,
      seconds,
      from.floor() as usize,
      (to.ceil() as usize).min(detail),
    )
    .map(|(at, cell)| (cell, (at as f64 + 1.0).min(to) - (at as f64).max(from)))
    .filter(|(_, weight)| *weight > 0.0)
    .collect::<Vec<_>>();
    read.push(!overlapping.is_empty());
    match kind {
      _ if overlapping.is_empty() => bytes.extend(std::iter::repeat_n(0, width)),
      Kind::Spectrogram => {
        let weights: f64 = overlapping.iter().map(|(_, weight)| weight).sum();
        bytes.extend((0..BINS).map(|bin| {
          let mean = overlapping
            .iter()
            .map(|(cell, weight)| power[usize::from(cell[bin])] * weight)
            .sum::<f64>()
            / weights;
          if mean > 0.0 {
            picture::level_byte(10.0 * mean.log10())
          } else {
            0
          }
        }));
      }
      Kind::Waveform => {
        let low = overlapping.iter().map(|(cell, _)| cell[0] as i8).min();
        let high = overlapping.iter().map(|(cell, _)| cell[1] as i8).max();
        bytes.extend([low.unwrap_or(0) as u8, high.unwrap_or(0) as u8]);
      }
    }
  }
  Cut {
    from: 0.0,
    to: detail as f64 / kind.per_second() as f64,
    count,
    bytes,
    unread: runs(&read),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A second of columns at one level each and buckets of one peak each.
  fn second(at: u32, columns: usize, level: u8, peak: i8) -> (u32, Vec<u8>, Vec<u8>) {
    let peaks = (0..columns * PEAKS_PER_SECOND / COLUMNS_PER_SECOND)
      .flat_map(|_| [(-peak) as u8, peak as u8])
      .collect();
    (at, vec![level; columns * BINS], peaks)
  }

  fn seconds(kept: &[(u32, Vec<u8>, Vec<u8>)]) -> Vec<Second<'_>> {
    kept
      .iter()
      .map(|(at, columns, peaks)| Second {
        at: *at,
        columns,
        peaks,
      })
      .collect()
  }

  #[test]
  fn tiles_cover_the_file_and_the_last_stops_at_its_end() {
    // a full read of 40.5 seconds: 40 whole windows and a half-second last one
    let kept: Vec<_> = (0..41)
      .map(|at| second(at, if at == 40 { 8 } else { 16 }, at as u8, 10))
      .collect();
    let read = seconds(&kept);
    assert_eq!(tile_count(40.5), 3);
    let first = tile(Kind::Spectrogram, &read, 40.5, 0).unwrap();
    assert_eq!((first.from, first.to, first.count), (0.0, 16.0, 256));
    assert_eq!(first.bytes.len(), 256 * BINS);
    assert_eq!(
      first.bytes[17 * BINS],
      1,
      "column 17 is in the second second"
    );
    assert!(first.unread.is_empty());
    let last = tile(Kind::Spectrogram, &read, 40.5, 2).unwrap();
    assert_eq!((last.from, last.to, last.count), (32.0, 40.5, 136));
    assert_eq!(last.bytes[135 * BINS], 40);
    assert!(last.unread.is_empty());
    assert_eq!(tile(Kind::Spectrogram, &read, 40.5, 3), None);
    let peaks = tile(Kind::Waveform, &read, 40.5, 2).unwrap();
    assert_eq!((peaks.from, peaks.to, peaks.count), (32.0, 40.5, 1700));
    assert_eq!(&peaks.bytes[..2], &[(-10i8) as u8, 10]);
  }

  #[test]
  fn a_last_window_cut_short_leaves_the_rest_of_the_file_unread() {
    // the file states 41 seconds but its last window ended half way
    let kept: Vec<_> = (0..41)
      .map(|at| second(at, if at == 40 { 8 } else { 16 }, 200, 10))
      .collect();
    let last = tile(Kind::Spectrogram, &seconds(&kept), 41.0, 2).unwrap();
    assert_eq!(last.count, 144);
    assert_eq!(last.unread, vec![[136, 144]]);
  }

  #[test]
  fn a_sampled_read_marks_what_lies_between_its_windows_unread() {
    // one-second windows at 0, 100 and 199 of a 200-second file
    let kept = vec![
      second(0, 16, 255, 100),
      second(100, 16, 255, 100),
      second(199, 16, 255, 100),
    ];
    let read = seconds(&kept);
    let tile6 = tile(Kind::Spectrogram, &read, 200.0, 6).unwrap();
    assert_eq!(tile6.from, 96.0);
    assert_eq!(tile6.unread, vec![[0, 64], [80, 256]]);
    let none = tile(Kind::Spectrogram, &read, 200.0, 2).unwrap();
    assert_eq!(none.unread, vec![[0, 256]]);
    assert!(none.bytes.iter().all(|b| *b == 0));

    let overview = overview(Kind::Spectrogram, &read, 200.0);
    // 3200 columns at full detail in 1024, 3.125 each
    assert_eq!(
      (overview.count, overview.from, overview.to),
      (1024, 0.0, 200.0)
    );
    assert_eq!(
      overview.unread[0],
      [6, 512],
      "second 0 is the first 5.12 columns"
    );
    assert_eq!(overview.unread[1], [518, 1018]);
    assert_eq!(overview.unread.len(), 2);
    // a column partly over read audio is the power mean of what was read in it
    assert_eq!(overview.bytes[5 * BINS], 255);
  }

  #[test]
  fn the_overview_averages_power_and_keeps_the_widest_peaks() {
    // seconds loud and 30 dB quieter by turns, two to an overview column
    let loud = picture::level_byte(-20.0);
    let quiet = picture::level_byte(-50.0);
    let mut kept: Vec<_> = (0..1024u32)
      .map(|at| {
        second(
          at,
          16,
          if at % 2 == 0 { loud } else { quiet },
          (at % 100) as i8,
        )
      })
      .collect();
    kept[3].2[0] = (-120i8) as u8;
    let read = seconds(&kept);
    let overview = overview(Kind::Spectrogram, &read, 2048.0);
    assert_eq!(overview.count, 1024);
    // the power mean of -20 and -50 dB is 3 dB under the louder
    assert_eq!(overview.bytes[0], picture::level_byte(-23.0));
    let peaks = super::overview(Kind::Waveform, &read, 2048.0);
    assert_eq!(peaks.count, 2048);
    assert_eq!(peaks.bytes[6], (-120i8) as u8, "a bucket is a second");
    assert_eq!(peaks.bytes[7], 3);
    // a short file has as many overview cells as it has at full detail
    let short = super::overview(Kind::Spectrogram, &read[..10], 9.5);
    assert_eq!(short.count, 152);
  }

  #[test]
  fn an_overview_takes_only_what_was_read_however_long_the_file_states() {
    // three seconds read of a file that states a length of tens of thousands of years
    let kept = vec![
      second(0, 16, 200, 50),
      second(1, 16, 200, 50),
      second(2, 16, 200, 50),
    ];
    let read = seconds(&kept);
    let peaks = overview(Kind::Waveform, &read, 1e12);
    assert_eq!((peaks.count, peaks.to), (OVERVIEW_PEAKS, 1e12));
    assert_eq!(peaks.unread, vec![[1, OVERVIEW_PEAKS as u32]]);
    assert_eq!(&peaks.bytes[..2], &[(-50i8) as u8, 50]);
  }
}
