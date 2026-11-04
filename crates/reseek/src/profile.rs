//! Our own profile: the text and picture a peer asking for our user info is sent, and the
//! picture as it is kept, an upload re-encoded to one small JPEG in the data directory, which
//! `soulseek.picture` points at. Every client can show a JPEG, and the older ones cannot show a
//! PNG.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits, Rgb, RgbImage};
use soulseek_client::LogLimiter;
use tracing::warn;

use crate::config::Options;

/// Profile pictures not read, which a peer's every user info request would repeat.
static PICTURE_UNREAD: LogLimiter = LogLimiter::new();

/// What our user info says that comes from this host rather than the upload queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalInfo {
  /// the profile text
  pub description: String,
  /// the profile picture's bytes, `None` without one or when it could not be read
  pub picture: Option<Vec<u8>>,
}

/// The profile text and picture of `options`; the picture is read each time.
pub async fn local_info(options: &Options) -> LocalInfo {
  let soulseek = &options.soulseek;
  let picture = match &soulseek.picture {
    Some(path) => match tokio::fs::read(path).await {
      Ok(bytes) => Some(bytes),
      Err(e) => {
        if let Some(suppressed) = PICTURE_UNREAD.allow() {
          warn!(path = %path.display(), error = %e, suppressed, "profile picture not read");
        }
        None
      }
    },
    None => None,
  };
  LocalInfo {
    description: soulseek.description.clone(),
    picture,
  }
}

/// Whether `soulseek.picture` of `options` names a file, found without reading it.
pub async fn has_picture(options: &Options) -> bool {
  let Some(path) = &options.soulseek.picture else {
    return false;
  };
  match tokio::fs::metadata(path).await {
    Ok(metadata) => metadata.is_file(),
    Err(e) => {
      if let Some(suppressed) = PICTURE_UNREAD.allow() {
        warn!(path = %path.display(), error = %e, suppressed, "profile picture not found");
      }
      false
    }
  }
}

/// The longest side of a kept picture, in pixels.
pub const LONGEST_SIDE: u32 = 512;
/// The largest upload taken, in bytes.
pub const MAX_UPLOAD: usize = 1024 * 1024;
/// The media types an upload may be sent as.
pub const UPLOAD_TYPES: &[&str] = &[
  "image/jpeg",
  "image/png",
  "image/gif",
  "image/webp",
  "image/bmp",
];

/// The JPEG quality a picture is kept at.
const QUALITY: u8 = 85;
/// The largest width or height an upload may decode to: a small file can describe a huge image,
/// which would take gigabytes to decode.
const MAX_DECODED_SIDE: u32 = 10_000;
/// The most memory decoding one upload may take.
const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;

/// Where the picture is kept, under the data directory `data`.
pub fn stored_path(data: &Path) -> PathBuf {
  data.join("profile").join("picture.jpg")
}

/// Why an upload could not be made into a picture.
#[derive(Debug, thiserror::Error)]
pub enum PictureError {
  /// the bytes are no JPEG, PNG, GIF, WebP or BMP image
  #[error("not a JPEG, PNG, GIF, WebP or BMP image")]
  Format,
  /// the image does not decode within the limits, or does not encode
  #[error("{0}")]
  Image(#[from] image::ImageError),
}

/// `bytes`, a JPEG, PNG, GIF, WebP or BMP image, as a JPEG at most [`LONGEST_SIDE`] pixels on its
/// longest side. Transparent parts are made white, since a JPEG has no transparency; a GIF
/// gives its first frame. Decoding and encoding take a while, so this is run off the runtime.
pub fn reencode(bytes: &[u8]) -> Result<Vec<u8>, PictureError> {
  let mut reader = ImageReader::new(Cursor::new(bytes))
    .with_guessed_format()
    .map_err(|e| PictureError::Image(e.into()))?;
  let accepted = [
    ImageFormat::Jpeg,
    ImageFormat::Png,
    ImageFormat::Gif,
    ImageFormat::WebP,
    ImageFormat::Bmp,
  ];
  if !reader.format().is_some_and(|f| accepted.contains(&f)) {
    return Err(PictureError::Format);
  }
  let mut limits = Limits::default();
  limits.max_image_width = Some(MAX_DECODED_SIDE);
  limits.max_image_height = Some(MAX_DECODED_SIDE);
  limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
  reader.limits(limits);
  let image = reader.decode()?;
  let image = if image.width().max(image.height()) > LONGEST_SIDE {
    image.resize(LONGEST_SIDE, LONGEST_SIDE, FilterType::Lanczos3)
  } else {
    image
  };
  let mut jpeg = Vec::new();
  JpegEncoder::new_with_quality(&mut jpeg, QUALITY).encode_image(&on_white(&image))?;
  Ok(jpeg)
}

/// `image` without transparency, its transparent parts blended onto white.
fn on_white(image: &DynamicImage) -> RgbImage {
  let rgba = image.to_rgba8();
  RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
    let [r, g, b, a] = rgba.get_pixel(x, y).0;
    let blend = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
    Rgb([blend(r), blend(g), blend(b)])
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use image::Rgba;

  use crate::testing::png;

  fn decoded(jpeg: &[u8]) -> DynamicImage {
    assert_eq!(image::guess_format(jpeg).unwrap(), ImageFormat::Jpeg);
    image::load_from_memory(jpeg).unwrap()
  }

  #[test]
  fn a_large_png_becomes_a_jpeg_within_the_longest_side() {
    let picture = decoded(&reencode(&png(1600, 640, Rgba([200, 30, 30, 255]))).unwrap());
    assert_eq!((picture.width(), picture.height()), (512, 205));
    let tall = decoded(&reencode(&png(300, 900, Rgba([0, 0, 0, 255]))).unwrap());
    assert_eq!((tall.width(), tall.height()), (171, 512));
  }

  #[test]
  fn a_small_picture_keeps_its_size_and_transparency_turns_white() {
    let picture = decoded(&reencode(&png(40, 20, Rgba([0, 0, 0, 0]))).unwrap());
    assert_eq!((picture.width(), picture.height()), (40, 20));
    let [r, g, b] = picture.to_rgb8().get_pixel(10, 10).0;
    assert!(r > 245 && g > 245 && b > 245, "{r} {g} {b}");
  }

  #[test]
  fn a_bmp_becomes_a_jpeg() {
    let source = image::load_from_memory(&png(16, 8, Rgba([200, 30, 30, 255]))).unwrap();
    let mut bmp = Cursor::new(Vec::new());
    source.write_to(&mut bmp, ImageFormat::Bmp).unwrap();
    let picture = decoded(&reencode(bmp.get_ref()).unwrap());
    assert_eq!((picture.width(), picture.height()), (16, 8));
  }

  #[test]
  fn what_is_no_accepted_image_is_refused() {
    assert!(matches!(
      reencode(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
      Err(PictureError::Format)
    ));
    let mut cut = png(64, 64, Rgba([1, 2, 3, 255]));
    cut.truncate(cut.len() / 2);
    assert!(matches!(reencode(&cut), Err(PictureError::Image(_))));
  }

  #[test]
  fn an_image_too_large_to_decode_is_refused_before_it_is_decoded() {
    let huge = png(MAX_DECODED_SIDE + 1, 1, Rgba([0, 0, 0, 255]));
    assert!(matches!(reencode(&huge), Err(PictureError::Image(_))));
  }
}
