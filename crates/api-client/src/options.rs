//! The options calls: the options in effect and at startup, the settings view, runtime
//! overrides, and the configuration files.

use api_types::options::{
  ConfigFile, ConfigView, FilePatch, FileQuery, FileReplace, FileValidate, FileWritten,
  OptionsChanged, OptionsDocument, OptionsPatch, Validation,
};
use reqwest::StatusCode;

use super::{Client, Result, Route, json, send};

impl Client {
  /// `GET /options`: the options in effect.
  pub async fn options(&self) -> Result<OptionsDocument> {
    json(self.get(Route::new("options"))).await
  }

  /// `GET /options/startup`: the options the daemon started with.
  pub async fn startup_options(&self) -> Result<OptionsDocument> {
    json(self.get(Route::new("options/startup"))).await
  }

  /// `GET /options/view`: every option with where it came from.
  pub async fn options_view(&self) -> Result<ConfigView> {
    json(self.get(Route::new("options/view"))).await
  }

  /// `GET /options/debug`: one `key=value (source)` line per option.
  pub async fn options_debug(&self) -> Result<String> {
    json(self.get(Route::new("options/debug"))).await
  }

  /// `PATCH /options`: sets and removes runtime overrides; `None` when there was nothing to
  /// change.
  pub async fn patch_options(&self, patch: &OptionsPatch) -> Result<Option<OptionsChanged>> {
    let response = send(self.patch(Route::new("options")).json(patch)).await?;
    if response.status() == StatusCode::NO_CONTENT {
      return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&response.bytes().await?)?))
  }

  /// `DELETE /options/overrides/{key}`: removes the runtime override of `key`.
  pub async fn remove_override(&self, key: &str) -> Result<OptionsChanged> {
    json(self.delete(Route::new("options/overrides").segment(key))).await
  }

  /// `GET /options/file`: a configuration file's text.
  pub async fn config_file(&self, query: &FileQuery) -> Result<ConfigFile> {
    json(self.get(Route::new("options/file").query(query)?)).await
  }

  /// `GET /options/file/location`: where a configuration file is.
  pub async fn config_file_location(&self, query: &FileQuery) -> Result<String> {
    json(self.get(Route::new("options/file/location").query(query)?)).await
  }

  /// `PATCH /options/file`: changes some keys of a configuration file.
  pub async fn patch_config_file(&self, patch: &FilePatch) -> Result<FileWritten> {
    json(self.patch(Route::new("options/file")).json(patch)).await
  }

  /// `PUT /options/file`: replaces a configuration file's text.
  pub async fn replace_config_file(&self, replace: &FileReplace) -> Result<FileWritten> {
    json(self.put(Route::new("options/file")).json(replace)).await
  }

  /// `POST /options/file/validate`: whether a text would load as a configuration file.
  pub async fn validate_config_file(&self, request: &FileValidate) -> Result<Validation> {
    json(self.post(Route::new("options/file/validate")).json(request)).await
  }
}
