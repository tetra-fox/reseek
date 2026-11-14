//! Our own profile: the text and picture other users are sent when they ask for our user info,
//! read with the figures they are told and what we like and hate, and changed where the
//! settings page would change `soulseek.description`, `soulseek.picture` and
//! `soulseek.interests`: in the configuration file when it can be written, else as a runtime
//! override that lasts until removed.
//!
//! Reading takes any role; changing takes an administrator and `remote_configuration`, and a
//! change written to the file also someone at the UI rather than a stored API key.

mod password;
mod recommendations;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_types::auth::Principal;
use api_types::options::{OptionsChanged, RevisionQuery};
use api_types::profile::{Interests, OwnProfile, ProfileSources, ProfileText};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use cascade::{Change, Lasts, Loaded, Source};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use serde_json::Value;
use tracing::{debug, info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, essence};
use super::options::{kept, remote};
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, require_interactive};
use crate::config::{Options, view};
use crate::profile::{self, MAX_UPLOAD, UPLOAD_TYPES};
use crate::users::{HATED, LIKED, picture_response, picture_tag};

/// The option that holds the profile text.
const DESCRIPTION: &str = "soulseek.description";
/// The option that holds the path of the profile picture.
const PICTURE: &str = "soulseek.picture";
/// Where the picture peers are sent is fetched, without the URL base and the version.
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
    .routes(routes!(put_interests))
    .merge(recommendations::routes())
    .merge(password::routes())
}

