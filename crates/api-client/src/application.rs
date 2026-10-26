//! The application calls: the state document, the version, stopping and restarting, the
//! server connection, the database files, the log and diagnostics.

use api_types::application::{DatabaseFile, DisconnectReason, VersionLatestQuery};
use api_types::files::FilesystemFile;
use api_types::logs::LogRecord;
use api_types::state::{ServerState, State, VersionInfo};
use api_types::transfers::DebugTransfers;
use reqwest::StatusCode;

use super::{Client, Result, Route, done, json, status, text};

impl Client {
  /// `GET /application`: the application state.
  pub async fn application(&self) -> Result<State> {
    json(self.get(Route::new("application"))).await
  }

  /// `DELETE /application`: stops the daemon.
  pub async fn shut_down(&self) -> Result<()> {
    done(self.delete(Route::new("application"))).await
  }

  /// `PUT /application`: stops the daemon so it starts again.
  pub async fn restart(&self) -> Result<()> {
    done(self.put(Route::new("application"))).await
  }

  /// `GET /application/version`: the running version.
  pub async fn version(&self) -> Result<String> {
    json(self.get(Route::new("application/version"))).await
  }

  /// `GET /application/version/latest`: the version state, checked now with `force_check`.
  pub async fn latest_version(&self, force_check: bool) -> Result<VersionInfo> {
    let route =
      Route::new("application/version/latest").query(&VersionLatestQuery { force_check })?;
    json(self.get(route)).await
  }

  /// `GET /server`: the server connection.
  pub async fn server(&self) -> Result<ServerState> {
    json(self.get(Route::new("server"))).await
  }

  /// `PUT /server`: connects; 205 when the client was trying already.
  pub async fn connect(&self) -> Result<StatusCode> {
    status(self.put(Route::new("server"))).await
  }

  /// `DELETE /server`: disconnects and stops reconnecting, saying why.
  pub async fn disconnect(&self, reason: &str) -> Result<()> {
    let body = DisconnectReason(reason.to_string());
    done(self.delete(Route::new("server")).json(&body)).await
  }

  /// `GET /application/data`: the database files and their sizes.
  pub async fn databases(&self) -> Result<Vec<DatabaseFile>> {
    json(self.get(Route::new("application/data"))).await
  }

  /// `POST /application/data/{name}/vacuum`: rebuilds one database file without its free
  /// pages.
  pub async fn vacuum_database(&self, name: &str) -> Result<()> {
    let route = Route::new("application/data").segment(name).then("vacuum");
    done(self.post(route)).await
  }

  /// `GET /logs/live`: the latest log records.
  pub async fn live_logs(&self) -> Result<Vec<LogRecord>> {
    json(self.get(Route::new("logs/live"))).await
  }

  /// `GET /logs/files`: the log files.
  pub async fn log_files(&self) -> Result<Vec<FilesystemFile>> {
    json(self.get(Route::new("logs/files"))).await
  }

  /// `GET /logs/files/{filename}`: one log file's text.
  pub async fn log_file(&self, filename: &str) -> Result<String> {
    text(self.get(Route::new("logs/files").segment(filename))).await
  }

  /// `POST /application/gc`: asks for a garbage collection, which does nothing. A diagnostic
  /// route the API document leaves out.
  pub async fn collect_garbage(&self) -> Result<()> {
    done(self.post(Route::new("application/gc"))).await
  }

  /// `GET /debug/transfers`: the Soulseek client's own records of its transfers. A diagnostic
  /// route the API document leaves out.
  pub async fn debug_transfers(&self) -> Result<DebugTransfers> {
    json(self.get(Route::new("debug/transfers"))).await
  }
}
