//! Reading a request: its JSON body, its query, its path parameters and the usernames and room
//! names in it. Anything that does not read is answered 400 in the body every error has, naming
//! the field that is wrong, before the handler runs.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{FromRequest, FromRequestParts, OptionalFromRequest, RawPathParams, Request};
use axum::http::header;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::error::Category;

use super::error::ApiError;

/// A JSON body read as `T`, or a JSON response. A body needs `Content-Type: application/json`;
/// `Option<Json<T>>` takes an empty body or `null` as none.
pub struct Json<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Json<T> {
  type Rejection = ApiError;

  async fn from_request(request: Request, state: &S) -> Result<Self, ApiError> {
    let bytes = json_bytes(request, state).await?;
    read_json(&bytes).map(Json)
  }
}

impl<S: Send + Sync, T: DeserializeOwned> OptionalFromRequest<S> for Json<T> {
  type Rejection = ApiError;

  async fn from_request(request: Request, state: &S) -> Result<Option<Self>, ApiError> {
    let bytes = json_bytes(request, state).await?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
      return Ok(None);
    }
    read_json::<Option<T>>(&bytes).map(|value| value.map(Json))
  }
}

impl<T: Serialize> IntoResponse for Json<T> {
  fn into_response(self) -> Response {
    axum::Json(self.0).into_response()
  }
}

/// The body of a request that says it is JSON; a body that says it is something else is
/// answered 415.
async fn json_bytes<S: Send + Sync>(request: Request, state: &S) -> Result<Bytes, ApiError> {
  let declared = request
    .headers()
    .get(header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .map(str::to_string);
  let bytes = Bytes::from_request(request, state)
    .await
    .map_err(|e: BytesRejection| {
      ApiError::invalid("Could not read the request body", e.body_text())
    })?;
  if bytes.is_empty() {
    return Ok(bytes);
  }
  match declared {
    Some(kind) if is_json(&kind) => Ok(bytes),
    _ => Err(ApiError::unsupported_media("The request body must be JSON")),
  }
}

/// A `Content-Type`'s media type without its parameters, in lower case: `text/plain` for
/// `Text/Plain; charset=utf-8`.
pub fn essence(content_type: &str) -> String {
  content_type
    .split(';')
    .next()
    .unwrap_or_default()
    .trim()
    .to_ascii_lowercase()
}

/// Whether a `Content-Type` is JSON: `application/json` or a `+json` type, with any
/// parameters.
fn is_json(content_type: &str) -> bool {
  let essence = essence(content_type);
  essence == "application/json"
    || (essence.starts_with("application/") && essence.ends_with("+json"))
}

/// `bytes` read as the JSON of `T`, refused with what was wrong; anything after the JSON is
/// refused too.
pub(in crate::api) fn read_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiError> {
  let reader = &mut serde_json::Deserializer::from_slice(bytes);
  let value = serde_path_to_error::deserialize(&mut *reader)
    .map_err(|e| refused_body(&e.path().to_string(), e.inner()))?;
  reader.end().map_err(|e| refused_body(".", &e))?;
  Ok(value)
}

/// The refusal of a body whose fault is at `at`. The log names the fault's kind, not serde's
/// text, which can quote a password, and names `at` only for a value that does not fit: a body
/// that is not JSON can fail inside a key the client made up.
fn refused_body(at: &str, error: &serde_json::Error) -> ApiError {
  let wrong = match error.classify() {
    Category::Syntax => "is not JSON",
    Category::Data => "does not fit what the route takes",
    Category::Eof => "is cut off",
    Category::Io => "was not read",
  };
  let subject = match error.classify() {
    Category::Data if at != "." => at,
    _ => "the body",
  };
  ApiError::invalid("Invalid request body", described(at, error))
    .logged_with(format!("{subject} {wrong}"))
}

/// A query read as `T`. A parameter may be repeated for a list, and one with an empty value,
/// as a cleared form field sends, counts as not given.
pub struct Query<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for Query<T> {
  type Rejection = ApiError;

  async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
    read_query(parts.uri.query().unwrap_or_default()).map(Query)
  }
}

fn read_query<T: DeserializeOwned>(query: &str) -> Result<T, ApiError> {
  // serde_html_form reads an empty value as `Some("")`, or as an error for any type but a
  // number or bool, so empty parameters are dropped before it sees them
  let given = query
    .split('&')
    .filter(|pair| {
      pair
        .split_once('=')
        .is_some_and(|(_, value)| !value.is_empty())
    })
    .collect::<Vec<_>>()
    .join("&");
  let reader = serde_html_form::Deserializer::from_bytes(given.as_bytes());
  serde_path_to_error::deserialize(reader).map_err(|e| {
    // serde's text can quote a value, which the request's logged path has masked where secret;
    // the parameter is one the route takes, as any other is ignored
    let at = e.path().to_string();
    let subject = if at == "." { "the query" } else { &at };
    ApiError::invalid("Invalid query", described(&at, e.inner()))
      .logged_with(format!("{subject} does not fit what the route takes"))
  })
}

/// `inner`, after the field `at` it is about when there is one.
fn described(at: &str, inner: &impl std::fmt::Display) -> String {
  if at == "." {
    inner.to_string()
  } else {
    format!("{at}: {inner}")
  }
}

/// The path parameters read as `T`, each percent-decoded once.
pub struct Path<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for Path<T> {
  type Rejection = ApiError;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
    axum::extract::Path::<T>::from_request_parts(parts, state)
      .await
      .map(|axum::extract::Path(value)| Path(value))
      .map_err(|e| ApiError::invalid("Invalid path", e.body_text()))
  }
}