/// Our profile as a user asking for our user info is sent it now: the text, whether a picture
/// goes with it, and the slots and queue a user we know nothing about is told of, with what we
/// like and hate and where each comes from.
#[utoipa::path(
  get, path = "/profile", tag = "profile",
  responses((status = 200, body = OwnProfile), (status = 401, body = api_types::ErrorBody))
)]
async fn get_profile(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Json<OwnProfile> {
  let current = app.config.current();
  let picture = profile::picture(&current.values).await;
  let forecast = app.uploads.default_forecast();
  let interests = &current.values.soulseek.interests;
  Json(OwnProfile {
    description: current.values.soulseek.description.clone(),
    has_picture: picture.is_some(),
    // the URL changes with the picture, so a page showing the old one fetches the new one
    picture_url: picture.map(|p| format!("{PICTURE_URL}?v={}", picture_tag(&p))),
    upload_slots: forecast.total_slots,
    free_upload_slot: forecast.free_slots > 0,
    queue_length: forecast.queue_length,
    interests: Interests {
      liked: interests.liked.clone(),
      hated: interests.hated.clone(),
    },
    sources: ProfileSources {
      description: source_of(&current, DESCRIPTION),
      picture: source_of(&current, PICTURE),
      liked: source_of(&current, LIKED),
      hated: source_of(&current, HATED),
    },
    options_revision: current.revision.clone(),
  })
}

/// Changes the profile text other users read. A change sent with a `revision` the options have
/// changed since is refused with 409, the current revision as its detail.
#[utoipa::path(
  put, path = "/profile", tag = "profile", request_body = ProfileText, params(RevisionQuery),
  responses(
    (status = 200, body = OptionsChanged), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn put_profile(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Query(query): Query<RevisionQuery>,
  Json(text): Json<ProfileText>,
) -> Result<Json<OptionsChanged>, ApiError> {
  remote(
    &app.config.current(),
    "Blocked profile change; remote configuration is disabled",
  )?;
  let _writing = app.profile_writes.lock().await;
  let target = target(&app.config.current(), &admin.principal, DESCRIPTION, false)?;
  let description = Value::String(text.description);
  let set = BTreeMap::from([(DESCRIPTION.to_string(), description)]);
  let read = query.revision.as_deref();
  let change = write(&app, &admin.principal, target, set, Vec::new(), read).await?;
  info!(by = admin.principal.subject, "profile text changed");
  Ok(Json(view::changed(change.as_deref(), &kept(&app, read))))
}

/// Replaces what we like and hate, each list in the order listed. The server is told what
/// changed, now when logged in and at the next login otherwise. A change sent with a `revision`
/// the options have changed since is refused with 409, the current revision as its detail.
#[utoipa::path(
  put, path = "/profile/interests", tag = "profile", request_body = Interests,
  params(RevisionQuery),
  responses(
    (status = 200, body = OptionsChanged), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn put_interests(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Query(query): Query<RevisionQuery>,
  Json(interests): Json<Interests>,
) -> Result<Json<OptionsChanged>, ApiError> {
  remote(
    &app.config.current(),
    "Blocked interests change; remote configuration is disabled",
  )?;
  let _writing = app.profile_writes.lock().await;
  let current = app.config.current();
  // both lists go in one write, so an interest moved from one to the other is never in both
  let liked_to = target(&current, &admin.principal, LIKED, false)?;
  if liked_to != target(&current, &admin.principal, HATED, false)? {
    let detail = format!("Set {LIKED} and {HATED} in one file to change them here");
    return Err(ApiError::conflict(
      "The interests are set in different files",
      Some(detail),
    ));
  }
  let (liked, hated) = (interests.liked.len(), interests.hated.len());
  let set = BTreeMap::from([
    (LIKED.to_string(), Value::from(interests.liked)),
    (HATED.to_string(), Value::from(interests.hated)),
  ]);
  let read = query.revision.as_deref();
  let change = write(&app, &admin.principal, liked_to, set, Vec::new(), read).await?;
  info!(
    by = admin.principal.subject,
    liked, hated, "interests changed"
  );
  Ok(Json(view::changed(change.as_deref(), &kept(&app, read))))
}

/// The picture other users are sent with our profile.
#[utoipa::path(
  get, path = "/profile/picture", tag = "profile",
  params(api_types::profile::PictureVersion),
  responses(
    (status = 200, content_type = "image/*", body = api_types::files::FileContent),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody)
  )
)]
async fn get_profile_picture(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Response, ApiError> {
  let picture = profile::picture(&app.config.current().values)
    .await
    .ok_or_else(|| ApiError::not_found("No profile picture is set"))?;
  // the profile names it by a URL that changes with it, so a copy kept a day is never stale
  Ok(picture_response(picture, "private, max-age=86400"))
}

/// Sets the picture other users are sent: a JPEG, PNG, GIF, WebP or BMP image of at most 1 MiB,
/// kept as a JPEG at most 512 pixels on its longest side.
#[utoipa::path(
  put, path = "/profile/picture", tag = "profile",
  request_body(
    description = "the image",
    content(
      (api_types::files::FileContent = "image/jpeg"), (api_types::files::FileContent = "image/png"),
      (api_types::files::FileContent = "image/gif"), (api_types::files::FileContent = "image/webp"),
      (api_types::files::FileContent = "image/bmp")
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
      "The picture must be a JPEG, PNG, GIF, WebP or BMP image",
    ));
  }
  // every refusal comes before the upload is read, so a refused one changes no picture
  target(&current, &admin.principal, PICTURE, false)?;
  let upload = match Limited::new(body, MAX_UPLOAD).collect().await {
    Ok(collected) => collected.to_bytes(),
    Err(e) if e.is::<LengthLimitError>() => {
      return Err(ApiError::too_large("The picture is larger than 1 MiB"));
    }
    Err(e) => {
      return Err(ApiError::invalid(
        "Could not read the request body",
        e.to_string(),
      ));
    }
  };
  let jpeg = tokio::task::spawn_blocking(move || profile::reencode(&upload))
    .await
    .map_err(ApiError::internal)?
    .map_err(|e| {
      debug!(error = %e, kind = declared, "profile picture refused");
      ApiError::invalid("Could not read the picture", e.to_string()).logged()
    })?;
  let _writing = app.profile_writes.lock().await;
  // checked again under the lock, against the file as the change before this one left it
  let target = target(&app.config.current(), &admin.principal, PICTURE, false)?;
  let path = profile::stored_path(&app.paths.data);
  let pointed = BTreeMap::from([(
    PICTURE.to_string(),
    Value::String(path.display().to_string()),
  )]);
  let staged = stage(&path, &jpeg).await.map_err(|e| {
    ApiError::internal(format!(
      "writing the picture next to {}: {e}",
      path.display()
    ))
  })?;
  let replaced = |e: std::io::Error| {
    ApiError::internal(format!("moving the picture to {}: {e}", path.display()))
  };
  // the picture is one value, which no list of another's change is lost under, so its writes
  // name no revision
  if tokio::fs::try_exists(&path).await.map_err(replaced)? {
    // the old picture stays in place until the option is written, which can still be refused
    match write(&app, &admin.principal, target, pointed, Vec::new(), None).await {
      Ok(_) => {}
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
    match write(&app, &admin.principal, target, pointed, Vec::new(), None).await {
      Ok(_) => {}
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
  let unset = vec![PICTURE.to_string()];
  write(&app, &admin.principal, target, BTreeMap::new(), unset, None).await?;
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
#[derive(PartialEq, Eq)]
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
/// command line sets, and for taking out (`unsetting`) a key a file that cannot be written
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
  // the file that sets a key is the one a save of it goes to, which cannot be written
  if let (true, Some(file)) = (unsetting, current.file_setting(key)) {
    warn!(
      key,
      file, reason, "profile value cannot be taken out of its file"
    );
    let detail = format!("{file} sets {key}, which cannot be changed from here: {reason}");
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

/// Sets the keys of `set` and takes out those of `unset`, in one write at `target`, refused when
/// the options changed since `read`, the revision the change was made against, when one is
/// given. A runtime override of one of them from before is removed after the write, so the
/// values written are the ones in effect. Gives the last step that changed a value.
async fn write(
  app: &AppState,
  by: &Principal,
  target: Target,
  set: BTreeMap<String, Value>,
  unset: Vec<String>,
  read: Option<&str>,
) -> Result<Option<Arc<Change<Options>>>, Failed> {
  let keys: Vec<String> = set.keys().chain(&unset).cloned().collect();
  let mut last = None;
  // the removal of the overrides after a file write follows our own change, so it checks nothing
  let mut removal_read = read;
  match target {
    Target::Override if !set.is_empty() => {
      let change = app
        .config
        .set_overrides(set, Vec::new(), &by.subject, Lasts::UntilRemoved, read)
        .await
        .map_err(|e| Failed::Unwritten(e.into()))?;
      debug!(keys = keys.join(", "), "profile set as a runtime override");
      return Ok(change);
    }
    // taking out a key no file sets only removes its override
    Target::Override => {}
    Target::File { name, revision } => {
      last = app
        .config
        .patch_file(&name, revision.as_deref(), read, set, unset)
        .await
        .map_err(|e| Failed::Unwritten(e.into()))?
        .change;
      removal_read = None;
      debug!(
        keys = keys.join(", "),
        file = name,
        "profile written to the configuration file"
      );
    }
  }
  let current = app.config.current();
  let overridden: Vec<String> = keys
    .into_iter()
    .filter(|key| current.overrides.contains_key(key))
    .collect();
  if !overridden.is_empty() {
    let removed = overridden.join(", ");
    let change = app
      .config
      .set_overrides(
        BTreeMap::new(),
        overridden,
        &by.subject,
        Lasts::UntilRemoved,
        removal_read,
      )
      .await
      .map_err(|e| Failed::Overridden(e.into()))?;
    last = change.or(last);
    debug!(keys = removed, "runtime override of the profile removed");
  }
  Ok(last)
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
