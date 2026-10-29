//! Finished downloads offered to the agents. Each agent logged in is told of every download
//! that reaches the downloads directory, by its path under that directory, with a token it
//! fetches the file with for the next ten minutes; whether it does is the agent's choice.

use std::path::{Component, Path};

use api_types::relay::RelayMessage;
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use tokio::time::Instant;
use tokio_util::io::ReaderStream;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Controller, Token};
use crate::api::error::ApiError;
use crate::relay::credential::Purpose;
use crate::relay::frames::{self, DOWNLOAD_TOKEN_LIFETIME, millis};
use crate::safety::{Os, is_absolute};
use crate::services::AgentCredential;
use crate::sync::lock;

/// `path` relative to `downloads` with this system's separators, `None` when it is not under
/// it.
fn relative(downloads: &Path, path: &Path) -> Option<String> {
  let rest = path.strip_prefix(downloads).ok()?;
  let parts: Vec<String> = rest
    .components()
    .map(|c| match c {
      Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
      _ => None,
    })
    .collect::<Option<_>>()?;
  (!parts.is_empty()).then(|| parts.join(&Os::CURRENT.separator().to_string()))
}

impl Controller {
  /// Tells every agent logged in of the finished download at `path`.
  pub async fn download_completed(&self, path: &Path) {
    let Some(filename) = relative(&self.downloads, path) else {
      warn!(path = %path.display(), "finished download is outside the downloads directory; agents not told");
      return;
    };
    let agents: Vec<(String, u64)> = lock(&self.registry)
      .agents
      .iter()
      .map(|(name, r)| (name.clone(), r.socket))
      .collect();
    if agents.is_empty() {
      return;
    }
    let size = match tokio::fs::metadata(path).await {
      Ok(metadata) => metadata.len(),
      Err(e) => {
        return warn!(path = %path.display(), error = %e, "finished download not read; agents not told");
      }
    };
    info!(
      filename,
      agents = agents.len(),
      "Notifying agents of download completion"
    );
    for (agent, socket) in agents {
      let token = Uuid::new_v4();
      {
        let mut registry = lock(&self.registry);
        let now = Instant::now();
        registry.download_tokens.retain(|_, t| t.expires > now);
        registry.download_tokens.insert(
          token,
          Token {
            agent: agent.clone(),
            socket,
            filename: Some(filename.clone()),
            expires: now + DOWNLOAD_TOKEN_LIFETIME,
          },
        );
      }
      let told = self.send(
        &agent,
        frames::encode(
          Some(token),
          RelayMessage::DownloadAvailable {
            filename: filename.clone(),
            token,
            size,
            expires_in: millis(DOWNLOAD_TOKEN_LIFETIME),
          },
        ),
      );
      if !told {
        warn!(
          agent,
          filename, "Failed to notify agent of download completion"
        );
      }
    }
  }

  /// The finished download `filename` the agent `caller` fetches with `token`, which stays
  /// valid for further attempts until it expires.
  pub async fn serve_download(
    &self,
    token: Uuid,
    caller: AgentCredential,
    filename: String,
  ) -> Result<Response, ApiError> {
    let served = self.open_download(token, caller, filename).await;
    let outcome = match &served {
      Ok(_) => "served",
      Err(e) if e.status() == StatusCode::NOT_FOUND => "missing",
      Err(_) => "refused",
    };
    self.count("download", outcome);
    served
  }

  async fn open_download(
    &self,
    token: Uuid,
    caller: AgentCredential,
    filename: String,
  ) -> Result<Response, ApiError> {
    let name = caller.agent.as_str();
    let announced = lock(&self.registry)
      .download_tokens
      .get(&token)
      .is_some_and(|t| {
        t.agent == name && t.expires > Instant::now() && t.filename.as_deref() == Some(&filename)
      });
    if !announced {
      warn!(agent = name, %token, filename, "download fetched with an unknown or expired token");
      return Err(ApiError::unauthorized("The token is unknown or expired").logged());
    }
    self.authenticate(&caller, Purpose::Download, token)?;
    if is_absolute(&filename, Os::CURRENT) {
      return Err(ApiError::bad_request("The filename is not relative"));
    }
    let source = self.downloads.join(&filename);
    let file = match tokio::fs::File::open(&source).await {
      Ok(file) => file,
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
        return Err(ApiError::not_found(format!("{filename} no longer exists")));
      }
      Err(e) => return Err(ApiError::internal(format!("{}: {e}", source.display()))),
    };
    let length = file
      .metadata()
      .await
      .map_err(|e| ApiError::internal(format!("{}: {e}", source.display())))?
      .len();
    debug!(
      agent = name,
      filename, length, "finished download sent to an agent"
    );
    let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
      header::CONTENT_TYPE,
      HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    Ok(response)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_download_is_named_by_its_path_under_the_directory_only() {
    let downloads = Path::new("/data/downloads");
    let sep = Os::CURRENT.separator();
    assert_eq!(
      relative(downloads, Path::new("/data/downloads/alice/Album/01.flac")).as_deref(),
      Some(format!("alice{sep}Album{sep}01.flac").as_str())
    );
    assert_eq!(
      relative(downloads, Path::new("/data/downloads2/alice/01.flac")),
      None,
      "a longer name is not under it"
    );
    assert_eq!(relative(downloads, Path::new("/elsewhere/01.flac")), None);
    assert_eq!(relative(downloads, downloads), None);
  }
}
