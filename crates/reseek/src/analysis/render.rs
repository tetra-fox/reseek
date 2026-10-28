//! The spectrogram image: frequency upwards on a linear, log or mel axis in kHz, the file's whole
//! length across in time, loudness as colour, with a time axis and a colour scale. A file read
//! in samples shows each sample where it falls in time and the stretches between as unread.
//! The image with guide lines adds a labelled line where each common encoder setting cuts off
//! and one at the top of the file's band, and a dashed line at the analyser's cutoff when it
//! found one; the image without them has the same layout, so a client can draw its own lines
//! over it.

use api_types::analysis::{GuideKind, GuideLine, SpectrogramLayout};
use image::{Rgb, RgbImage};

use super::canvas::{GLYPH, RANGE_DB, colour, horizontal, png, text, vertical};
use super::measure::{ROWS, Scale, column_byte, column_db};
use super::thresholds::GUIDES;

/// How many columns the plot has across the file's length: twice the width a drawer shows it
/// at, so it stays sharp on a dense screen.
pub(super) const PLOT_WIDTH: u32 = 1024;

/// The plot's left edge: room for the frequency labels.
const LEFT: u32 = 48;
/// The plot's top edge.
const TOP: u32 = 14;
/// Below the plot: the time axis and its labels.
const BOTTOM: u32 = 30;
/// Right of the plot: the guide labels and the colour scale.
const RIGHT: u32 = 222;
/// Where the guide labels start, right of the plot.
const LABELS_FROM: u32 = 12;
/// How wide the colour scale bar is.
const BAR: u32 = 10;

const BACKGROUND: Rgb<u8> = Rgb([16, 16, 20]);
const TEXT: Rgb<u8> = Rgb([200, 200, 208]);
const TICK: Rgb<u8> = Rgb([96, 96, 112]);
const GUIDE: Rgb<u8> = Rgb([90, 210, 255]);
const LIMIT: Rgb<u8> = Rgb([255, 120, 120]);
const CUTOFF: Rgb<u8> = Rgb([255, 216, 80]);
const UNREAD: Rgb<u8> = Rgb([48, 48, 58]);

/// A spectrogram drawn twice, with and without its guide lines.
pub(super) struct Rendered {
  /// the image without guide lines, as PNG
  pub plain: Vec<u8>,
  /// the image with guide lines, as PNG
  pub guided: Vec<u8>,
  /// where the plot is and how its axes run
  pub layout: SpectrogramLayout,
}

/// One analysed window as a spectrogram shows it.
pub(super) struct Shown<'a> {
  /// where it starts, in seconds from the start of the file
  pub t: f64,
  /// how long it is, in seconds
  pub seconds: f64,
  /// its column on the picture's scale, top row first
  pub cells: &'a [u8],
}

impl Shown<'_> {
  fn end(&self) -> f64 {
    self.t + self.seconds
  }

  fn middle(&self) -> f64 {
    self.t + self.seconds / 2.0
  }
}

/// What a spectrogram shows.
pub(super) struct Picture<'a> {
  /// the windows, in time order
  pub windows: Vec<Shown<'a>>,
  /// how long the file is, in seconds, which the time axis spans
  pub duration: f64,
  /// the file's sample rate
  pub rate: u32,
  /// how the frequency axis runs
  pub scale: Scale,
  /// whether the windows are samples spread over a longer file
  pub sampled: bool,
  /// the analyser's cutoff, when it found one, in Hz
  pub cutoff_hz: Option<f64>,
}

/// Draws the spectrogram of `picture`.
pub(super) fn render(picture: &Picture<'_>) -> Result<Rendered, image::ImageError> {
  let columns = columns(picture);
  let layout = layout(picture);
  let mut image = RgbImage::from_pixel(layout.width, layout.height, BACKGROUND);
  draw_plot(&mut image, &columns, &layout);
  draw_frequency_axis(&mut image, &layout, picture.scale);
  draw_time_axis(&mut image, &layout, picture);
  draw_colour_scale(&mut image, &layout, &columns);
  let plain = png(&image)?;
  draw_guides(&mut image, &layout);
  let guided = png(&image)?;
  Ok(Rendered {
    plain,
    guided,
    layout,
  })
}

