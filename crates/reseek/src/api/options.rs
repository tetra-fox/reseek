//! The options routes: the options in effect and at startup, the settings page's view, runtime
//! overrides, and reading, checking and writing the configuration files.
//!
//! Every route is for administrators. Changing anything, and reading the files' raw text,
//! needs `remote_configuration`; touching the files, or the authentication options, also needs
//! someone at the UI rather than a stored API key.

use std::collections::BTreeMap;

use super::extract::Json;
use api_types::options::{
  ConfigFile, ConfigView, FilePatch, FileQuery, FileReplace, FileValidate, FileWritten,
  OptionError, OptionsChanged, OptionsDocument, OptionsPatch, Validation,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cascade::{Lasts, Loaded};
use chrono::Utc;
use serde_json::Value;
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use crate::app::AppState;
use crate::auth::sessions::lifetime;
use crate::auth::{Administrator, Authorized, require_interactive};
use crate::config::{Options, redact, view};
use crate::db::shifted;

/// The keys an API key or token may not change.
const AUTHENTICATION: &str = "web.authentication";

/// The `expiresIn` of overrides that end when the daemon stops.
const UNTIL_RESTART: &str = "restart";

/// The options routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(get_options, patch_options))
    .routes(routes!(get_startup))
    .routes(routes!(get_view))
    .routes(routes!(get_debug))
    .routes(routes!(delete_override))
    .routes(routes!(get_file, patch_file, put_file))
    .routes(routes!(get_file_location))
    .routes(routes!(validate_file))
}

/// The options in effect.
#[utoipa::path(
  get, path = "/options", tag = "options",
  responses((status = 200, body = OptionsDocument), (status = 401), (status = 403))
)]
async fn get_options(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
) -> Json<OptionsDocument> {
  Json(OptionsDocument(redact::values(&app.config.current())))
}

/// The options the daemon started with, which every restart-class option still runs with.
#[utoipa::path(
  get, path = "/options/startup", tag = "options",
  responses((status = 200, body = OptionsDocument), (status = 401), (status = 403))
)]
async fn get_startup(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
) -> Json<OptionsDocument> {
  Json(OptionsDocument(redact::values(&app.startup)))
}

/// Every option with where its value came from, the files, the overrides and what is pending.
#[utoipa::path(
  get, path = "/options/view", tag = "options",
  responses((status = 200, body = ConfigView), (status = 401), (status = 403))
)]
async fn get_view(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
) -> Json<ConfigView> {
  Json(view::config_view(&app.config.current()))
}

