//! Changing our Soulseek account's password against a scripted server: refused while logged
//! out, from an API key where the file needs someone at the UI, and when the command line sets
//! the password, each before the server is asked; otherwise the server is asked first and the
//! configuration file holds the new password once it agreed.

mod support;

use api_client::{Client, Credential};
use soulseek_proto::server::NewPassword;
use soulseek_proto::{ServerRequest, ServerResponse};
use support::{client, logged_in, scripted, scripted_login_with, signed_in, until};

/// An administrator's API key, which is no one at the UI.
const KEY: &str = "robot-key-0123456789";

/// The status a refused call answered with.
fn refused(result: api_client::Result<()>) -> Option<u16> {
  result.unwrap_err().status().map(|status| status.as_u16())
}

#[tokio::test]
async fn a_new_password_reaches_the_server_before_the_file() {
  let dir = tempfile::tempdir().unwrap();
  let config = dir.path().join("config.toml");
  let text = format!(
    "remote_configuration = true\n[soulseek]\npassword = \"{}\"\n\
     [web.authentication.api_keys.robot]\nkey = \"{KEY}\"\nrole = \"administrator\"\n",
    client::PASSWORD
  );
  std::fs::write(&config, text).unwrap();
  let (running, mut server) = scripted_login_with(dir.path(), &[]).await;
  let api = signed_in(&running).await;
  assert_eq!(refused(api.change_password("too-early").await), Some(503));
  server.accept_login().await;
  until(&api, "logged in", logged_in).await;
  let robot = Client::new(&running.url())
    .unwrap()
    .with_credential(Credential::ApiKey(KEY.to_string()));
  assert_eq!(
    refused(robot.change_password("from-a-key").await),
    Some(403)
  );

  let changing = tokio::spawn({
    let api = api.clone();
    async move { api.change_password("n3w-password").await }
  });
  let asked = server
    .request_matching(|request| match request {
      ServerRequest::NewPassword(asked) => Some(asked),
      _ => None,
    })
    .await;
  assert_eq!(
    asked.new_password.expose(),
    "n3w-password",
    "a refused change reached the server"
  );
  let before = std::fs::read_to_string(&config).unwrap();
  assert!(!before.contains("n3w-password"), "{before}");
  let echo = NewPassword {
    new_password: asked.new_password,
  };
  server.send(&ServerResponse::NewPassword(echo)).await;
  changing.await.unwrap().unwrap();
  let after = std::fs::read_to_string(&config).unwrap();
  assert!(after.contains("password = \"n3w-password\""), "{after}");
  drop(running);

  // the command line's password is the one the next login uses
  let (_running, api, _server) = scripted(dir.path()).await;
  assert_eq!(
    refused(api.change_password("from-the-page").await),
    Some(409)
  );
}
