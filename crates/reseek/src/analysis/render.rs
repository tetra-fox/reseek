//! The spectrogram image: one column per analysed window, frequency upwards on a log or linear
//! axis in kHz, loudness as colour, with a time axis and a colour scale. The image with guide
//! lines adds a labelled line where each common encoder setting cuts off and one at the top of
//! the file's band, and a dashed line at the analyser's cutoff when it found one; the image
//! without them has the same layout, so a client can draw its own lines over it.

use api_types::analysis::{GuideKind, GuideLine, SpectrogramLayout};
use image::{Rgb, RgbImage};

use super::canvas::{GLYPH, RANGE_DB, colour, horizontal, png, text, vertical};
use super::measure::{ROWS, Scale, Window, column_byte, column_db};
use super::thresholds::GUIDES;

/// The most columns a spectrogram has; longer files share columns between windows.
pub(super) const MAX_COLUMNS: usize = 2000;

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

/// A spectrogram drawn twice, with and without its guide lines.
pub(super) struct Rendered {
  /// the image without guide lines, as PNG
  pub plain: Vec<u8>,
  /// the image with guide lines, as PNG
  pub guided: Vec<u8>,
  /// where the plot is and how its axes run
  pub layout: SpectrogramLayout,
}

/// What a spectrogram shows.
pub(super) struct Picture<'a> {
  /// the windows, in time order
  pub windows: &'a [Window],
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
  let columns = columns(picture.windows);
  let width = columns.len().max(1) as u32;
  let layout = layout(picture, width);
  let mut image = RgbImage::from_pixel(layout.width, layout.height, BACKGROUND);
  draw_plot(&mut image, &columns, &layout);
  draw_frequency_axis(&mut image, &layout, picture.scale);
  draw_time_axis(
    &mut image,
    picture.windows,
    &columns,
    &layout,
    picture.sampled,
  );
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
fn layout(picture: &Picture<'_>, plot_width: u32) -> SpectrogramLayout {
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
    width: LEFT + plot_width + RIGHT,
    height: TOP + ROWS as u32 + BOTTOM,
    plot_x: LEFT,
    plot_y: TOP,
    plot_width,
    plot_height: ROWS as u32,
    scale: picture.scale.into(),
    min_hz: picture.scale.min_hz(),
    max_hz,
    sampled: picture.sampled,
    guides,
  }
}