/// Every option's value with where it came from, one `key=value (source)` line each, secrets
/// hidden. Needs `debug` and `remote_configuration` on, and someone at the UI.
#[utoipa::path(
  get, path = "/options/debug", tag = "options",
  responses(
    (status = 200, body = String, content_type = "application/json"),
    (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn get_debug(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
) -> Result<Json<String>, ApiError> {
  let current = app.config.current();
  if !current.values.debug {
    warn!("Blocked configuration debug view; debug mode is off");
    return Err(ApiError::forbidden("The debug view requires debug mode").logged());
  }
  remote(
    &current,
    "Blocked configuration debug view; remote configuration is disabled",
  )?;
  require_interactive(&admin.principal)?;
  Ok(Json(view::debug_view(&current)))
}

/// Sets and removes runtime overrides. They last across restarts until they expire, are
/// removed, or the files change the value under them; with `expiresIn` `restart` they are kept
/// in memory only and end when the daemon stops.
#[utoipa::path(
  patch, path = "/options", tag = "options", request_body = OptionsPatch,
  responses(
    (status = 200, body = OptionsChanged), (status = 204, description = "nothing to change"),
    (status = 400, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn patch_options(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Json(patch): Json<OptionsPatch>,
) -> Result<Response, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked options overlay application; remote configuration is disabled",
  )?;
  if patch.set.is_empty() && patch.unset.is_empty() {
    return Ok(StatusCode::NO_CONTENT.into_response());
  }
  let keys: Vec<String> = patch.set.keys().chain(&patch.unset).cloned().collect();
  if keys.iter().any(|k| is_authentication(k)) {
    require_interactive(&admin.principal)?;
  }
  let lasts = match patch.expires_in.as_deref() {
    Some(UNTIL_RESTART) => Lasts::UntilRestart,
    Some(text) => {
      let duration = humantime::parse_duration(text).map_err(|e| {
        ApiError::invalid("The value of expiresIn is not a duration", e.to_string())
      })?;
      Lasts::Until(shifted(Utc::now(), lifetime(duration)))
    }
    None => Lasts::UntilRemoved,
  };
  let set = with_urls(&current, patch.set)?;
  let change = app
    .config
    .set_overrides(set, patch.unset, &admin.principal.subject, lasts)
    .await
    .map_err(|e| match e {
      cascade::Error::Invalid(_) | cascade::Error::Value { .. } => {
        warn!(error = %e, "Options patch validation failed");
        ApiError::from(e).logged()
      }
      other => ApiError::from(other),
    })?;
  info!(
    by = admin.principal.subject,
    keys = keys.join(", "),
    "runtime overrides changed"
  );
  Ok(Json(view::changed(change.as_deref())).into_response())
}

/// Removes the runtime override of one key.
#[utoipa::path(
  delete, path = "/options/overrides/{key}", tag = "options",
  params(("key" = String, Path, description = "the configuration key")),
  responses(
    (status = 200, body = OptionsChanged), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn delete_override(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Path(key): Path<String>,
) -> Result<Json<OptionsChanged>, ApiError> {
  remote(
    &app.config.current(),
    "Blocked runtime override removal; remote configuration is disabled",
  )?;
  if is_authentication(&key) {
    require_interactive(&admin.principal)?;
  }
  let change = app.config.remove_override(&key).await?;
  info!(
    by = admin.principal.subject,
    key, "runtime override removed"
  );
  Ok(Json(view::changed(change.as_deref())))
}

/// A configuration file's text, secrets and all.
#[utoipa::path(
  get, path = "/options/file", tag = "options", params(FileQuery),
  responses(
    (status = 200, body = ConfigFile), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn get_file(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Query(query): Query<FileQuery>,
) -> Result<Json<ConfigFile>, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked configuration file read; remote configuration is disabled",
  )?;
  require_interactive(&admin.principal)?;
  let file = file(&current, query.file.as_deref())?;
  let text = match tokio::fs::read_to_string(&file.path).await {
    Ok(text) => Some(text),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
    Err(e) => {
      return Err(ApiError::internal(format!(
        "reading {}: {e}",
        file.path.display()
      )));
    }
  };
  Ok(Json(ConfigFile {
    name: file.name.clone(),
    path: file.path.display().to_string(),
    // the text's revision as the configuration last read it, which a write is checked against
    revision: file.revision.clone(),
    text,
  }))
}

/// Where a configuration file is.
#[utoipa::path(
  get, path = "/options/file/location", tag = "options", params(FileQuery),
  responses(
    (status = 200, body = String, content_type = "application/json"),
    (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn get_file_location(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Query(query): Query<FileQuery>,
) -> Result<Json<String>, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked configuration file read; remote configuration is disabled",
  )?;
  require_interactive(&admin.principal)?;
  Ok(Json(
    file(&current, query.file.as_deref())?
      .path
      .display()
      .to_string(),
  ))
}

/// Changes some keys of a configuration file, keeping its comments, then reloads.
#[utoipa::path(
  patch, path = "/options/file", tag = "options", request_body = FilePatch,
  responses(
    (status = 200, body = FileWritten), (status = 400, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn patch_file(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Json(patch): Json<FilePatch>,
) -> Result<Json<FileWritten>, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked configuration file write; remote configuration is disabled",
  )?;
  require_interactive(&admin.principal)?;
  let name = file(&current, patch.file.as_deref())?.name.clone();
  let set = with_urls(&current, patch.set)?;
  let written = app
    .config
    .patch_file(&name, patch.revision.as_deref(), set, patch.unset)
    .await?;
  info!(
    by = admin.principal.subject,
    file = name,
    "configuration file changed"
  );
  Ok(Json(FileWritten {
    revision: written.revision,
    changed: view::changed(written.change.as_deref()),
  }))
}

/// Replaces the whole text of a configuration file, then reloads.
#[utoipa::path(
  put, path = "/options/file", tag = "options", request_body = FileReplace,
  responses(
    (status = 200, body = FileWritten), (status = 400, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn put_file(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Json(replace): Json<FileReplace>,
) -> Result<Json<FileWritten>, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked configuration file write; remote configuration is disabled",
  )?;
  require_interactive(&admin.principal)?;
  let name = file(&current, replace.file.as_deref())?.name.clone();
  let written = app
    .config
    .replace_file(&name, replace.revision.as_deref(), replace.text)
    .await?;
  info!(
    by = admin.principal.subject,
    file = name,
    "configuration file replaced"
  );
  Ok(Json(FileWritten {
    revision: written.revision,
    changed: view::changed(written.change.as_deref()),
  }))
}

/// Whether a text would load as a configuration file, checked with every other layer.
#[utoipa::path(
  post, path = "/options/file/validate", tag = "options", request_body = FileValidate,
  responses(
    (status = 200, body = Validation), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn validate_file(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
  Json(request): Json<FileValidate>,
) -> Result<Json<Validation>, ApiError> {
  let current = app.config.current();
  remote(
    &current,
    "Blocked configuration validation; remote configuration is disabled",
  )?;
  let name = file(&current, request.file.as_deref())?.name.clone();
  let checked = app.config.check_file(&name, &request.text).await?;
  let errors = checked
    .error
    .as_ref()
    .map(option_errors)
    .unwrap_or_default();
  Ok(Json(Validation {
    valid: checked.error.is_none(),
    errors,
    warnings: checked.warnings,
  }))
}

/// Refuses the call unless `remote_configuration` is on, logging `blocked` when it is not.
pub(super) fn remote(current: &Loaded<Options>, blocked: &str) -> Result<(), ApiError> {
  if current.values.remote_configuration {
    return Ok(());
  }
  warn!("{blocked}");
  Err(ApiError::forbidden("Remote configuration is disabled").logged())
}

/// `set` with the credentials of the URLs it shows hidden put back from the values in effect.
fn with_urls(
  current: &Loaded<Options>,
  mut set: BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, ApiError> {
  redact::restore_urls(&mut set, current.tree()).map_err(|invalid| {
    let e = cascade::Error::Invalid(vec![invalid]);
    warn!(error = %e, "hidden URL credentials not restored");
    ApiError::from(e).logged()
  })?;
  Ok(set)
}

fn is_authentication(key: &str) -> bool {
  key == AUTHENTICATION || key.starts_with(&format!("{AUTHENTICATION}."))
}

/// The configuration file named `name`, the first one when not given.
fn file<'a>(
  current: &'a Loaded<Options>,
  name: Option<&str>,
) -> Result<&'a cascade::LoadedFile, ApiError> {
  match name {
    Some(name) => current
      .files
      .iter()
      .find(|f| f.name == name)
      .ok_or_else(|| ApiError::not_found(format!("{name} is not a configuration file"))),
    None => current
      .files
      .first()
      .ok_or_else(|| ApiError::internal("the configuration has no files")),
  }
}

/// What is wrong with a text, one entry per key where the error says which.
fn option_errors(error: &cascade::Error) -> Vec<OptionError> {
  match error {
    cascade::Error::Invalid(invalid) => invalid
      .iter()
      .map(|i| OptionError {
        key: Some(i.key.clone()),
        message: i.message.clone(),
      })
      .collect(),
    cascade::Error::Value { key, message, .. } => vec![OptionError {
      key: Some(key.clone()),
      message: message.clone(),
    }],
    other => vec![OptionError {
      key: None,
      message: other.to_string(),
    }],
  }
}

#[cfg(test)]
mod tests {
  use crate::testing::{app, request};
  use serde_json::json;

  /// Loopback is trusted, so requests from it are an administrator at the UI.
  const TRUSTED: &str = "remote_configuration = true\n[web.authentication]\ntrusted_networks = \
                         [\"127.0.0.0/8\"]\npassword = \"hunter2\"\n[web.authentication.api_keys.\
                         robot]\nkey = \"robot-key-0123456789\"\nrole = \"administrator\"\n";

  #[tokio::test]
  async fn the_view_names_sources_and_hides_secrets() {
    let app = app(
      &format!("{TRUSTED}[logger]\nloki = \"https://a:b@loki.lan/\"\n"),
      &[("RESEEK_HTTP_PORT", "6000")],
    )
    .await;
    let answer = app.send(request("GET", "/api/v0/options/view", None)).await;
    assert_eq!(answer.status, 200);
    let view = answer.json();
    assert_eq!(view["values"]["web"]["authentication"]["password"], "*****");
    assert_eq!(view["values"]["web"]["port"], 6000);
    assert_eq!(view["values"]["logger"]["loki"], "https://*****@loki.lan/");
    assert_eq!(
      view["fields"]["web.port"]["source"],
      json!({ "layer": "env", "var": "RESEEK_HTTP_PORT" })
    );
    assert_eq!(
      view["fields"]["remote_configuration"]["source"],
      json!({ "layer": "file", "file": "config.toml" })
    );
    assert_eq!(view["fields"]["web.port"]["apply"], "restart");
    let level = &view["fields"]["logging.level"];
    assert_eq!(
      level["choices"],
      json!(["error", "warn", "info", "debug", "trace"])
    );
    assert_eq!(level["default"], "info");
    let mode = &view["fields"]["relay.mode"];
    assert_eq!(mode["choices"], json!(["controller"]));
    let retry = &view["fields"]["transfers.download.retry.delay"];
    assert_eq!(retry["choices"], json!(null), "a duration is no choice");
    assert_eq!(retry["default"], "5s");
    assert_eq!(
      view["fields"]["web.authentication.password"]["default"], "*****",
      "a secret's default is hidden like its value"
    );
    assert_eq!(view["files"][0]["name"], "config.toml");
    assert_eq!(view["files"][0]["writable"], true);
    assert_eq!(view["loadError"], json!(null));
  }

  #[tokio::test]
  async fn overrides_take_live_keys_and_refuse_restart_and_env_keys() {
    let app = app(TRUSTED, &[("RESEEK_SLSK_DESCRIPTION", "from env")]).await;
    let set = |body| request("PATCH", "/api/v0/options", Some(body));
    let answer = app
      .send(set(
        json!({ "set": { "rooms": ["indie"] }, "expiresIn": "2h" }),
      ))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    assert_eq!(answer.json()["applied"], json!(["rooms"]));
    let view = app
      .send(request("GET", "/api/v0/options/view", None))
      .await
      .json();
    assert_eq!(view["overrides"]["rooms"]["setBy"], "network:127.0.0.0/8");
    assert!(view["overrides"]["rooms"]["expiresAt"].is_string());

    let restart = app.send(set(json!({ "set": { "web.port": 1 } }))).await;
    assert_eq!(restart.status, 400);
    assert_eq!(
      restart.json()["error"],
      "Requires a restart and must be saved to the file"
    );
    let env = app
      .send(set(json!({ "set": { "soulseek.description": "x" } })))
      .await;
    assert_eq!(env.status, 409);
    assert_eq!(env.json()["error"], "Set by RESEEK_SLSK_DESCRIPTION");
    let empty = app.send(set(json!({}))).await;
    assert_eq!(empty.status, 204);

    let removed = app
      .send(request("DELETE", "/api/v0/options/overrides/rooms", None))
      .await;
    assert_eq!(removed.status, 200);
    let again = app
      .send(request("DELETE", "/api/v0/options/overrides/rooms", None))
      .await;
    assert_eq!(again.status, 404);
  }

  #[tokio::test]
  async fn filter_presets_are_one_live_list_replaced_whole() {
    let app = app(TRUSTED, &[]).await;
    let get = || request("GET", "/api/v0/options", None);
    let mine = json!([
      { "name": "FLAC", "filter": "type:flac" },
      { "name": "Big", "filter": "size:100m" }
    ]);
    let answer = app
      .send(request(
        "PATCH",
        "/api/v0/options",
        Some(json!({ "set": { "search.filter_presets": mine } })),
      ))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    assert_eq!(answer.json()["applied"], json!(["search.filter_presets"]));
    assert_eq!(
      app.send(get()).await.json()["search"]["filter_presets"],
      mine
    );
  }

  #[tokio::test]
  async fn an_override_until_restart_lasts_through_reloads_but_not_a_restart() {
    let app = app(TRUSTED, &[]).await;
    let set = |body| request("PATCH", "/api/v0/options", Some(body));
    let answer = app
      .send(set(
        json!({ "set": { "rooms": ["indie"] }, "expiresIn": "restart" }),
      ))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    let kept = app
      .send(set(json!({ "set": { "soulseek.description": "kept" } })))
      .await;
    assert_eq!(kept.status, 200);
    std::fs::write(
      app.dir.path().join("config.toml"),
      format!("{TRUSTED}[transfers.upload]\nslots = 3\n"),
    )
    .unwrap();
    app.state.config.reload().await.unwrap();
    let view = app
      .send(request("GET", "/api/v0/options/view", None))
      .await
      .json();
    assert_eq!(
      view["values"]["rooms"],
      json!(["indie"]),
      "a reload keeps it"
    );
    assert_eq!(view["overrides"]["rooms"]["untilRestart"], true);
    assert_eq!(view["overrides"]["rooms"]["expiresAt"], json!(null));
    assert_eq!(view["fields"]["rooms"]["source"]["untilRestart"], true);
    let stored = std::fs::read_to_string(app.dir.path().join("overrides.toml")).unwrap();
    assert!(!stored.contains("rooms"), "{stored}");
    assert!(stored.contains("soulseek.description"), "{stored}");
    // the next start reads only what the file holds
    let restarted = cascade::Overrides::load(app.dir.path().join("overrides.toml")).unwrap();
    assert_eq!(
      restarted.items().keys().collect::<Vec<_>>(),
      ["soulseek.description"]
    );
  }

  #[tokio::test]
  async fn changing_anything_needs_remote_configuration() {
    let app = app(crate::testing::TRUSTED, &[]).await;
    assert_eq!(
      app
        .send(request("GET", "/api/v0/options", None))
        .await
        .status,
      200
    );
    for (method, uri, body) in [
      (
        "PATCH",
        "/api/v0/options",
        Some(json!({ "set": { "rooms": [] } })),
      ),
      ("GET", "/api/v0/options/file", None),
      (
        "POST",
        "/api/v0/options/file/validate",
        Some(json!({ "text": "" })),
      ),
      (
        "PUT",
        "/api/v0/options/file",
        Some(json!({ "text": "", "revision": null })),
      ),
    ] {
      let answer = app.send(request(method, uri, body)).await;
      assert_eq!(answer.status, 403, "{method} {uri}");
      assert_eq!(answer.json()["error"], "Remote configuration is disabled");
    }
  }

  #[tokio::test]
  async fn file_writes_check_the_revision_validation_and_later_files() {
    let app = app(TRUSTED, &[]).await;
    std::fs::create_dir(app.dir.path().join("config.d")).unwrap();
    std::fs::write(
      app.dir.path().join("config.d/10-rooms.toml"),
      "rooms = [\"later\"]\n",
    )
    .unwrap();
    app.state.config.reload().await.unwrap();
    let file = app
      .send(request("GET", "/api/v0/options/file", None))
      .await
      .json();
    let revision = file["revision"].clone();
    assert!(
      file["text"]
        .as_str()
        .unwrap()
        .contains("remote_configuration")
    );
    let patch = |body| request("PATCH", "/api/v0/options/file", Some(body));

    let stale = app
      .send(patch(
        json!({ "set": { "debug": true }, "revision": "sha256:old" }),
      ))
      .await;
    assert_eq!(stale.status, 409);
    assert_eq!(
      stale.json()["error"],
      "The configuration file changed on disk"
    );
    let invalid = app
      .send(patch(
        json!({ "set": { "web.https.port": 0 }, "revision": revision }),
      ))
      .await;
    assert_eq!(invalid.status, 400);
    assert!(
      invalid.json()["detail"]
        .as_str()
        .unwrap()
        .contains("web.https.port")
    );
    let shadowed = app
      .send(patch(
        json!({ "set": { "rooms": ["mine"] }, "revision": revision }),
      ))
      .await;
    assert_eq!(shadowed.status, 409);
    assert_eq!(shadowed.json()["error"], "Set by config.d/10-rooms.toml");
    let written = app
      .send(patch(
        json!({ "set": { "debug": true }, "revision": revision }),
      ))
      .await;
    assert_eq!(written.status, 200);
    let written = written.json();
    assert_eq!(written["restartRequired"], json!(["debug"]));
    assert_ne!(written["revision"], revision);
    let text = std::fs::read_to_string(app.dir.path().join("config.toml")).unwrap();
    assert!(text.contains("debug = true") && text.contains("[web.authentication]"));
  }

  #[tokio::test]
  async fn a_url_sent_back_without_its_credentials_keeps_them() {
    let hook = "[integrations.scripts.hook]\non = [\"DownloadFileComplete\"]\n\
                run.executable = \"/bin/true\"\n\
                run.arglist = [\"https://u:p@hooks.lan/x\", \"plain\"]\n";
    let app = app(&format!("{TRUSTED}{hook}"), &[]).await;
    let view = app
      .send(request("GET", "/api/v0/options/view", None))
      .await
      .json();
    let mut scripts = view["values"]["integrations"]["scripts"].clone();
    assert_eq!(
      scripts["hook"]["run"]["arglist"],
      json!(["https://*****@hooks.lan/x", "plain"])
    );
    // a second entry made from the first, as a client copies a row
    scripts["copy"] = scripts["hook"].clone();
    let revision = view["files"][0]["revision"].clone();
    let patch = |scripts: &serde_json::Value| {
      request(
        "PATCH",
        "/api/v0/options/file",
        Some(json!({ "set": { "integrations.scripts": scripts }, "revision": revision })),
      )
    };
    let written = app.send(patch(&scripts)).await;
    assert_eq!(written.status, 200, "{:?}", written.body);
    let text = std::fs::read_to_string(app.dir.path().join("config.toml")).unwrap();
    assert_eq!(text.matches("https://u:p@hooks.lan/x").count(), 2, "{text}");
    assert!(!text.contains("*****"), "{text}");

    scripts["hook"]["run"]["arglist"][0] = json!("https://*****@elsewhere.lan/");
    let refused = app.send(patch(&scripts)).await;
    assert_eq!(refused.status, 400);
    let detail = refused.json()["detail"].as_str().unwrap().to_string();
    assert!(
      detail.contains("integrations.scripts.hook.run.arglist[0]"),
      "{detail}"
    );
    let override_refused = app
      .send(request(
        "PATCH",
        "/api/v0/options",
        Some(json!({ "set": { "integrations.scripts": scripts } })),
      ))
      .await;
    assert_eq!(override_refused.status, 400);
    assert_eq!(override_refused.json()["detail"], refused.json()["detail"]);
  }

  #[tokio::test]
  async fn the_debug_view_names_sources_hides_secrets_and_needs_debug() {
    let debugging = app(
      &format!("debug = true\n{TRUSTED}[logger]\nloki = \"https://a:b@c.lan/\"\n"),
      &[("RESEEK_HTTP_PORT", "6000")],
    )
    .await;
    let answer = debugging
      .send(request("GET", "/api/v0/options/debug", None))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    let text = answer.json();
    let text = text.as_str().unwrap();
    assert!(
      text.contains("web.port=6000 (environment RESEEK_HTTP_PORT)\n"),
      "{text}"
    );
    assert!(text.contains("debug=true (file config.toml)\n"), "{text}");
    assert!(text.contains("web.authentication.password=\"*****\" (file config.toml)\n"));
    assert!(text.contains("logger.loki=\"https://*****@c.lan/\""));
    assert!(
      !text.contains("hunter2") && !text.contains("robot-key"),
      "{text}"
    );

    let quiet = app(TRUSTED, &[]).await;
    let refused = quiet
      .send(request("GET", "/api/v0/options/debug", None))
      .await;
    assert_eq!(refused.status, 403);
  }

  #[tokio::test]
  async fn a_managed_file_is_read_only() {
    let app = app(TRUSTED, &[("RESEEK_CONFIG_MANAGED_BY", "nixos")]).await;
    let view = app
      .send(request("GET", "/api/v0/options/view", None))
      .await
      .json();
    assert_eq!(view["managedBy"], "nixos");
    assert_eq!(view["files"][0]["readOnlyReason"], "managed by nixos");
    let revision = view["files"][0]["revision"].clone();
    let answer = app
      .send(request(
        "PATCH",
        "/api/v0/options/file",
        Some(json!({ "set": { "debug": true }, "revision": revision })),
      ))
      .await;
    assert_eq!(answer.status, 409);
    assert_eq!(answer.json()["detail"], "managed by nixos");
  }

  #[tokio::test]
  async fn validate_reports_errors_by_key_and_unknown_keys() {
    let app = app(TRUSTED, &[]).await;
    let check = |text: &str| {
      request(
        "POST",
        "/api/v0/options/file/validate",
        Some(json!({ "text": text })),
      )
    };
    let answer = app
      .send(check("[web]\nprot = 1\n[web.https]\nport = 0\n"))
      .await
      .json();
    assert_eq!(answer["valid"], false);
    assert_eq!(answer["errors"][0]["key"], "web.https.port");
    assert_eq!(
      answer["warnings"],
      json!(["config.toml: unknown key `web.prot`"])
    );
    let fine = app.send(check("")).await.json();
    assert_eq!(fine, json!({ "valid": true, "errors": [], "warnings": [] }));
    let broken = app.send(check("[web\n")).await.json();
    assert_eq!(broken["errors"][0]["key"], json!(null));
  }

  #[tokio::test]
  async fn a_value_its_type_refuses_is_keyed_like_any_refusal() {
    let app = app(TRUSTED, &[]).await;
    let view = app
      .send(request("GET", "/api/v0/options/view", None))
      .await
      .json();
    let revision = view["files"][0]["revision"].clone();
    let refused = app
      .send(request(
        "PATCH",
        "/api/v0/options/file",
        Some(json!({ "set": { "analysis.timeout": "five" }, "revision": revision })),
      ))
      .await;
    assert_eq!(refused.status, 400);
    let detail = refused.json()["detail"].as_str().unwrap().to_string();
    assert!(detail.starts_with("analysis.timeout: "), "{detail}");
    assert!(!detail.contains("set by"), "{detail}");

    let checked = app
      .send(request(
        "POST",
        "/api/v0/options/file/validate",
        Some(json!({ "text": format!("{TRUSTED}[analysis]\ntimeout = \"five\"\n") })),
      ))
      .await
      .json();
    assert_eq!(checked["errors"][0]["key"], "analysis.timeout");
    let message = checked["errors"][0]["message"].as_str().unwrap();
    assert!(
      !message.contains("analysis.timeout") && !message.contains("set by"),
      "{message}"
    );
  }

  #[tokio::test]
  async fn an_api_key_may_read_but_not_touch_the_files() {
    let app = app(TRUSTED, &[]).await;
    let with_key = |method: &str, uri: &str, body| {
      let mut r = request(method, uri, body);
      r.headers_mut()
        .insert("x-api-key", "robot-key-0123456789".parse().unwrap());
      r
    };
    let options = app
      .send_from("192.0.2.1:1", with_key("GET", "/api/v0/options", None))
      .await;
    assert_eq!(options.status, 200);
    let file = app
      .send_from("192.0.2.1:1", with_key("GET", "/api/v0/options/file", None))
      .await;
    assert_eq!(file.status, 403);
    let auth = app
      .send_from(
        "192.0.2.1:1",
        with_key(
          "PATCH",
          "/api/v0/options",
          Some(json!({ "set": { "web.authentication.username": "x" } })),
        ),
      )
      .await;
    assert_eq!(auth.status, 403, "authentication options need a login");
  }
}
