//! The file calls: listing and deleting under the download directories, a file's or a
//! directory's bytes, the link token, and audio analysis.

use api_types::analysis::{
  AnalysisJob, AnalysisJobCreated, AnalysisListQuery, AnalysisReportQuery, AudioAnalysis,
  RunAnalysisQuery, SetAnalysisOverride, SpectrogramTile, TileQuery, WaveformTile, WindowSpectra,
  WindowSpectrum,
};
use api_types::files::{
  ArchiveQuery, ContentQuery, DirectoryListingQuery, FileRoot, FileToken, FileTokenRequest,
  FilesystemDirectory,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use uuid::Uuid;

use reqwest::Method;
use reqwest::header::HeaderMap;

use super::{Client, Framed, Result, Route, bytes, done, framed, json, send};

/// A file under a root, or the file a succeeded download was moved to.
#[derive(Clone, Copy, Debug)]
pub enum LocalFile<'a> {
  /// a file under a root, by its `/`-separated path relative to the root
  Under(FileRoot, &'a str),
  /// the file of a succeeded download
  Download(&'a str, Uuid),
}

/// The route of a root's `kind`, `directories` or `files`, at `path` when it is not the root.
fn under(root: FileRoot, kind: &str, path: &str) -> Route {
  let root = match root {
    FileRoot::Downloads => "downloads",
    FileRoot::Incomplete => "incomplete",
  };
  let route = Route::new("files").then(root).then(kind);
  if path.is_empty() {
    return route;
  }
  route.segment(&URL_SAFE_NO_PAD.encode(path))
}

/// The route of a file, as a file under a root or as a download's.
fn file(target: LocalFile<'_>) -> Route {
  match target {
    LocalFile::Under(root, path) => under(root, "files", path),
    LocalFile::Download(username, id) => Route::new("transfers/downloads")
      .segment(username)
      .segment(&id.to_string()),
  }
}

impl Client {
  /// `GET /files/{root}/directories[/{path}]`: a directory, the root itself for an empty path.
  pub async fn list_directory(
    &self,
    root: FileRoot,
    path: &str,
    recursive: bool,
  ) -> Result<FilesystemDirectory> {
    let route = under(root, "directories", path).query(&DirectoryListingQuery { recursive })?;
    json(self.get(route)).await
  }

  /// `DELETE /files/{root}/directories/{path}`: deletes a directory and what is in it.
  pub async fn delete_directory(&self, root: FileRoot, path: &str) -> Result<()> {
    done(self.delete(under(root, "directories", path))).await
  }

  /// `DELETE /files/{root}/files/{path}`: deletes a file.
  pub async fn delete_file(&self, root: FileRoot, path: &str) -> Result<()> {
    done(self.delete(under(root, "files", path))).await
  }

  /// `GET .../content`: a file's bytes.
  pub async fn file_content(&self, target: LocalFile<'_>, query: &ContentQuery) -> Result<Bytes> {
    bytes(self.get(file(target).then("content").query(query)?)).await
  }

  /// `HEAD .../content`: a file's headers alone, such as its length and `ETag`.
  pub async fn file_content_headers(
    &self,
    target: LocalFile<'_>,
    query: &ContentQuery,
  ) -> Result<HeaderMap> {
    let route = file(target).then("content").query(query)?;
    let mut response = send(self.request(Method::HEAD, &route)).await?;
    Ok(std::mem::take(response.headers_mut()))
  }

  /// `GET /files/{root}/directories/{path}/archive`: a directory as a zip archive.
  pub async fn archive(&self, root: FileRoot, path: &str, name: Option<&str>) -> Result<Bytes> {
    let query = ArchiveQuery {
      name: name.map(str::to_string),
    };
    let route = under(root, "directories", path)
      .then("archive")
      .query(&query)?;
    bytes(self.get(route)).await
  }

  /// `POST /session/file-token`: a token that lets a link load one file.
  pub async fn file_token(&self, request: &FileTokenRequest) -> Result<FileToken> {
    json(self.post(Route::new("session/file-token")).json(request)).await
  }

  /// `POST .../analysis`: queues a file for analysis.
  pub async fn run_analysis(
    &self,
    target: LocalFile<'_>,
    force: bool,
  ) -> Result<AnalysisJobCreated> {
    let route = file(target)
      .then("analysis")
      .query(&RunAnalysisQuery { force })?;
    json(self.post(route)).await
  }

  /// `GET .../analysis`: a file's analysis report.
  pub async fn analysis(
    &self,
    target: LocalFile<'_>,
    query: &AnalysisReportQuery,
  ) -> Result<AudioAnalysis> {
    let route = file(target).then("analysis").query(query)?;
    json(self.get(route)).await
  }

  /// `GET .../analysis/spectrogram`: a file's spectrogram, the overview or a tile.
  pub async fn spectrogram(
    &self,
    target: LocalFile<'_>,
    query: &TileQuery,
  ) -> Result<Framed<SpectrogramTile>> {
    let route = file(target).then("analysis/spectrogram").query(query)?;
    framed(self.get(route)).await
  }

  /// `GET .../analysis/waveform`: a file's waveform, the overview or a tile.
  pub async fn waveform(
    &self,
    target: LocalFile<'_>,
    query: &TileQuery,
  ) -> Result<Framed<WaveformTile>> {
    let route = file(target).then("analysis/waveform").query(query)?;
    framed(self.get(route)).await
  }

  /// `GET .../analysis/spectra`: the spectrum of every window of a file's analysis.
  pub async fn window_spectra(&self, target: LocalFile<'_>) -> Result<Framed<WindowSpectra>> {
    framed(self.get(file(target).then("analysis/spectra"))).await
  }

  /// `PUT .../analysis/override`: records our verdict on a file.
  pub async fn set_analysis_override(
    &self,
    target: LocalFile<'_>,
    verdict: &SetAnalysisOverride,
  ) -> Result<()> {
    let route = file(target).then("analysis/override");
    done(self.put(route).json(verdict)).await
  }

  /// `DELETE .../analysis/override`: takes our verdict away.
  pub async fn clear_analysis_override(&self, target: LocalFile<'_>) -> Result<()> {
    done(self.delete(file(target).then("analysis/override"))).await
  }

  /// `GET /analysis`: the analysis reports.
  pub async fn analyses(&self, query: &AnalysisListQuery) -> Result<Vec<AudioAnalysis>> {
    json(self.get(Route::new("analysis").query(query)?)).await
  }

  /// `GET /analysis/jobs`: the analyses queued and running.
  pub async fn analysis_jobs(&self) -> Result<Vec<AnalysisJob>> {
    json(self.get(Route::new("analysis/jobs"))).await
  }

  /// `DELETE /analysis/jobs/{id}`: takes a queued analysis off the queue or stops a running one.
  pub async fn cancel_analysis_job(&self, id: Uuid) -> Result<()> {
    done(self.delete(Route::new("analysis/jobs").then(&id.to_string()))).await
  }

  /// `GET .../analysis/windows/{index}`: the spectrum of one window of a file's analysis.
  pub async fn analysis_window(&self, target: LocalFile<'_>, index: u32) -> Result<WindowSpectrum> {
    let route = file(target)
      .then("analysis/windows")
      .then(&index.to_string());
    json(self.get(route)).await
  }
}
