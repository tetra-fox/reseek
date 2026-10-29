//! Our own account: presence, share counts, upload speed, password and privileges.

use std::time::Duration;

use soulseek_proto::server::{
  AcknowledgeNotifyPrivileges, GivePrivileges, NewPassword, SendUploadSpeed, SetOnlineStatus,
  SharedFoldersAndFiles,
};
use soulseek_proto::{Secret, ServerCode, ServerRequest, UserPresence};

use super::Client;
use super::request::{failed, non_blank};
use crate::error::ClientError;
use crate::waiter::WaitKey;

impl Client {
  /// tells the server whether we are away or online
  pub async fn set_status(&self, presence: UserPresence) -> Result<(), ClientError> {
    self
      .tell(
        "set status",
        ServerRequest::SetOnlineStatus(SetOnlineStatus { status: presence }),
      )
      .await
  }

  /// tells the server how many folders and files we share
  pub async fn set_shared_counts(&self, directories: u32, files: u32) -> Result<(), ClientError> {
    self
      .tell(
        "set shared counts",
        ServerRequest::SharedFoldersAndFiles(SharedFoldersAndFiles {
          directory_count: directories,
          file_count: files,
        }),
      )
      .await
  }

  /// reports the average speed, in bytes/second, of an upload we just finished
  pub async fn send_upload_speed(&self, speed: u32) -> Result<(), ClientError> {
    if speed == 0 {
      return Err(ClientError::InvalidArgument(
        "The upload speed must be greater than zero".into(),
      ));
    }
    self
      .tell(
        "send upload speed",
        ServerRequest::SendUploadSpeed(SendUploadSpeed { speed }),
      )
      .await
  }

  /// Changes our password. The server echoes the new password once it took it; an echo that
  /// differs means the change probably failed.
  pub async fn change_password(&self, password: &str) -> Result<(), ClientError> {
    non_blank("password", password)?;
    let echo: Secret = self
      .ask(
        "change password",
        WaitKey::Server(ServerCode::NewPassword),
        ServerRequest::NewPassword(NewPassword {
          new_password: Secret::new(password),
        }),
      )
      .await?;
    if echo.expose() != password {
      return Err(ClientError::client(
        "Probably failed to change password; the response from the server doesn't match the specified password",
        None,
      ));
    }
    Ok(())
  }

  /// how much privilege time our account has left
  pub async fn get_privileges(&self) -> Result<Duration, ClientError> {
    let seconds: u32 = self
      .ask(
        "get privileges",
        WaitKey::Server(ServerCode::CheckPrivileges),
        ServerRequest::CheckPrivileges,
      )
      .await?;
    Ok(Duration::from_secs(seconds.into()))
  }

  /// Gives `days` of our privileges to `username`. The server does not answer; a failure comes
  /// later as a private message from `server`.
  pub async fn grant_user_privileges(&self, username: &str, days: u32) -> Result<(), ClientError> {
    non_blank("username", username)?;
    if days == 0 {
      return Err(ClientError::InvalidArgument(
        "The number of days granted must be greater than zero".into(),
      ));
    }
    self
      .tell(
        &format!("grant privileges to user {username}"),
        ServerRequest::GivePrivileges(GivePrivileges {
          username: username.to_owned(),
          days,
        }),
      )
      .await
  }

  /// Acknowledges the privilege notification `id`. Allowed while logging in too, like private
  /// message acknowledgments.
  pub async fn acknowledge_privilege_notification(&self, id: u32) -> Result<(), ClientError> {
    let verb = format!("acknowledge privilege notification with ID {id}");
    self.require_logging_in_or_logged_in(&verb)?;
    self
      .write_server_message(&ServerRequest::AcknowledgeNotifyPrivileges(
        AcknowledgeNotifyPrivileges { id },
      ))
      .await
      .map_err(|error| failed(&verb, error))
  }
}
