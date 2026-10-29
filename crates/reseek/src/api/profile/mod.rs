//! Our own profile: the text and picture other users are sent when they ask for our user info,
//! read with the figures they are told, and changed where the settings page would change
//! `soulseek.description` and `soulseek.picture`: in the configuration file when it can be
//! written, else as a runtime override that lasts until removed.
//!
//! Reading takes any role; changing takes an administrator and `remote_configuration`, and a
//! change written to the file also someone at the UI rather than a stored API key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_types::auth::Principal;
use api_types::profile::{OwnProfile, ProfileSources, ProfileText};
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use cascade::{Lasts, Loaded, Source};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use serde_json::Value;
use tracing::{debug, info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, essence};
use super::options::remote;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, require_interactive};
use crate::config::{Options, view};
use crate::profile::{self, MAX_UPLOAD, UPLOAD_TYPES};
use crate::users::picture_response;

/// The option that holds the profile text.
const DESCRIPTION: &str = "soulseek.description";
/// The option that holds the path of the profile picture.
const PICTURE: &str = "soulseek.picture";
/// Where the picture peers are sent is fetched, without the URL base.
const PICTURE_URL: &str = "/api/v0/profile/picture";

/// The profile routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(get_profile, put_profile))
    .routes(routes!(
      get_profile_picture,
      put_profile_picture,
      delete_profile_picture
    ))
}