impl From<Scale> for api_types::analysis::SpectrogramScale {
  fn from(scale: Scale) -> Self {
    match scale {
      Scale::Log => Self::Log,
      Scale::Linear => Self::Linear,
    }
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

/// The columns to draw: each window's, or for more windows than [`MAX_COLUMNS`] the power
/// mean of each run of windows, with the window each column starts at.
fn columns(windows: &[Window]) -> Vec<(usize, Vec<u8>)> {
  if windows.len() <= MAX_COLUMNS {
    return windows
      .iter()
      .enumerate()
      .map(|(i, w)| (i, w.column.clone()))
      .collect();
  }
  (0..MAX_COLUMNS)
    .map(|column| {
      let first = column * windows.len() / MAX_COLUMNS;
      let last = ((column + 1) * windows.len() / MAX_COLUMNS).max(first + 1);
      let run = &windows[first..last];
      let merged = (0..ROWS)
        .map(|row| {
          let power = run
            .iter()
            .map(|w| 10f64.powf(column_db(w.column[row]) / 10.0))
            .sum::<f64>()
            / run.len() as f64;
          column_byte(10.0 * power.max(f64::MIN_POSITIVE).log10())
        })
        .collect();
      (first, merged)
    })
    .collect()
}

/// The loudest cell of any column, as a byte.
fn loudest(columns: &[(usize, Vec<u8>)]) -> u8 {
  columns
    .iter()
    .flat_map(|(_, c)| c.iter().copied())
    .max()
    .unwrap_or(255)
}

fn draw_plot(image: &mut RgbImage, columns: &[(usize, Vec<u8>)], layout: &SpectrogramLayout) {
  let top_db = column_db(loudest(columns));
  for (x, (_, column)) in columns.iter().enumerate() {
    for (y, byte) in column.iter().enumerate() {
      let below = top_db - column_db(*byte);
      image.put_pixel(
        layout.plot_x + x as u32,
        layout.plot_y + y as u32,
        colour(below),
      );
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

fn draw_time_axis(
  image: &mut RgbImage,
  windows: &[Window],
  columns: &[(usize, Vec<u8>)],
  layout: &SpectrogramLayout,
  sampled: bool,
) {
  let y = layout.plot_y + layout.plot_height;
  for x in 0..layout.plot_width {
    // a sampled file's axis is dotted: its columns are not next to each other in time
    if !sampled || x % 2 == 0 {
      image.put_pixel(layout.plot_x + x, y, TICK);
    }
  }
  if columns.is_empty() {
    return;
  }
  let labels = (layout.plot_width / 56).max(1);
  let mut free_from = 0;
  for i in 0..=labels {
    let x = (i * (layout.plot_width - 1) / labels) as usize;
    let Some((window, _)) = columns.get(x) else {
      continue;
    };
    let seconds = windows[*window].t;
    let px = layout.plot_x + x as u32;
    vertical(image, px, y, y + 4, TICK);
    let label = format!("{}:{:02}", (seconds / 60.0) as u64, (seconds % 60.0) as u64);
    let wide = GLYPH * label.len() as u32;
    let left = px.saturating_sub(wide / 2).min(layout.width - wide);
    // a label that would run into the one before it is left out
    if left < free_from {
      continue;
    }
    text(image, left, y + 8, &label, TEXT);
    free_from = left + wide + GLYPH;
  }
  if sampled {
    let note = format!("{} sampled seconds", windows.len());
    let x = layout.plot_x + layout.plot_width + LABELS_FROM;
    text(image, x, y + 8, &note, TICK);
  }
}

fn draw_colour_scale(
  image: &mut RgbImage,
  layout: &SpectrogramLayout,
  columns: &[(usize, Vec<u8>)],
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
  let scale = match layout.scale {
    api_types::analysis::SpectrogramScale::Log => Scale::Log,
    api_types::analysis::SpectrogramScale::Linear => Scale::Linear,
  };
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

  /// `count` windows with the same level in every row.
  fn flat(count: usize) -> Vec<Window> {
    (0..count)
      .map(|i| Window {
        t: i as f64,
        rms_dbfs: -20.0,
        cutoff_hz: None,
        shelf_db: None,
        holes: None,
        inner_shelf: None,
        frame_score: None,
        energy: (0.0, 0.0),
        column: vec![128; ROWS],
      })
      .collect()
  }

  fn decoded(png: &[u8]) -> RgbImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8()
  }

  #[test]
  fn guide_lines_fall_on_the_rows_of_their_frequencies() {
    let windows = flat(40);
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
    ];
    for (scale, rows) in cases {
      let picture = Picture {
        windows: &windows,
        rate: 44_100,
        scale,
        sampled: false,
        cutoff_hz: Some(10_000.0),
      };
      let rendered = render(&picture).unwrap();
      let layout = &rendered.layout;
      assert_eq!((layout.plot_width, layout.plot_height), (40, 512));
      let (guided, plain) = (decoded(&rendered.guided), decoded(&rendered.plain));
      assert_eq!(guided.dimensions(), (layout.width, layout.height));
      let at = |image: &RgbImage, x: u32, row: u32| {
        *image.get_pixel(layout.plot_x + x, layout.plot_y + row)
      };
      for (hz, row) in rows {
        assert_eq!(row_of(layout, scale, hz), row, "{scale:?} {hz}");
        for x in [0, 20, 39] {
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
    let windows = flat(30);
    let picture = Picture {
      windows: &windows,
      rate: 32_000,
      scale: Scale::Linear,
      sampled: true,
      cutoff_hz: None,
    };
    let layout = render(&picture).unwrap().layout;
    let guides: Vec<(f64, GuideKind)> = layout.guides.iter().map(|g| (g.hz, g.kind)).collect();
    assert_eq!(guides, [(16_000.0, GuideKind::FileLimit)]);
    assert!(layout.sampled);
  }
}