/// Where everything goes, with the guide lines for this file.
fn layout(picture: &Picture<'_>) -> SpectrogramLayout {
  let max_hz = f64::from(picture.rate) / 2.0;
  let mut guides: Vec<GuideLine> = GUIDES
    .iter()
    .filter(|(hz, _)| *hz < max_hz)
    .map(|(hz, label)| GuideLine {
      hz: *hz,
      label: (*label).to_string(),
      kind: GuideKind::Encoder,
    })
    .collect();
  guides.push(GuideLine {
    hz: max_hz,
    label: "file limit".to_string(),
    kind: GuideKind::FileLimit,
  });
  if let Some(cutoff) = picture.cutoff_hz {
    guides.push(GuideLine {
      hz: cutoff,
      label: "analyser's cutoff".to_string(),
      kind: GuideKind::Cutoff,
    });
  }
  guides.sort_by(|a, b| a.hz.total_cmp(&b.hz));
  SpectrogramLayout {
    width: LEFT + PLOT_WIDTH + RIGHT,
    height: TOP + ROWS as u32 + BOTTOM,
    plot_x: LEFT,
    plot_y: TOP,
    plot_width: PLOT_WIDTH,
    plot_height: ROWS as u32,
    scale: picture.scale.into(),
    min_hz: picture.scale.min_hz(),
    max_hz,
    duration_seconds: picture.duration,
    sampled: picture.sampled,
    guides,
  }
}

impl From<Scale> for api_types::analysis::SpectrogramScale {
  fn from(scale: Scale) -> Self {
    match scale {
      Scale::Linear => Self::Linear,
      Scale::Log => Self::Log,
      Scale::Mel => Self::Mel,
    }
  }
}

impl From<api_types::analysis::SpectrogramScale> for Scale {
  fn from(scale: api_types::analysis::SpectrogramScale) -> Self {
    match scale {
      api_types::analysis::SpectrogramScale::Linear => Self::Linear,
      api_types::analysis::SpectrogramScale::Log => Self::Log,
      api_types::analysis::SpectrogramScale::Mel => Self::Mel,
    }
  }
}

impl From<crate::config::enums::SpectrogramScale> for Scale {
  fn from(scale: crate::config::enums::SpectrogramScale) -> Self {
    match scale {
      crate::config::enums::SpectrogramScale::Linear => Self::Linear,
      crate::config::enums::SpectrogramScale::Log => Self::Log,
      crate::config::enums::SpectrogramScale::Mel => Self::Mel,
    }
  }
}

/// `layout` with its frequency axis on `scale`; the plot, its size and the guides are the same
/// on every scale.
pub(super) fn on_scale(layout: &SpectrogramLayout, scale: Scale) -> SpectrogramLayout {
  SpectrogramLayout {
    scale: scale.into(),
    min_hz: scale.min_hz(),
    ..layout.clone()
  }
}

/// The plot row, from the top, that `hz` falls in.
pub(super) fn row_of(layout: &SpectrogramLayout, scale: Scale, hz: f64) -> u32 {
  let position = scale
    .position(hz, layout.min_hz, layout.max_hz)
    .clamp(0.0, 1.0);
  let row = ((1.0 - position) * f64::from(layout.plot_height)).floor() as u32;
  row.min(layout.plot_height - 1)
}

/// The column at each of the plot's [`PLOT_WIDTH`] steps across the file's length, `None` where
/// no window was read. A step that one window covers blends it with the window next to it in
/// time, by how far the step's middle is from each window's middle, so a full read changes
/// smoothly from second to second; a step that several windows cover takes their power mean.
fn columns(picture: &Picture<'_>) -> Vec<Option<Vec<u8>>> {
  let windows = &picture.windows;
  let step = picture.duration / f64::from(PLOT_WIDTH);
  (0..PLOT_WIDTH)
    .map(|x| {
      let (from, to) = (f64::from(x) * step, f64::from(x + 1) * step);
      let first = windows.partition_point(|w| w.end() <= from);
      let last = windows.partition_point(|w| w.t < to);
      match last.saturating_sub(first) {
        0 => None,
        1 => Some(blended(windows, first, from + step / 2.0)),
        _ => Some(power_mean(&windows[first..last])),
      }
    })
    .collect()
}

