//! How the server dispatcher treats messages beyond their own rows: the ping and password echoes,
//! codes nothing handles, malformed frames, and unread bytes.

mod support;

use soulseek_client::{ClientOptions, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{Secret, ServerCode, ServerResponse};
use support::{Logs, Session, options};
#[tokio::test]
async fn ping_completes_its_wait_and_an_unsolicited_one_is_dropped() {
  let mut session = Session::start().await;
  session.server.send(&ServerResponse::Ping).await;
  session.settle().await;
  let () = session
    .reply(WaitKey::Server(ServerCode::Ping), ServerResponse::Ping)
    .await;
}

#[tokio::test]
async fn new_password_completes_with_the_echo() {
  let mut session = Session::start().await;
  let got: Secret = session
    .reply(
      WaitKey::Server(ServerCode::NewPassword),
      ServerResponse::NewPassword(NewPassword {
        new_password: Secret::new("n3w"),
      }),
    )
    .await;
  assert_eq!(got.expose(), "n3w");
}

#[tokio::test]
async fn unhandled_and_obsolete_codes_are_logged_and_the_connection_stays_up() {
  let logs = Logs::capture();
  let mut session = Session::start().await;
  // a code the server echoes that nothing handles, a code nobody knows, an obsolete code with a
  // decoder and one without
  session
    .server
    .send_bytes(&[8, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0])
    .await;
  session
    .server
    .send_bytes(&[4, 0, 0, 0, 0x0f, 0x27, 0, 0])
    .await;
  session
    .server
    .send(&ServerResponse::IgnoreUser(IgnoreUser {
      username: "bob".into(),
    }))
    .await;
  session.server.send_bytes(&[4, 0, 0, 0, 58, 0, 0, 0]).await;
  session.settle().await;
  assert_eq!(
    logs
      .lines_with(&["unhandled server message", "code=2 "])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&["unhandled server message", "code=9999"])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&["obsolete server message", "code=11", "IgnoreUser"])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&["obsolete server message", "code=58", "AdminCommand"])
      .len(),
    1
  );
  assert_eq!(session.client.state(), soulseek_client::ClientState::READY);
}

#[tokio::test]
async fn a_malformed_message_is_dropped_and_the_connection_stays_up() {
  let mut session = Session::start().await;
  // GetStatus with a presence of 9
  let mut payload = 7u32.to_le_bytes().to_vec();
  payload.extend(3u32.to_le_bytes());
  payload.extend(b"bob");
  payload.extend(9u32.to_le_bytes());
  payload.push(0);
  let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
  frame.extend(payload);
  session.server.send_bytes(&frame).await;
  session.settle().await;
  assert_eq!(session.client.state(), soulseek_client::ClientState::READY);
}

#[tokio::test]
async fn unread_bytes_are_reported_when_asked_for() {
  let logs = Logs::capture();
  let mut session = Session::with_options(ClientOptions {
    report_unread_message_data: true,
    ..options()
  })
  .await;
  let mut frame = ServerResponse::GlobalAdminMessage(GlobalAdminMessage {
    message: "with a tail".into(),
  })
  .encode();
  frame.extend([1, 2, 3]);
  let len = (frame.len() - 4) as u32;
  frame[..4].copy_from_slice(&len.to_le_bytes());
  session.server.send_bytes(&frame).await;
  session.settle().await;
  assert_eq!(
    logs
      .lines_with(&["bytes past its fields", "code=66", "unread=3"])
      .len(),
    1
  );
}
