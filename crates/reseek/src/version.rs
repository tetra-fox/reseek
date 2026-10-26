//! The running version, and the check of the release feed for a newer one.

use std::time::Duration;

use api_types::state::VersionInfo;
use tracing::info;

use crate::state::StateOwner;

/// The revision the build was made from, when the build says.
const REVISION: Option<&str> = option_env!("RESEEK_GIT_REVISION");

/// The feed that names the latest release.
pub const RELEASE_FEED: &str = "https://api.github.com/repos/tetra-fox/reseek/releases/latest";

/// How often a running daemon reads the release feed again.
pub const RECHECK: Duration = Duration::from_secs(24 * 3600);

/// The running version: `full` is `<semver> (<semver>+<revision>)`, the revision left out when
/// the build does not name one. A debug build is a development build.
pub fn info() -> VersionInfo {
  let current = env!("CARGO_PKG_VERSION").to_string();
  let informational = match REVISION {
    Some(revision) => format!("{current}+{revision}"),
    None => current.clone(),
  };
  VersionInfo {
    full: format!("{current} ({informational})"),
    current,
    latest: None,
    is_update_available: None,
    is_canary: false,
    is_development: cfg!(debug_assertions),
  }
}

/// Reads the release feed.
pub struct Releases {
  http: reqwest::Client,
  feed: String,
}

impl Releases {
  /// A reader of the release feed at `feed`.
  pub fn new(feed: &str) -> Result<Self, String> {
    Ok(Self {
      http: crate::http::outbound::client(None)?,
      feed: feed.to_string(),
    })
  }

  /// `current` with the latest release the feed names, and whether it is newer. A development
  /// or canary build is not checked and comes back as it is.
  pub async fn check(&self, current: VersionInfo) -> Result<VersionInfo, String> {
    if current.is_development {
      info!("Skipping version check for Development build");
      return Ok(current);
    }
    if current.is_canary {
      info!("Skipping version check for Canary build; check for updates manually.");
      return Ok(current);
    }
    info!(
      feed = self.feed,
      "Checking GitHub Releases for latest version"
    );
    let latest = self.latest(&current.full).await?;
    let checked = with_latest(current, &latest)?;
    if checked.is_update_available == Some(true) {
      info!(
        current = checked.current,
        latest, "A new version is available!"
      );
    } else {
      info!(current = checked.current, "Version is up to date.");
    }
    Ok(checked)
  }

  /// Checks the version `state` holds and puts the outcome into it.
  pub async fn update(&self, state: &StateOwner) -> Result<VersionInfo, String> {
    let checked = self.check(state.current().version.clone()).await?;
    let version = checked.clone();
    state.update(|state| state.version = version);
    Ok(checked)
  }

  /// The tag of the latest release.
  async fn latest(&self, full: &str) -> Result<String, String> {
    let response = self
      .http
      .get(&self.feed)
      .header("User-Agent", format!("reseek v{full}"))
      .header("Accept", "application/vnd.github+json")
      .send()
      .await
      .and_then(reqwest::Response::error_for_status)
      .map_err(|e| e.to_string())?;
    let body = response.text().await.map_err(|e| e.to_string())?;
    let release: serde_json::Value =
      serde_json::from_str(&body).map_err(|e| format!("the release feed is not JSON: {e}"))?;
    release["tag_name"]
      .as_str()
      .map(str::to_string)
      .ok_or_else(|| "the release feed names no tag_name".to_string())
  }
}

/// `info` with `tag`, the latest release, as its `latest` when newer, else its own version.
fn with_latest(mut info: VersionInfo, tag: &str) -> Result<VersionInfo, String> {
  let latest = semver(tag)?;
  let current = semver(&info.current)?;
  let newer = latest > current;
  info.latest = Some(if newer {
    tag.trim_start_matches('v').to_string()
  } else {
    info.current.clone()
  });
  info.is_update_available = Some(newer);
  Ok(info)
}