/// Our profile as a user asking for our user info is sent it now: the text, whether a picture
/// goes with it, and the slots and queue a user we know nothing about is told of, with where
/// the text and the picture come from.
#[utoipa::path(
  get, path = "/profile", tag = "profile",
  responses((status = 200, body = OwnProfile), (status = 401, body = api_types::ErrorBody))
)]
async fn get_profile(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Json<OwnProfile> {
  let has_picture = app.shares.has_picture().await;
  let forecast = app.uploads.default_forecast();
  let current = app.config.current();
  Json(OwnProfile {
    description: current.values.soulseek.description.clone(),
    has_picture,
    picture_url: has_picture.then(|| PICTURE_URL.to_string()),
    upload_slots: forecast.total_slots,
    free_upload_slot: forecast.free_slots > 0,
    queue_length: forecast.position,
    sources: ProfileSources {
      description: source_of(&current, DESCRIPTION),
      picture: source_of(&current, PICTURE),
    },
  })
}

/// Changes the profile text other users read.
#[utoipa::path(
  put, path = "/profile", tag = "profile", request_body = ProfileText,
  responses(
    (status = 204, description = "changed"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn put_profile(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Json(text): Json<ProfileText>,
) -> Result<StatusCode, ApiError> {
  remote(
    &app.config.current(),
    "Blocked profile change; remote configuration is disabled",
  )?;
  let _writing = app.profile_writes.lock().await;
  let target = target(&app.config.current(), &admin.principal, DESCRIPTION, false)?;
  let description = Value::String(text.description);
  write(
    &app,
    &admin.principal,
    DESCRIPTION,
    target,
    Some(description),
  )
  .await?;
  info!(by = admin.principal.subject, "profile text changed");
  Ok(StatusCode::NO_CONTENT)
}

/// The picture other users are sent with our profile.
#[utoipa::path(
  get, path = "/profile/picture", tag = "profile",
  responses(
    (status = 200, content_type = "image/*", body = api_types::files::FileContent),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody)
  )
)]
async fn get_profile_picture(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Response, ApiError> {
  let picture = app
    .shares
    .user_info()
    .await
    .picture
    .ok_or_else(|| ApiError::not_found("no profile picture is set"))?;
  // it changes whenever a new one is uploaded, so the browser asks again each time
  Ok(picture_response(picture, "private, no-cache"))
}

/// Sets the picture other users are sent: a JPEG, PNG, GIF or WebP image of at most 1 MiB,
/// kept as a JPEG at most 512 pixels on its longest side.
#[utoipa::path(
  put, path = "/profile/picture", tag = "profile",
  request_body(
    description = "the image",
    content(
      (api_types::files::FileContent = "image/jpeg"), (api_types::files::FileContent = "image/png"),
      (api_types::files::FileContent = "image/gif"), (api_types::files::FileContent = "image/webp")
    )
  ),
  responses(
    (status = 204, description = "changed"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 413, body = api_types::ErrorBody),
    (status = 415, body = api_types::ErrorBody)
  )
)]
async fn put_profile_picture(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  headers: HeaderMap,
  body: Body,
) -> Result<StatusCode, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked profile picture change; remote configuration is disabled",
  )?;
  let declared = headers
    .get(header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .map(essence)
    .unwrap_or_default();
  if !UPLOAD_TYPES.contains(&declared.as_str()) {
    return Err(ApiError::unsupported_media(
      "the picture must be a JPEG, PNG, GIF or WebP image",
    ));
  }
  // every refusal comes before the upload is read, so a refused one changes no picture
  target(&current, &admin.principal, PICTURE, false)?;
  let upload = match Limited::new(body, MAX_UPLOAD).collect().await {
    Ok(collected) => collected.to_bytes(),
    Err(e) if e.is::<LengthLimitError>() => {
      return Err(ApiError::too_large("the picture is larger than 1 MiB"));
    }
    Err(e) => {
      return Err(ApiError::invalid(
        "the body could not be read",
        e.to_string(),
      ));
    }
  };
  let jpeg = tokio::task::spawn_blocking(move || profile::reencode(&upload))
    .await
    .map_err(ApiError::internal)?
    .map_err(|e| {
      debug!(error = %e, kind = declared, "profile picture refused");
      ApiError::invalid("the picture could not be read", e.to_string()).logged()
    })?;
  let _writing = app.profile_writes.lock().await;
  // checked again under the lock, against the file as the change before this one left it
  let target = target(&app.config.current(), &admin.principal, PICTURE, false)?;
  let path = profile::stored_path(&app.paths.data);
  let pointed = Value::String(path.display().to_string());
  let staged = stage(&path, &jpeg).await.map_err(|e| {
    ApiError::internal(format!(
      "writing the picture next to {}: {e}",
      path.display()
    ))
  })?;
  let replaced = |e: std::io::Error| {
    ApiError::internal(format!("moving the picture to {}: {e}", path.display()))
  };
  if tokio::fs::try_exists(&path).await.map_err(replaced)? {
    // the old picture stays in place until the option is written, which can still be refused
    match write(&app, &admin.principal, PICTURE, target, Some(pointed)).await {
      Ok(()) => {}
      Err(Failed::Unwritten(e)) => {
        discard(&staged).await;
        return Err(e);
      }
      Err(Failed::Overridden(e)) => {
        tokio::fs::rename(&staged, &path).await.map_err(replaced)?;
        return Err(e);
      }
    }
    tokio::fs::rename(&staged, &path).await.map_err(replaced)?;
  } else {
    // the option is checked against a readable file, so the first picture goes in place first;
    // nothing pointed at it before and no other change runs, so only a refused write removes it
    tokio::fs::rename(&staged, &path).await.map_err(replaced)?;
    match write(&app, &admin.principal, PICTURE, target, Some(pointed)).await {
      Ok(()) => {}
      Err(Failed::Unwritten(e)) => {
        discard(&path).await;
        return Err(e);
      }
      Err(Failed::Overridden(e)) => return Err(e),
    }
  }
  info!(
    by = admin.principal.subject,
    bytes = jpeg.len(),
    "profile picture changed"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Stops sending a picture with our profile and removes the one kept.
#[utoipa::path(
  delete, path = "/profile/picture", tag = "profile",
  responses(
    (status = 204, description = "removed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody)
  )
)]
async fn delete_profile_picture(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
) -> Result<StatusCode, ApiError> {
  remote(
    &app.config.current(),
    "Blocked profile picture removal; remote configuration is disabled",
  )?;
  let _writing = app.profile_writes.lock().await;
  let target = target(&app.config.current(), &admin.principal, PICTURE, true)?;
  write(&app, &admin.principal, PICTURE, target, None).await?;
  let path = profile::stored_path(&app.paths.data);
  match tokio::fs::remove_file(&path).await {
    Ok(()) => {}
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
    Err(e) => {
      return Err(ApiError::internal(format!(
        "removing the picture at {}: {e}",
        path.display()
      )));
    }
  }
  info!(by = admin.principal.subject, "profile picture removed");
  Ok(StatusCode::NO_CONTENT)
}

/// The layer that sets `key`, as the API writes it.
fn source_of(current: &Loaded<Options>, key: &str) -> api_types::options::OptionSource {
  view::source(current.sources.get(key).cloned().unwrap_or(Source::Default))
}

/// Refuses a change of `key` when the environment or the command line sets it.
fn locked(current: &Loaded<Options>, key: &str) -> Result<(), ApiError> {
  match current.sources.get(key).and_then(Source::fixed_by) {
    Some(by) => Err(
      cascade::Error::SetBy {
        key: key.to_string(),
        by: by.to_string(),
      }
      .into(),
    ),
    None => Ok(()),
  }
}

/// Where a change of a profile option goes.
enum Target {
  /// the configuration file `name`, as it was read at `revision`
  File {
    name: String,
    revision: Option<String>,
  },
  /// a runtime override lasting until removed
  Override,
}

/// Where a change of `key` by `by` goes, as a save from the settings page would: the file a
/// write of the key goes to when it can be written, which takes someone at the UI, else a
/// runtime override. Refused, before anything changes, for a key the environment or the
/// command line sets, and for taking out (`unsetting`) a key a file that can not be written
/// sets, also under a runtime override, since the file's value would then be in effect.
fn target(
  current: &Loaded<Options>,
  by: &Principal,
  key: &str,
  unsetting: bool,
) -> Result<Target, ApiError> {
  locked(current, key)?;
  let file = current
    .file_for(key)
    .and_then(|name| current.files.iter().find(|f| f.name == name))
    .ok_or_else(|| ApiError::internal("the configuration has no files"))?;
  let reason = match cascade::writability(&file.path, current.managed_by.as_deref()) {
    Ok(()) => {
      require_interactive(by)?;
      return Ok(Target::File {
        name: file.name.clone(),
        revision: file.revision.clone(),
      });
    }
    Err(reason) => reason,
  };
  // the file that sets a key is the one a save of it goes to, which can not be written
  if let (true, Some(file)) = (unsetting, current.file_setting(key)) {
    warn!(
      key,
      file, reason, "profile value can not be taken out of its file"
    );
    let detail = format!("{file} sets {key}, which can not be changed from here: {reason}");
    let refused = ApiError::conflict(format!("{file} is read-only"), Some(detail));
    return Err(refused.logged());
  }
  Ok(Target::Override)
}

/// How a write of a profile option failed.
enum Failed {
  /// nothing was changed
  Unwritten(ApiError),
  /// the value was written, but a runtime override of the key from before could not be removed
  Overridden(ApiError),
}

impl From<Failed> for ApiError {
  fn from(failed: Failed) -> Self {
    match failed {
      Failed::Unwritten(e) | Failed::Overridden(e) => e,
    }
  }
}

/// Sets `key` to `value`, or takes it out with `None`, at `target`. A runtime override of the
/// key from before is removed after the write, so the value written is the one in effect.
async fn write(
  app: &AppState,
  by: &Principal,
  key: &str,
  target: Target,
  value: Option<Value>,
) -> Result<(), Failed> {
  if let (Target::Override, Some(value)) = (&target, &value) {
    let set = BTreeMap::from([(key.to_string(), value.clone())]);
    app
      .config
      .set_overrides(set, Vec::new(), &by.subject, Lasts::UntilRemoved)
      .await
      .map_err(|e| Failed::Unwritten(e.into()))?;
    debug!(key, "profile set as a runtime override");
    return Ok(());
  }
  if let Target::File { name, revision } = &target {
    let (set, unset) = match value {
      Some(value) => (BTreeMap::from([(key.to_string(), value)]), Vec::new()),
      None => (BTreeMap::new(), vec![key.to_string()]),
    };
    app
      .config
      .patch_file(name, revision.as_deref(), set, unset)
      .await
      .map_err(|e| Failed::Unwritten(e.into()))?;
    debug!(
      key,
      file = name,
      "profile written to the configuration file"
    );
  }
  if app.config.current().overrides.contains_key(key) {
    let unset = vec![key.to_string()];
    app
      .config
      .set_overrides(BTreeMap::new(), unset, &by.subject, Lasts::UntilRemoved)
      .await
      .map_err(|e| Failed::Overridden(e.into()))?;
    debug!(key, "runtime override of the profile removed");
  }
  Ok(())
}

/// Writes `bytes` to a file of its own next to `path`, to be moved over it once the change is
/// accepted, so a reader never sees half a picture and two uploads never share a file.
async fn stage(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
  let dir = path.parent().expect("the kept picture is in a directory");
  tokio::fs::create_dir_all(dir).await?;
  let staged = dir.join(format!("picture.{}.part", Uuid::new_v4()));
  tokio::fs::write(&staged, bytes).await?;
  Ok(staged)
}

/// Removes a picture a refused change left behind.
async fn discard(path: &Path) {
  if let Err(e) = tokio::fs::remove_file(path).await {
    warn!(path = %path.display(), error = %e, "picture of a refused change not removed");
  }
}

#[cfg(test)]
mod tests;
