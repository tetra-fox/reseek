//! The agent against a controller the test plays, over an in-memory socket.

use api_types::relay::{RELAY_PROTOCOL, RelayMessage};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;

use super::dial::{Io, Settings, Socket};
use crate::testing::app;
use reseek_relay_proto::frames;

const CONFIG: &str = "[relay]\nenabled = true\nmode = \"agent\"\n\
  [relay.controller]\naddress = \"http://127.0.0.1:9\"\n\
  api_key = \"agent-key-0123456789\"\nsecret = \"agent-secret-0123456789\"\n";

#[tokio::test]
async fn an_agent_stopped_while_dialing_does_not_log_in() {
  let app = app(CONFIG, &[]).await;
  let agent = app.state.relay.agent.clone().unwrap();
  // the daemon starts the agent once its startup reaches it
  agent
    .started
    .subscribe()
    .wait_for(|started| *started)
    .await
    .unwrap();
  let settings = Settings::new(&app.state.config.current().values).unwrap();
  let (ours, theirs) = tokio::io::duplex(64 * 1024);
  let ours: Box<dyn Io> = Box::new(ours);
  let socket: Socket = WebSocketStream::from_raw_socket(ours, Role::Client, None).await;
  let mut controller = WebSocketStream::from_raw_socket(theirs, Role::Server, None).await;
  let challenge = RelayMessage::Challenge {
    token: "abc".into(),
    expires_in: 10_000,
    protocol: RELAY_PROTOCOL,
  };
  controller
    .send(Message::Text(frames::encode(None, challenge).into()))
    .await
    .unwrap();
  // stopped after the dial began and before the socket it opened is served
  agent.stop();
  let serving = tokio::spawn({
    let agent = agent.clone();
    async move { agent.serve(socket, std::sync::Arc::new(settings)).await }
  });
  let first = controller.next().await;
  assert!(
    matches!(first, Some(Ok(Message::Close(_)))),
    "the socket is closed without a login: {first:?}"
  );
  serving.await.unwrap();
}
