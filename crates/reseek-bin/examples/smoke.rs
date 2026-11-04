//! The daemon's smoke run against a daemon that is already running, logged in to a test server,
//! with the soulseek-rs CLI as the other user: the NixOS VM test runs it beside the service. It
//! shares a file, searches, browses, downloads and uploads through the API, logs each step and
//! exits 0, or panics naming the step that failed.
//!
//! ```text
//! smoke --url http://127.0.0.1:5030 --login admin:secret --ours reseek --share /srv/share \
//!   --server 127.0.0.1:2242 --peer /path/to/soulseek-rs --peer-dir /tmp/peer
//! ```

#[path = "../tests/support/smoke.rs"]
#[allow(
  dead_code,
  reason = "the program runs the test server's half of the smoke run only"
)]
mod smoke;
#[path = "../../soulseek-client/tests/support/soulseek_rs.rs"]
mod soulseek_rs;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use soulseek_rs::Peer;

/// The flags, each taking one value.
const FLAGS: [&str; 7] = [
  "--url",
  "--login",
  "--ours",
  "--share",
  "--server",
  "--peer",
  "--peer-dir",
];

/// The other user's account on the test server.
const PEER_USERNAME: &str = "smokepeer";
const PEER_PASSWORD: &str = "smokepeer";
/// The port the other user takes connections on.
const PEER_PORT: u16 = 50310;

fn parse() -> Result<HashMap<String, String>, String> {
  let mut values = HashMap::new();
  let mut args = std::env::args().skip(1);
  while let Some(flag) = args.next() {
    if !FLAGS.contains(&flag.as_str()) {
      return Err(format!(
        "unknown argument {flag}; the flags are {}",
        FLAGS.join(" ")
      ));
    }
    let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
    values.insert(flag, value);
  }
  match FLAGS.iter().find(|flag| !values.contains_key(**flag)) {
    Some(missing) => Err(format!("{missing} is missing")),
    None => Ok(values),
  }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let values = match parse() {
    Ok(values) => values,
    Err(e) => {
      eprintln!("smoke: {e}");
      return ExitCode::from(2);
    }
  };
  tracing_subscriber::fmt().with_env_filter("info").init();
  let (username, password) = values["--login"]
    .split_once(':')
    .expect("--login is username:password");
  let api = api_client::Client::new(&values["--url"]).expect("--url is a base URL");
  api
    .login(username, password)
    .await
    .expect("the API login works");
  let peer = Peer {
    binary: PathBuf::from(&values["--peer"]),
    server: values["--server"].parse().expect("--server is ip:port"),
    username: PEER_USERNAME.to_string(),
    password: PEER_PASSWORD.to_string(),
    listen_port: PEER_PORT,
    root: PathBuf::from(&values["--peer-dir"]),
  };
  let share = PathBuf::from(&values["--share"]);
  smoke::local(&api, &values["--ours"], &share, &peer).await;
  eprintln!("smoke passed");
  ExitCode::SUCCESS
}
