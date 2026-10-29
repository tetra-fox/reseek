//! The spectrograms kept on disk. A report keeps its windows' columns on every scale in one
//! file, and the images drawn from them, with and without guide lines, for each scale that was
//! drawn: the option's when the file is analysed, another the first time a viewer asks for it.
//! Every file of a report starts with the report's image name, so they are counted and removed
//! together.

use std::path::{Path, PathBuf};

use api_types::analysis::{AudioAnalysis, SpectrogramLayout};
use tracing::{debug, warn};

use super::measure::{ROWS, Scale, Window};
use super::render::{Picture, Shown, render};
use super::{Analysis, jobs, store};
use crate::api::error::ApiError;

/// An image of a report on `scale`, with guide lines or without.
pub(super) fn image_path(dir: &Path, name: &str, scale: Scale, guides: bool) -> PathBuf {
  let scale = scale_name(scale);
  if guides {
    dir.join(format!("{name}.{scale}.png"))
  } else {
    dir.join(format!("{name}.{scale}.plain.png"))
  }
}

/// The columns of every window of a report on every scale, kept for drawing it on another scale
/// and for showing one window's spectrum.
pub(super) fn columns_path(dir: &Path, name: &str) -> PathBuf {
  dir.join(format!("{name}.columns"))
}

/// The images an older version kept a report under, one scale only: no longer served, but
/// counted with the report's files and removed with them.
pub(super) fn older_paths(dir: &Path, name: &str) -> [PathBuf; 2] {
  [
    dir.join(format!("{name}.png")),
    dir.join(format!("{name}.plain.png")),
  ]
}

/// Every file a report may have on disk.
pub(super) fn all_paths(dir: &Path, name: &str) -> Vec<PathBuf> {
  Scale::ALL
    .into_iter()
    .flat_map(|scale| [true, false].map(|guides| image_path(dir, name, scale, guides)))
    .chain([columns_path(dir, name)])
    .chain(older_paths(dir, name))
    .collect()
}

fn scale_name(scale: Scale) -> &'static str {
  match scale {
    Scale::Linear => "linear",
    Scale::Log => "log",
    Scale::Mel => "mel",
  }
}

/// The columns file's contents: every window's column on the first scale of [`Scale::ALL`],
/// then on the second, then on the third.
pub(super) fn columns_bytes(windows: &[Window]) -> Vec<u8> {
  Scale::ALL
    .into_iter()
    .flat_map(|scale| {
      windows
        .iter()
        .flat_map(move |w| w.column(scale).iter().copied())
    })
    .collect()
}

/// The columns of `windows` windows on `scale` from a columns file, `None` when the file does
/// not hold that many windows on every scale.
pub(super) fn scale_columns(bytes: &[u8], windows: usize, scale: Scale) -> Option<&[u8]> {
  let block = windows * ROWS;
  if bytes.len() != block * Scale::ALL.len() {
    return None;
  }
  let start = scale.index() * block;
  bytes.get(start..start + block)
}

/// Writes files under `dir`, creating it.
pub(super) fn write(dir: &Path, files: &[(PathBuf, &[u8])]) -> Result<(), String> {
  std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  for (path, bytes) in files {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
  }
  Ok(())
}

/// Removes the images an older version kept a report under, once the report has its own.
pub(super) fn remove_older(dir: &Path, name: &str) {
  for path in older_paths(dir, name) {
    if let Err(e) = std::fs::remove_file(&path)
      && e.kind() != std::io::ErrorKind::NotFound
    {
      warn!(path = %path.display(), error = %e, "an older spectrogram not removed");
    }
  }
}

/// How many bytes a report's files take on disk.
pub(super) fn kept_bytes(dir: &Path, name: &str) -> u64 {
  all_paths(dir, name)
    .iter()
    // a scale not drawn yet has no file
    .filter_map(|path| std::fs::metadata(path).ok())
    .map(|metadata| metadata.len())
    .sum()
}