/// Window `i`'s column at time `at`, blended towards the window that runs on from it on that
/// side, if one does; across a stretch that was not read nothing is blended.
fn blended(windows: &[Shown<'_>], i: usize, at: f64) -> Vec<u8> {
  let here = &windows[i];
  let beside = if at < here.middle() {
    i.checked_sub(1).map(|j| &windows[j])
  } else {
    windows.get(i + 1)
  };
  let touching =
    |w: &&Shown<'_>| (w.end() - here.t).abs() < 1e-6 || (here.end() - w.t).abs() < 1e-6;
  let Some(beside) = beside.filter(touching) else {
    return here.cells.to_vec();
  };
  let share = ((at - here.middle()) / (beside.middle() - here.middle())).clamp(0.0, 1.0);
  // a byte is a level in dB on a straight scale, so blending bytes blends levels
  here
    .cells
    .iter()
    .zip(beside.cells)
    .map(|(a, b)| (f64::from(*a) + share * (f64::from(*b) - f64::from(*a))).round() as u8)
    .collect()
}

/// The power mean of several windows' columns, as a column.
fn power_mean(run: &[Shown<'_>]) -> Vec<u8> {
  (0..ROWS)
    .map(|row| {
      let power = run
        .iter()
        .map(|w| 10f64.powf(column_db(w.cells[row]) / 10.0))
        .sum::<f64>()
        / run.len() as f64;
      column_byte(10.0 * power.max(f64::MIN_POSITIVE).log10())
    })
    .collect()
}

/// The loudest cell of any column, as a byte.
fn loudest(columns: &[Option<Vec<u8>>]) -> u8 {
  columns
    .iter()
    .flatten()
    .flat_map(|c| c.iter().copied())
    .max()
    .unwrap_or(255)
}

fn draw_plot(image: &mut RgbImage, columns: &[Option<Vec<u8>>], layout: &SpectrogramLayout) {
  let top_db = column_db(loudest(columns));
  for (x, column) in (0..).zip(columns) {
    for y in 0..layout.plot_height {
      let pixel = match column {
        Some(cells) => colour(top_db - column_db(cells[y as usize])),
        // a stretch that was not read is hatched, so it cannot pass for silence
        None if (x + y) / 4 % 2 == 0 => UNREAD,
        None => BACKGROUND,
      };
      image.put_pixel(layout.plot_x + x, layout.plot_y + y, pixel);
    }
  }
}

/// The frequencies the axis is labelled at, in Hz.
fn ticks(scale: Scale, max_hz: f64) -> Vec<f64> {
  match scale {
    Scale::Log => [
      50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 10e3, 20e3, 50e3, 100e3,
    ]
    .into_iter()
    .filter(|hz| *hz <= max_hz)
    .collect(),
    Scale::Linear => {
      let step = [1e3, 2e3, 4e3, 5e3, 10e3, 20e3, 50e3]
        .into_iter()
        .find(|step| max_hz / step <= 12.0)
        .unwrap_or(100e3);
      (0..)
        .map(|i| f64::from(i) * step)
        .take_while(|hz| *hz <= max_hz)
        .collect()
    }
    Scale::Mel => [
      0.0, 250.0, 500.0, 1e3, 2e3, 4e3, 8e3, 12e3, 16e3, 20e3, 32e3, 64e3,
    ]
    .into_iter()
    .filter(|hz| *hz <= max_hz)
    .collect(),
  }
}

fn draw_frequency_axis(image: &mut RgbImage, layout: &SpectrogramLayout, scale: Scale) {
  let axis = layout.plot_x - 1;
  vertical(
    image,
    axis,
    layout.plot_y,
    layout.plot_y + layout.plot_height,
    TICK,
  );
  text(image, 4, 2, "kHz", TEXT);
  for hz in ticks(scale, layout.max_hz) {
    let y = layout.plot_y + row_of(layout, scale, hz);
    horizontal(image, axis - 4, axis, y, TICK);
    let label = kilohertz(hz);
    let x = axis.saturating_sub(6 + GLYPH * label.len() as u32);
    let y = y.saturating_sub(GLYPH / 2).max(layout.plot_y);
    text(image, x, y, &label, TEXT);
  }
}

/// A frequency in kHz as briefly as it reads: `0.05`, `1`, `22.05`.
fn kilohertz(hz: f64) -> String {
  let text = format!("{:.2}", hz / 1000.0);
  text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The times the time axis is labelled at, in seconds, and the plot column each falls in: every
/// whole step from 0, the step the shortest that leaves room for each label, then the file's
/// end, in place of a step less than half a step before it.
pub(super) fn time_ticks(duration: f64, plot_width: u32) -> Vec<(u32, f64)> {
  let room = f64::from((plot_width / 64).max(1));
  let step = [
    1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0,
  ]
  .into_iter()
  .find(|step| duration / step <= room)
  .unwrap_or(3600.0);
  let mut times: Vec<f64> = (0..)
    .map(|i| f64::from(i) * step)
    .take_while(|t| *t < duration)
    .collect();
  if times.len() > 1 && times.last().is_some_and(|t| duration - t < step / 2.0) {
    times.pop();
  }
  times.push(duration);
  times
    .into_iter()
    .map(|t| {
      let x = (t / duration * f64::from(plot_width)).floor() as u32;
      (x.min(plot_width - 1), t)
    })
    .collect()
}

fn draw_time_axis(image: &mut RgbImage, layout: &SpectrogramLayout, picture: &Picture<'_>) {
  let y = layout.plot_y + layout.plot_height;
  horizontal(
    image,
    layout.plot_x,
    layout.plot_x + layout.plot_width,
    y,
    TICK,
  );
  let mut free_from = 0;
  for (x, seconds) in time_ticks(picture.duration, layout.plot_width) {
    let px = layout.plot_x + x;
    vertical(image, px, y, y + 4, TICK);
    let label = format!("{}:{:02}", (seconds / 60.0) as u64, (seconds % 60.0) as u64);
    let wide = GLYPH * label.len() as u32;
    // the end's label ends at the plot's edge, clear of the note beside it
    let left = px
      .saturating_sub(wide / 2)
      .min(layout.plot_x + layout.plot_width - wide);
    // a label that would run into the one before it is left out
    if left < free_from {
      continue;
    }
    text(image, left, y + 8, &label, TEXT);
    free_from = left + wide + GLYPH;
  }
  if picture.sampled {
    let note = format!("{} sampled seconds", picture.windows.len());
    let x = layout.plot_x + layout.plot_width + LABELS_FROM;
    text(image, x, y + 8, &note, TICK);
  }
}

fn draw_colour_scale(
  image: &mut RgbImage,
  layout: &SpectrogramLayout,
  columns: &[Option<Vec<u8>>],
) {
  let x = layout.width - 8 - 5 * GLYPH - BAR;
  for y in 0..layout.plot_height {
    let below = RANGE_DB * f64::from(y) / f64::from(layout.plot_height - 1);
    horizontal(image, x, x + BAR, layout.plot_y + y, colour(below));
  }
  let top = column_db(loudest(columns)).round();
  text(
    image,
    x + BAR + 4,
    layout.plot_y,
    &format!("{top:.0}"),
    TEXT,
  );
  let bottom = layout.plot_y + layout.plot_height - GLYPH;
  text(
    image,
    x + BAR + 4,
    bottom,
    &format!("{:.0}", top - RANGE_DB),
    TEXT,
  );
  text(
    image,
    x + BAR + 4,
    layout.plot_y + layout.plot_height / 2,
    "dB",
    TEXT,
  );
}

fn draw_guides(image: &mut RgbImage, layout: &SpectrogramLayout) {
  let scale = Scale::from(layout.scale);
  let right = layout.plot_x + layout.plot_width;
  let label_x = right + LABELS_FROM;
  // labels go top first, each at its line or pushed below the one above it
  let mut placed: Vec<(u32, u32, &GuideLine)> = layout
    .guides
    .iter()
    .rev()
    .map(|guide| (layout.plot_y + row_of(layout, scale, guide.hz), 0, guide))
    .collect();
  let mut next_free = 0;
  for (line, label, _) in &mut placed {
    *label = line.saturating_sub(GLYPH / 2).max(next_free);
    next_free = *label + GLYPH + 3;
  }
  for (line, label, guide) in &placed {
    let colour = match guide.kind {
      GuideKind::Encoder => GUIDE,
      GuideKind::FileLimit => LIMIT,
      GuideKind::Cutoff => CUTOFF,
    };
    for x in layout.plot_x..right + 4 {
      let dashed = guide.kind == GuideKind::Cutoff && (x - layout.plot_x) % 7 >= 4;
      if !dashed {
        image.put_pixel(x, *line, colour);
      }
    }
    let middle = label + GLYPH / 2;
    vertical(
      image,
      right + 4,
      (*line).min(middle),
      (*line).max(middle) + 1,
      colour,
    );
    horizontal(image, right + 4, label_x - 2, middle, colour);
    text(image, label_x, *label, &guide.label, colour);
  }
  let note = ["common encoder", "defaults; others", "cut elsewhere"];
  let mut y = layout.plot_y + layout.plot_height - GLYPH * 3 - 6;
  for line in note {
    text(image, label_x, y, line, TICK);
    y += GLYPH + 2;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// `count` one-second windows from `t = 0`, `apart` seconds apart, at the same level in every
  /// row.
  fn flat(count: usize, apart: f64) -> Vec<Shown<'static>> {
    static CELLS: [u8; ROWS] = [128; ROWS];
    (0..count)
      .map(|i| Shown {
        t: i as f64 * apart,
        seconds: 1.0,
        cells: &CELLS,
      })
      .collect()
  }

  fn picture(windows: Vec<Shown<'static>>, duration: f64, scale: Scale) -> Picture<'static> {
    Picture {
      windows,
      duration,
      rate: 44_100,
      scale,
      sampled: false,
      cutoff_hz: Some(10_000.0),
    }
  }

  fn decoded(png: &[u8]) -> RgbImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8()
  }

  #[test]
  fn guide_lines_fall_on_the_rows_of_their_frequencies() {
    // rows from the top of a 512-row plot up to 22.05 kHz, worked out by hand
    let cases = [
      (
        Scale::Linear,
        [
          (16_000.0, 140),
          (19_000.0, 70),
          (19_500.0, 59),
          (20_000.0, 47),
        ],
      ),
      (
        Scale::Log,
        [(16_000.0, 23), (19_000.0, 10), (19_500.0, 8), (20_000.0, 7)],
      ),
      (
        Scale::Mel,
        [
          (16_000.0, 45),
          (19_000.0, 21),
          (19_500.0, 17),
          (20_000.0, 13),
        ],
      ),
    ];
    for (scale, rows) in cases {
      let rendered = render(&picture(flat(40, 1.0), 40.0, scale)).unwrap();
      let layout = &rendered.layout;
      assert_eq!((layout.plot_width, layout.plot_height), (PLOT_WIDTH, 512));
      let (guided, plain) = (decoded(&rendered.guided), decoded(&rendered.plain));
      assert_eq!(guided.dimensions(), (layout.width, layout.height));
      let at = |image: &RgbImage, x: u32, row: u32| {
        *image.get_pixel(layout.plot_x + x, layout.plot_y + row)
      };
      for (hz, row) in rows {
        assert_eq!(row_of(layout, scale, hz), row, "{scale:?} {hz}");
        for x in [0, PLOT_WIDTH / 2, PLOT_WIDTH - 1] {
          assert_eq!(at(&guided, x, row), GUIDE, "{scale:?} {hz} at {x}");
          assert_ne!(
            at(&plain, x, row),
            GUIDE,
            "{scale:?} {hz} drawn without guides"
          );
        }
        if !rows.iter().any(|(_, other)| *other == row + 1) {
          assert_ne!(
            at(&guided, 20, row + 1),
            GUIDE,
            "{scale:?} {hz}: one row only"
          );
        }
      }
      assert_eq!(at(&guided, 10, 0), LIMIT, "the file limit along the top");
      let cutoff = row_of(layout, scale, 10_000.0);
      let dashes: Vec<bool> = (0..14).map(|x| at(&guided, x, cutoff) == CUTOFF).collect();
      assert!(
        dashes.contains(&true) && dashes.contains(&false),
        "{dashes:?}"
      );
      let kinds: Vec<GuideKind> = layout.guides.iter().map(|g| g.kind).collect();
      assert_eq!(
        kinds.iter().filter(|k| **k == GuideKind::Encoder).count(),
        4
      );
      assert!(
        layout
          .guides
          .windows(2)
          .all(|pair| pair[0].hz <= pair[1].hz)
      );
    }
  }

  #[test]
  fn only_lines_below_the_file_limit_are_drawn() {
    let mut picture = picture(flat(30, 1.0), 30.0, Scale::Linear);
    picture.rate = 32_000;
    picture.cutoff_hz = None;
    picture.sampled = true;
    let layout = render(&picture).unwrap().layout;
    let guides: Vec<(f64, GuideKind)> = layout.guides.iter().map(|g| (g.hz, g.kind)).collect();
    assert_eq!(guides, [(16_000.0, GuideKind::FileLimit)]);
    assert!(layout.sampled);
  }

  #[test]
  fn a_full_read_fills_the_time_axis_from_start_to_end() {
    let picture = picture(flat(40, 1.0), 40.0, Scale::Log);
    assert!(columns(&picture).iter().all(Option::is_some));
    let layout = render(&picture).unwrap().layout;
    assert_eq!(layout.duration_seconds, 40.0);
    let ticks = time_ticks(40.0, PLOT_WIDTH);
    assert_eq!(ticks.first(), Some(&(0, 0.0)));
    // the last label is the end, at the plot's right edge
    assert_eq!(ticks.last(), Some(&(PLOT_WIDTH - 1, 40.0)));
    let (x, t) = ticks[ticks.len() / 2];
    assert_eq!(x, (t / 40.0 * f64::from(PLOT_WIDTH)) as u32);
  }

  #[test]
  fn a_sampled_read_puts_each_window_at_its_time_and_hatches_the_rest() {
    // four one-second samples of a 400-second file, at 0, 100, 200 and 300 s
    let mut picture = picture(flat(4, 100.0), 400.0, Scale::Linear);
    picture.sampled = true;
    let columns = columns(&picture);
    let read: Vec<usize> = (0..)
      .zip(&columns)
      .filter_map(|(x, c)| c.as_ref().map(|_| x))
      .collect();
    // a column of 1024 across 400 s is 0.39 s, so each one-second sample covers the three
    // columns it overlaps, at its own time, and nothing else is read
    let step = 400.0 / f64::from(PLOT_WIDTH);
    let expected: Vec<usize> = [0.0, 100.0, 200.0, 300.0]
      .into_iter()
      .flat_map(|t: f64| (t / step).floor() as usize..((t + 1.0) / step).ceil() as usize)
      .collect();
    assert_eq!(read, expected);
    let ticks = time_ticks(400.0, PLOT_WIDTH);
    assert_eq!(ticks.last().map(|(_, t)| *t), Some(400.0));
    let rendered = render(&picture).unwrap();
    let image = decoded(&rendered.plain);
    let layout = &rendered.layout;
    let unread: Vec<Rgb<u8>> = (0..8)
      .map(|y| *image.get_pixel(layout.plot_x + 600, layout.plot_y + y))
      .collect();
    assert!(
      unread.contains(&UNREAD) && unread.contains(&BACKGROUND),
      "{unread:?}"
    );
  }
}
