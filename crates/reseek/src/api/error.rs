//! The one error type of the API's handlers: each kind has its status, and the body is always
//! `{ "error", "detail" }`.

use api_types::ErrorBody;
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

/// What went wrong in a handler: the status that answers it, the message and its detail.
#[derive(Debug)]
pub struct ApiError {
  status: StatusCode,
  error: String,
  detail: Option<String>,
  /// the full reason of a 500, logged and not sent
  internal: Option<String>,
  /// the handler logged the failure itself, so the response does not log it again
  logged: bool,
  /// seconds to send in `Retry-After`
  retry_after: Option<u32>,
}

impl ApiError {
  fn new(status: StatusCode, error: impl Into<String>, detail: Option<String>) -> Self {
    Self {
      status,
      error: error.into(),
      detail,
      internal: None,
      logged: false,
      retry_after: None,
    }
  }

  /// A 400 with a message: the request is malformed or a value is not valid.
  pub fn bad_request(error: impl Into<String>) -> Self {
    Self::new(StatusCode::BAD_REQUEST, error, None)
  }

  /// A 400 with a message and more about it.
  pub fn invalid(error: impl Into<String>, detail: impl Into<String>) -> Self {
    Self::new(StatusCode::BAD_REQUEST, error, Some(detail.into()))
  }

  /// A 415: the body is not of a type the call reads.
  pub fn unsupported_media(error: impl Into<String>) -> Self {
    Self::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, error, None)
  }

  /// A 401: no credential, or one that is not valid.
  pub fn unauthorized(error: impl Into<String>) -> Self {
    Self::new(StatusCode::UNAUTHORIZED, error, None)
  }

  /// A 403: the principal's role or credential may not do this, or the feature is off.
  pub fn forbidden(error: impl Into<String>) -> Self {
    Self::new(StatusCode::FORBIDDEN, error, None)
  }

  /// A 403 with a fixed code and why, for a call this instance does not take at all.
  pub fn disabled(error: impl Into<String>, detail: impl Into<String>) -> Self {
    Self::new(StatusCode::FORBIDDEN, error, Some(detail.into()))
  }

  /// A 429 for an address that failed too often lately, telling it to wait a minute.
  pub fn rate_limited() -> Self {
    let mut e = Self::new(
      StatusCode::TOO_MANY_REQUESTS,
      "too many failed attempts",
      Some("try again in a minute".to_string()),
    );
    e.retry_after = Some(60);
    e
  }

  /// A 429: the principal holds as many of something as it may.
  pub fn too_many(error: impl Into<String>) -> Self {
    Self::new(StatusCode::TOO_MANY_REQUESTS, error, None)
  }

  /// A 404: the thing asked for does not exist.
  pub fn not_found(error: impl Into<String>) -> Self {
    Self::new(StatusCode::NOT_FOUND, error, None)
  }

  /// A 413: what was asked for is larger than a configured limit.
  pub fn too_large(error: impl Into<String>) -> Self {
    Self::new(StatusCode::PAYLOAD_TOO_LARGE, error, None)
  }

  /// A 416: the byte range asked for is not in the file.
  pub fn range_not_satisfiable(error: impl Into<String>) -> Self {
    Self::new(StatusCode::RANGE_NOT_SATISFIABLE, error, None)
  }

  /// A 409: the state does not allow it, such as a duplicate or a stale revision.
  pub fn conflict(error: impl Into<String>, detail: Option<String>) -> Self {
    Self::new(StatusCode::CONFLICT, error, detail)
  }

  /// A 502: a peer or the server failed.
  pub fn upstream(error: impl Into<String>, detail: Option<String>) -> Self {
    Self::new(StatusCode::BAD_GATEWAY, error, detail)
  }

  /// A 503: the Soulseek connection the call needs is not there.
  pub fn unavailable(detail: Option<String>) -> Self {
    Self::new(
      StatusCode::SERVICE_UNAVAILABLE,
      "soulseek_unavailable",
      detail,
    )
  }

  /// A 503 for something other than the Soulseek connection that is not there now.
  pub fn service_unavailable(error: impl Into<String>) -> Self {
    Self::new(StatusCode::SERVICE_UNAVAILABLE, error, None)
  }

  /// A 500 for `error`, which is logged and not sent.
  pub fn internal(error: impl std::fmt::Display) -> Self {
    let mut e = Self::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "internal error",
      Some("the reason is in the daemon's log".to_string()),
    );
    e.internal = Some(error.to_string());
    e
  }

  /// The same error, which the handler has logged itself.
  pub fn logged(mut self) -> Self {
    self.logged = true;
    self
  }

  /// The status that answers the error.
  pub fn status(&self) -> StatusCode {
    self.status
  }

  /// The longer explanation sent with the message, when there is one.
  pub fn detail(&self) -> Option<&str> {
    self.detail.as_deref()
  }
}

impl IntoResponse for ApiError {
  fn into_response(self) -> Response {
    let status = self.status;
    // the request's span, which every handler runs in, names the request
    match (&self.internal, self.logged) {
      (_, true) => {}
      (Some(reason), false) => tracing::error!(%status, error = reason, "request failed"),
      (None, false) => {
        tracing::debug!(%status, error = self.error, detail = self.detail, "request refused");
      }
    }
    let body = ErrorBody {
      error: self.error,
      detail: self.detail,
    };
    let mut response = (status, Json(body)).into_response();
    if let Some(seconds) = self.retry_after {
      response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
    }
    if status == StatusCode::UNAUTHORIZED {
      response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
  }
}

impl From<cascade::Error> for ApiError {
  fn from(error: cascade::Error) -> Self {
    use cascade::Error as E;
    match error {
      E::Stale => Self::conflict("config file changed on disk", None),
      E::ReadOnly { file, reason } => Self::conflict(format!("{file} is read-only"), Some(reason)),
      E::SetBy { key, by } => Self::conflict(format!("set by {by}"), Some(key)),
      E::Invalid(invalid) => Self::invalid(
        "invalid configuration",
        invalid
          .iter()
          .map(ToString::to_string)
          .collect::<Vec<_>>()
          .join("; "),
      ),
      e @ (E::Value { .. } | E::Parse { .. }) => {
        Self::invalid("invalid configuration", e.to_string())
      }
      E::UnknownKey(key) => Self::bad_request(format!("unknown key {key}")),
      E::NeedsRestart(key) => Self::invalid("needs a restart; write it to the file", key),
      E::NoOverride(key) => Self::not_found(format!("no runtime override for {key}")),
      E::UnknownFile(file) => Self::not_found(format!("{file} is not a configuration file")),
      e @ (E::Read { .. } | E::Write { .. } | E::Overrides { .. } | E::Stopped) => {
        Self::internal(e)
      }
    }
  }
}