/// What a stored report's spectrogram shows on `scale`, from that scale's columns. Windows are a
/// second long, the last cut short by the end of the file.
pub(super) fn stored_picture<'a>(
  report: &AudioAnalysis,
  layout: &SpectrogramLayout,
  columns: &'a [u8],
  scale: Scale,
) -> Picture<'a> {
  let windows = report.windows.as_deref().unwrap_or_default();
  let duration = report.duration_seconds;
  let shown = windows
    .iter()
    .zip(columns.as_chunks::<ROWS>().0)
    .enumerate()
    .map(|(i, (w, cells))| {
      let next = windows.get(i + 1).map_or(duration, |n| n.t);
      Shown {
        t: w.t,
        seconds: 1f64.min(next - w.t).min(duration - w.t),
        cells,
      }
    })
    .collect();
  Picture {
    windows: shown,
    duration,
    rate: report.claimed.sample_rate,
    scale,
    sampled: layout.sampled,
    cutoff_hz: layout
      .guides
      .iter()
      .find(|g| g.kind == api_types::analysis::GuideKind::Cutoff)
      .map(|g| g.hz),
  }
}

/// The answer when a report's spectra are no longer on disk.
fn not_kept() -> ApiError {
  ApiError::not_found("the spectrogram is no longer kept; analyse the file again to draw it")
}

impl Analysis {
  /// The columns of a stored report's windows on `scale`.
  pub(super) async fn columns(
    &self,
    stored: &store::Stored,
    scale: Scale,
  ) -> Result<Vec<u8>, ApiError> {
    let path = columns_path(&self.images, &stored.image);
    let bytes = match tokio::fs::read(&path).await {
      Ok(bytes) => bytes,
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_kept()),
      Err(e) => {
        return Err(ApiError::internal(format!(
          "the spectra '{}' could not be read: {e}",
          path.display()
        )));
      }
    };
    let windows = stored.report.windows.as_deref().unwrap_or_default().len();
    match scale_columns(&bytes, windows, scale) {
      Some(columns) => Ok(columns.to_vec()),
      None => {
        warn!(path = %path.display(), windows, "spectra do not match their report");
        Err(not_kept())
      }
    }
  }

  /// Draws a stored report's spectrogram on `scale`, keeps both images of it and gives the one
  /// with guide lines or without.
  pub(super) async fn draw(
    &self,
    stored: &store::Stored,
    scale: Scale,
    guides: bool,
  ) -> Result<Vec<u8>, ApiError> {
    let layout = stored.report.spectrogram.clone().ok_or_else(not_kept)?;
    let columns = self.columns(stored, scale).await?;
    let (report, dir, name) = (
      stored.report.clone(),
      self.images.clone(),
      stored.image.clone(),
    );
    let drawn = tokio::task::spawn_blocking(move || {
      let picture = stored_picture(&report, &layout, &columns, scale);
      let rendered = render(&picture).map_err(|e| e.to_string())?;
      write(
        &dir,
        &[
          (image_path(&dir, &name, scale, true), &rendered.guided),
          (image_path(&dir, &name, scale, false), &rendered.plain),
        ],
      )?;
      let bytes = kept_bytes(&dir, &name);
      Ok::<_, String>((rendered, bytes))
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(|e| ApiError::internal(format!("the spectrogram could not be drawn: {e}")))?;
    let (rendered, bytes) = drawn;
    debug!(
      image = stored.image,
      ?scale,
      "spectrogram drawn on another scale"
    );
    jobs::keep_images(self, &stored.image, bytes).await;
    Ok(if guides {
      rendered.guided
    } else {
      rendered.plain
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_images_an_older_version_kept_are_counted_and_removed_with_a_new_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let [guided, plain] = older_paths(dir.path(), "report");
    std::fs::write(&guided, [0; 10]).unwrap();
    std::fs::write(&plain, [0; 5]).unwrap();
    std::fs::write(columns_path(dir.path(), "report"), [0; 3]).unwrap();
    assert_eq!(
      kept_bytes(dir.path(), "report"),
      18,
      "counted, so the cache evicts them"
    );
    remove_older(dir.path(), "report");
    assert!(!guided.exists() && !plain.exists());
    assert_eq!(kept_bytes(dir.path(), "report"), 3);
  }
}
