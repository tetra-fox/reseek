//! Drawing on the spectrogram image: the colour of a level, lines, text in eight-pixel
//! glyphs, and the image written as PNG.

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

/// The range of loudness the colours cover, below the loudest cell, in dB.
pub(super) const RANGE_DB: f64 = 120.0;
/// Glyphs are eight pixels square.
pub(super) const GLYPH: u32 = 8;

/// The colours from quiet to loud, evenly spaced.
const COLOURS: [[u8; 3]; 9] = [
  [0, 0, 4],
  [27, 12, 65],
  [74, 12, 107],
  [120, 28, 109],
  [165, 44, 96],
  [207, 68, 70],
  [237, 105, 37],
  [251, 155, 6],
  [252, 255, 164],
];

/// The colour of a cell `below_db` under the loudest.
pub(super) fn colour(below_db: f64) -> Rgb<u8> {
  let t = (1.0 - below_db / RANGE_DB).clamp(0.0, 1.0) * (COLOURS.len() - 1) as f64;
  let index = (t.floor() as usize).min(COLOURS.len() - 2);
  let fraction = t - index as f64;
  let (a, b) = (COLOURS[index], COLOURS[index + 1]);
  let mix =
    |i: usize| (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * fraction).round() as u8;
  Rgb([mix(0), mix(1), mix(2)])
}

/// A line along row `y` from `from` to before `to`; what falls outside the image is left out.
pub(super) fn horizontal(image: &mut RgbImage, from: u32, to: u32, y: u32, colour: Rgb<u8>) {
  for x in from..to.min(image.width()) {
    if y < image.height() {
      image.put_pixel(x, y, colour);
    }
  }
}

/// A line down column `x` from `from` to before `to`; what falls outside the image is left out.
pub(super) fn vertical(image: &mut RgbImage, x: u32, from: u32, to: u32, colour: Rgb<u8>) {
  for y in from..to.min(image.height()) {
    if x < image.width() {
      image.put_pixel(x, y, colour);
    }
  }
}

/// Writes `line` in eight-pixel glyphs with its top left at `x`, `y`; what falls outside the
/// image is left out.
pub(super) fn text(image: &mut RgbImage, x: u32, y: u32, line: &str, colour: Rgb<u8>) {
  for (i, character) in line.chars().enumerate() {
    let glyph = font8x8::legacy::BASIC_LEGACY
      .get(character as usize)
      .copied()
      .unwrap_or([0; 8]);
    for (row, bits) in glyph.iter().enumerate() {
      for column in 0..GLYPH {
        if bits & (1 << column) != 0 {
          let (px, py) = (x + i as u32 * GLYPH + column, y + row as u32);
          if px < image.width() && py < image.height() {
            image.put_pixel(px, py, colour);
          }
        }
      }
    }
  }
}

/// The image as PNG.
pub(super) fn png(image: &RgbImage) -> Result<Vec<u8>, image::ImageError> {
  let mut bytes = Vec::new();
  PngEncoder::new(&mut bytes).write_image(
    image.as_raw(),
    image.width(),
    image.height(),
    ExtendedColorType::Rgb8,
  )?;
  Ok(bytes)
}
