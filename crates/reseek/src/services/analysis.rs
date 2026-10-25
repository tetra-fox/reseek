//! Audio analysis of downloaded files: the spectrum reading, the spectrogram and the user's
//! judgement.

use api_types::analysis::{
  AnalysisJob, AnalysisJobCreated, AnalysisListQuery, AudioAnalysis, SetAnalysisOverride,
  SpectrogramQuery,
};
use api_types::auth::Principal;
use async_trait::async_trait;
use axum::response::Response;

use super::{FileTarget, not_yet};
use crate::api::error::ApiError;

const DOMAIN: &str = "analysis";

/// Running analyses, their reports and spectrograms, and the user's own verdict.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait AnalysisService: Send + Sync {
  /// Queues an analysis of a file, again with `force` when it has one. One queued or running
  /// for the file already is a conflict.
  async fn run(&self, target: FileTarget, force: bool) -> Result<AnalysisJobCreated, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A file's report, with its per-window readings when asked for; 404 before the first
  /// analysis finishes.
  async fn report(&self, target: FileTarget, windows: bool) -> Result<AudioAnalysis, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A file's spectrogram image as the whole response.
  async fn spectrogram(
    &self,
    target: FileTarget,
    query: SpectrogramQuery,
  ) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Records the user's verdict on a file, which the report shows in place of the reading.
  async fn set_override(
    &self,
    by: &Principal,
    target: FileTarget,
    verdict: SetAnalysisOverride,
  ) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Takes the user's verdict on a file away.
  async fn clear_override(&self, target: FileTarget) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The reports, without their windows.
  async fn list(&self, query: AnalysisListQuery) -> Result<Vec<AudioAnalysis>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The analyses queued and running.
  async fn jobs(&self) -> Result<Vec<AnalysisJob>, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