/// `MAJOR.MINOR.PATCH` of a version or tag, with an optional leading `v` and build metadata
/// after a `+`.
fn semver(text: &str) -> Result<(u64, u64, u64), String> {
  let core = text.trim_start_matches('v');
  let core = core.split('+').next().unwrap_or(core);
  let parts: Vec<&str> = core.split('.').collect();
  let number = |part: &str| {
    part
      .parse::<u64>()
      .map_err(|_| format!("`{text}` is not a version"))
  };
  match parts.as_slice() {
    [major, minor, patch] => Ok((number(major)?, number(minor)?, number(patch)?)),
    _ => Err(format!("`{text}` is not a version")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  fn released(current: &str) -> VersionInfo {
    VersionInfo {
      full: format!("{current} ({current})"),
      current: current.to_string(),
      latest: None,
      is_update_available: None,
      is_canary: false,
      is_development: false,
    }
  }

  /// A feed on loopback that answers one request with `status` and `body`, and hands back the
  /// request it read.
  async fn feed(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/releases/latest", listener.local_addr().unwrap());
    let answer = format!(
      "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
       Connection: close\r\n\r\n{body}",
      body.len()
    );
    let served = tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      let mut request = Vec::new();
      let mut buffer = [0u8; 1024];
      while !request.ends_with(b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.unwrap();
        request.extend_from_slice(&buffer[..read]);
      }
      stream.write_all(answer.as_bytes()).await.unwrap();
      String::from_utf8(request).unwrap()
    });
    (url, served)
  }

  #[tokio::test]
  async fn a_newer_release_is_an_update_and_the_same_one_is_not() {
    let (url, served) = feed("200 OK", r#"{"tag_name":"v0.3.0","name":"0.3.0"}"#).await;
    let checked = Releases::new(&url)
      .unwrap()
      .check(released("0.2.9"))
      .await
      .unwrap();
    assert_eq!(checked.latest.as_deref(), Some("0.3.0"));
    assert_eq!(checked.is_update_available, Some(true));
    let request = served.await.unwrap().to_ascii_lowercase();
    assert!(
      request.contains("user-agent: reseek v0.2.9 (0.2.9)"),
      "{request}"
    );

    let (url, _) = feed("200 OK", r#"{"tag_name":"0.2.9"}"#).await;
    let checked = Releases::new(&url)
      .unwrap()
      .check(released("0.2.9"))
      .await
      .unwrap();
    assert_eq!(checked.latest.as_deref(), Some("0.2.9"));
    assert_eq!(checked.is_update_available, Some(false));
  }

  #[tokio::test]
  async fn a_failed_check_says_why_and_a_development_build_is_not_checked() {
    let (url, _) = feed("404 Not Found", r#"{"message":"Not Found"}"#).await;
    let failed = Releases::new(&url)
      .unwrap()
      .check(released("0.2.9"))
      .await
      .unwrap_err();
    assert!(failed.contains("404"), "{failed}");
    let (url, _) = feed("200 OK", r#"{"tag_name":"latest"}"#).await;
    let failed = Releases::new(&url)
      .unwrap()
      .check(released("0.2.9"))
      .await
      .unwrap_err();
    assert!(failed.contains("not a version"), "{failed}");

    let development = VersionInfo {
      is_development: true,
      ..released("0.0.1")
    };
    let unchecked = Releases::new("http://192.0.2.1/unreachable")
      .unwrap()
      .check(development.clone())
      .await
      .unwrap();
    assert_eq!(unchecked, development);
  }

  #[test]
  fn versions_compare_by_their_numbers() {
    assert_eq!(semver("v1.2.3"), Ok((1, 2, 3)));
    assert_eq!(semver("0.10.0+abc"), Ok((0, 10, 0)));
    assert!(semver("0.10.0+abc") > semver("0.9.12"));
    assert!(semver("1.2").is_err());
    assert!(semver("1.2.3-rc1").is_err());
  }
}