/// The `{id}` path parameter read as `T`, beside parameters read some other way.
pub struct Id<T>(pub T);

#[derive(serde::Deserialize)]
struct IdParameter<T> {
  id: T,
}

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for Id<T> {
  type Rejection = ApiError;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
    let Path(IdParameter { id }) = Path::from_request_parts(parts, state).await?;
    Ok(Id(id))
  }
}

/// The user a request is about: `?username=` when given, else the `{username}` path parameter.
/// The query reaches names a path segment cannot hold, such as `..`.
pub struct NamedUser(pub String);

impl<S: Send + Sync> FromRequestParts<S> for NamedUser {
  type Rejection = ApiError;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
    named(parts, state, "username").await.map(NamedUser)
  }
}

/// The room a request is about: `?roomName=` when given, else the `{roomName}` path parameter.
pub struct NamedRoom(pub String);

impl<S: Send + Sync> FromRequestParts<S> for NamedRoom {
  type Rejection = ApiError;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
    named(parts, state, "roomName").await.map(NamedRoom)
  }
}

/// The value of the query parameter `key` when given, else of the path parameter `key`; empty
/// is refused.
async fn named<S: Send + Sync>(
  parts: &mut Parts,
  state: &S,
  key: &str,
) -> Result<String, ApiError> {
  let query = parts.uri.query().unwrap_or_default();
  let from_query = form_urlencoded::parse(query.as_bytes())
    .find(|(name, _)| name == key)
    .map(|(_, value)| value.into_owned());
  let value = match from_query {
    Some(value) => value,
    None => RawPathParams::from_request_parts(parts, state)
      .await
      .map_err(|e| ApiError::invalid("Invalid path", e.body_text()))?
      .iter()
      .find(|(name, _)| *name == key)
      .map(|(_, value)| value.to_string())
      .ok_or_else(|| ApiError::internal(format!("a route takes {key} it has not got")))?,
  };
  if value.is_empty() {
    return Err(ApiError::bad_request(format!(
      "A value for {key} is required"
    )));
  }
  Ok(value)
}

#[cfg(test)]
mod tests {
  use super::*;
  use api_types::transfers::{TransferListQuery, TransferStateCategory, TransferStateFilter};
  use api_types::users::UserListQuery;

  #[test]
  fn json_content_types_are_recognized_with_parameters_and_suffixes() {
    assert!(is_json("application/json"));
    assert!(is_json("Application/JSON; charset=utf-8"));
    assert!(is_json("application/merge-patch+json"));
    assert!(!is_json("text/plain"));
    assert!(!is_json("application/x-www-form-urlencoded"));
    assert!(!is_json("text/json+html"));
  }

  #[test]
  fn query_lists_repeat_and_errors_name_the_field() {
    let query: TransferListQuery = read_query("state=Failed&state=Queued%2C%20Remotely&limit=5")
      .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(query.limit, 5);
    assert_eq!(
      query.state[0],
      TransferStateFilter::Category(TransferStateCategory::Failed)
    );
    assert_eq!(query.state.len(), 2);
    let error = read_query::<TransferListQuery>("offset=-1").unwrap_err();
    assert_eq!(error.status(), 400);
    assert!(
      error.detail().unwrap_or_default().starts_with("offset: "),
      "{error:?}"
    );
  }

  /// What refusing with `error` logs.
  fn logged(error: ApiError) -> String {
    let (logs, _capturing) = crate::testing::captured_logs();
    drop(error.into_response());
    let lines: Vec<String> = logs.snapshot().into_iter().map(|l| l.message).collect();
    lines.join("\n")
  }

  #[test]
  fn a_body_with_anything_after_its_json_is_not_json() {
    for body in [&b"{} x"[..], b"{}{}", b"[1] 2"] {
      let error = read_json::<serde_json::Value>(body).unwrap_err();
      assert_eq!(error.status(), 400);
      assert!(logged(error).contains("the body is not JSON"));
    }
    assert!(read_json::<serde_json::Value>(b" {} \n").is_ok());
  }

  #[test]
  fn a_refused_body_logs_no_text_the_client_made_up() {
    // a key the route does not take, broken inside its value
    let error =
      read_json::<api_types::auth::session::LoginRequest>(br#"{"hunter2": [}"#).unwrap_err();
    let text = logged(error);
    assert!(text.contains("the body is not JSON"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
    let error = read_json::<api_types::auth::session::LoginRequest>(br#"{"password": 4815162342}"#)
      .unwrap_err();
    let text = logged(error);
    assert!(text.contains("password does not fit"), "{text}");
    assert!(!text.contains("4815162342"), "{text}");
  }

  #[test]
  fn a_refused_query_logs_the_parameter_and_not_its_value() {
    let error = read_query::<TransferListQuery>("sort=hunter2").unwrap_err();
    assert!(
      error.detail().unwrap_or_default().contains("hunter2"),
      "{error:?}"
    );
    let text = logged(error);
    assert!(text.contains("sort does not fit"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
  }

  #[test]
  fn empty_query_parameters_count_as_not_given() {
    let query: UserListQuery =
      read_query("q=&presence=&limit=5&group=").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(query.q, None);
    assert_eq!(query.presence, None);
    assert_eq!(query.group, None);
    assert_eq!(query.limit, 5);
    let query: TransferListQuery =
      read_query("since=&state=&state=Failed").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(query.since, None);
    assert_eq!(query.state.len(), 1);
  }
}
