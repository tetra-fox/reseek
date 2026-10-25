//! A smoke test of the client against a running Soulseek server, the real one by default. It logs
//! in, joins a room, runs one network search, browses a user who answered, downloads one small
//! file that user offered into a temporary directory, checks the file has the size the uploader
//! announced, disconnects and exits 0. A step that fails ends the run with exit status 1 and one
//! line on stderr naming the step and the reason.
//!
//! Log in with a throwaway account made for this, never a personal one: the server allows one
//! session per account, so this login kicks any other session of the same account.
//!
//! ```text
//! RESEEK_TEST_USERNAME=... RESEEK_TEST_PASSWORD=... \
//!   cargo run -p soulseek-client --example live_smoke -- --help
//! ```
//!
//! `--help` lists the environment variables and the options.

use std::future::Future;
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use soulseek_client::{
  BrowseOptions, Client, ClientError, ClientOptions, DEFAULT_SERVER_ADDRESS, DEFAULT_SERVER_PORT,
  DownloadOutput, DownloadRequest, SearchOptions, SearchScope,
};
use soulseek_proto::peer::SearchResponse;
use soulseek_proto::{RemoteFilename, RoomType};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_ROOM: &str = "reseek";
const DEFAULT_QUERY: &str = "folder jpg";
const DEFAULT_MAX_SIZE: u64 = 1 << 20;
const DEFAULT_LISTEN_PORT: u16 = 50_000;

/// how long the search waits after its last response before it ends
const SEARCH_IDLE: Duration = Duration::from_secs(10);
/// how long the search may run at all; a search still taking responses then ends with them
const SEARCH_WINDOW: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);
const BROWSE_TIMEOUT: Duration = Duration::from_secs(120);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What one run does, from the flags, the environment and the defaults
struct Config {
  host: String,
  port: u16,
  username: String,
  password: String,
  room: String,
  query: String,
  max_size: u64,
  listen_port: u16,
}

enum Command {
  Help,
  Run(Config),
}

/// A step that failed, and why
struct Failure {
  step: &'static str,
  reason: String,
}

impl Failure {
  fn new(step: &'static str, reason: impl Into<String>) -> Self {
    Self {
      step,
      reason: reason.into(),
    }
  }
}

/// The file the run downloads, as its search response offered it
struct Pick {
  username: String,
  filename: RemoteFilename,
  size: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let config = match parse(std::env::args().skip(1).collect()) {
    Ok(Command::Help) => {
      print!("{}", help());
      return ExitCode::SUCCESS;
    }
    Ok(Command::Run(config)) => config,
    Err(error) => {
      eprintln!("live smoke: {error} (see --help)");
      return ExitCode::from(2);
    }
  };
  init_logging();
  match run(&config).await {
    Ok(()) => {
      eprintln!("live smoke passed");
      ExitCode::SUCCESS
    }
    Err(failure) => {
      eprintln!("live smoke failed at {}: {}", failure.step, failure.reason);
      ExitCode::FAILURE
    }
  }
}

fn parse(args: Vec<String>) -> Result<Command, String> {
  let mut flags = Vec::new();
  let mut args = args.into_iter();
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "-h" | "--help" => return Ok(Command::Help),
      "--room" | "--query" | "--max-size" | "--listen-port" => {
        let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
        flags.push((arg, value));
      }
      _ => return Err(format!("unknown argument {arg}")),
    }
  }
  let variable = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
  // a later flag overrides an earlier one, and any flag overrides its variable
  let setting = |flag: &str, name: &str| {
    flags
      .iter()
      .rev()
      .find(|(given, _)| given == flag)
      .map(|(_, value)| value.clone())
      .or_else(|| variable(name))
  };
  let (Some(username), Some(password)) = (
    variable("RESEEK_TEST_USERNAME"),
    variable("RESEEK_TEST_PASSWORD"),
  ) else {
    return Err(
      "RESEEK_TEST_USERNAME and RESEEK_TEST_PASSWORD must name a throwaway account".into(),
    );
  };
  let (host, port) = match variable("RESEEK_TEST_SERVER") {
    Some(server) => parse_server(&server)?,
    None => (DEFAULT_SERVER_ADDRESS.to_owned(), DEFAULT_SERVER_PORT),
  };
  let max_size = match setting("--max-size", "RESEEK_TEST_MAX_SIZE") {
    Some(text) => text
      .parse()
      .map_err(|_| format!("the size ceiling {text:?} is not a number of bytes"))?,
    None => DEFAULT_MAX_SIZE,
  };
  let listen_port = match setting("--listen-port", "RESEEK_TEST_LISTEN_PORT") {
    Some(text) => text
      .parse()
      .map_err(|_| format!("the listen port {text:?} is not a port"))?,
    None => DEFAULT_LISTEN_PORT,
  };
  Ok(Command::Run(Config {
    host,
    port,
    username,
    password,
    room: setting("--room", "RESEEK_TEST_ROOM").unwrap_or_else(|| DEFAULT_ROOM.into()),
    query: setting("--query", "RESEEK_TEST_QUERY").unwrap_or_else(|| DEFAULT_QUERY.into()),
    max_size,
    listen_port,
  }))
}

/// `host:port`, with the host in brackets when it is an IPv6 address
fn parse_server(server: &str) -> Result<(String, u16), String> {
  let invalid = || format!("RESEEK_TEST_SERVER {server:?} is not host:port");
  let (host, port) = server.rsplit_once(':').ok_or_else(invalid)?;
  let host = host.trim_start_matches('[').trim_end_matches(']');
  let port = port.parse().map_err(|_| invalid())?;
  if host.is_empty() {
    return Err(invalid());
  }
  Ok((host.to_owned(), port))
}

/// the `--help` text, with the defaults filled in
fn help() -> String {
  format!(
    "\
Logs in to a Soulseek server, joins a room, runs one network search, browses a user who
answered, downloads one small file that user shares into a temporary directory, checks its
size, disconnects and exits 0. A failed step exits 1 with one line naming the step.

Use a throwaway account made for this, never a personal one: a second login with the same
account kicks the first.

It shares nothing and answers no searches. Peers connect back on the listen port, so forward
it for the best chance of a download.

Environment:
  RESEEK_TEST_USERNAME     account to log in as (required)
  RESEEK_TEST_PASSWORD     its password (required)
  RESEEK_TEST_SERVER       host:port of the server [default: {DEFAULT_SERVER_ADDRESS}:{DEFAULT_SERVER_PORT}]
  RUST_LOG                 log filter, as in soulseek_client=debug,info [default: info]

Options, each also read from the variable named after it:
  --room <name>            room to join (RESEEK_TEST_ROOM) [default: {DEFAULT_ROOM}]
  --query <text>           what to search for (RESEEK_TEST_QUERY) [default: {DEFAULT_QUERY}]
  --max-size <bytes>       largest file to download (RESEEK_TEST_MAX_SIZE) [default: {DEFAULT_MAX_SIZE}]
  --listen-port <port>     port peers connect to (RESEEK_TEST_LISTEN_PORT) [default: {DEFAULT_LISTEN_PORT}]
  -h, --help               print this and exit
"
  )
}

/// info and above, or what `RUST_LOG` asks for, on stderr
fn init_logging() {
  let requested = std::env::var("RUST_LOG").ok();
  let parsed = requested.as_deref().map(str::parse::<Targets>);
  let filter = match &parsed {
    Some(Ok(targets)) => targets.clone(),
    _ => Targets::new().with_default(tracing::Level::INFO),
  };
  tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
    .with(filter)
    .init();
  if let Some(Err(error)) = parsed {
    tracing::warn!(%error, "RUST_LOG is not a log filter, logging at info");
  }
}

/// runs every step, then disconnects and removes the download directory however the steps went
async fn run(config: &Config) -> Result<(), Failure> {
  let options = ClientOptions {
    listen_port: config.listen_port,
    ..ClientOptions::default()
  };
  let client = Client::new(options).map_err(|error| Failure::new("start", error.to_string()))?;
  let directory = std::env::temp_dir().join(format!("reseek-live-smoke-{}", std::process::id()));
  let cancel = CancellationToken::new();
  let result = steps(&client, config, &directory, &cancel).await;
  let disconnected = tokio::time::timeout(
    DISCONNECT_TIMEOUT,
    client.disconnect(Some("Smoke test finished"), None),
  )
  .await;
  // ends any transfer or wait a failed step left behind
  client.dispose().await;
  if let Err(error) = tokio::fs::remove_dir_all(&directory).await
    && error.kind() != std::io::ErrorKind::NotFound
  {
    tracing::warn!(directory = %directory.display(), %error, "download directory not removed");
  }
  result?;
  disconnected.map_err(|_| Failure::new("disconnect", timed_out(DISCONNECT_TIMEOUT)))
}

/// the steps in order; the first to fail ends the run
async fn steps(
  client: &Client,
  config: &Config,
  directory: &Path,
  cancel: &CancellationToken,
) -> Result<(), Failure> {
  let connect = client.connect(
    &config.host,
    config.port,
    &config.username,
    &config.password,
    cancel,
  );
  step("connect", CONNECT_TIMEOUT, cancel, connect).await?;

  let joined = client.join_room(&config.room, RoomType::Public);
  let room = step("join room", JOIN_TIMEOUT, cancel, joined).await?;
  tracing::info!(room = room.name, members = room.user_count(), "joined room");

  let responses = search(client, &config.query, cancel)
    .instrument(step_span("search"))
    .await?;
  let pick = pick(&responses, config.max_size).ok_or_else(|| {
    Failure::new(
      "pick",
      format!(
        "none of the {} responses offered a non-empty file of at most {} bytes from a user with \
         a free upload slot",
        responses.len(),
        config.max_size
      ),
    )
  })?;
  tracing::info!(
    username = pick.username,
    filename = %pick.filename,
    size = pick.size,
    "picked file"
  );

  let browsed = client.browse(&pick.username, BrowseOptions::default(), cancel);
  let listing = step("browse", BROWSE_TIMEOUT, cancel, browsed).await?;
  tracing::info!(
    username = pick.username,
    directories = listing.directories.len(),
    files = listing
      .directories
      .iter()
      .map(|directory| directory.files.len())
      .sum::<usize>(),
    "browsed user"
  );

  // a fixed local name, so the remote filename never becomes a local path
  let destination = directory.join("download");
  tokio::fs::create_dir_all(directory)
    .await
    .map_err(|error| {
      let reason = format!("cannot create {}: {error}", directory.display());
      Failure::new("download", reason)
    })?;
  let request = DownloadRequest::new(
    &pick.username,
    pick.filename.clone(),
    pick.size,
    DownloadOutput::File(destination.clone()),
  );
  let downloaded = client.download(request, cancel);
  let transfer = step("download", DOWNLOAD_TIMEOUT, cancel, downloaded).await?;
  verify(&destination, transfer.size, transfer.bytes_transferred).await
}

/// Runs one step under `limit`. A step that runs out of time cancels `cancel`, which stops
/// whatever it started.
async fn step<T>(
  name: &'static str,
  limit: Duration,
  cancel: &CancellationToken,
  work: impl Future<Output = Result<T, ClientError>>,
) -> Result<T, Failure> {
  match tokio::time::timeout(limit, work)
    .instrument(step_span(name))
    .await
  {
    Ok(Ok(value)) => Ok(value),
    Ok(Err(error)) => Err(Failure::new(name, error.to_string())),
    Err(_) => {
      cancel.cancel();
      Err(Failure::new(name, timed_out(limit)))
    }
  }
}

/// the span every line logged during a step carries
fn step_span(name: &'static str) -> tracing::Span {
  tracing::info_span!("step", step = name)
}

fn timed_out(limit: Duration) -> String {
  format!("no result within {} s", limit.as_secs())
}

/// Searches the whole network for `query`. The search ends on its own once responses stop
/// coming; one still taking them when the window closes is cancelled and ends with what it has.
async fn search(
  client: &Client,
  query: &str,
  cancel: &CancellationToken,
) -> Result<Vec<SearchResponse>, Failure> {
  let responses = Arc::new(Mutex::new(Vec::new()));
  let collect = {
    let responses = responses.clone();
    move |response| {
      responses
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(response);
    }
  };
  let options = SearchOptions {
    search_timeout: SEARCH_IDLE,
    ..SearchOptions::default()
  };
  let window = cancel.child_token();
  let searching = client.search_with(query, SearchScope::Network, None, options, collect, &window);
  tokio::pin!(searching);
  let searched = tokio::select! {
    searched = &mut searching => searched,
    () = tokio::time::sleep(SEARCH_WINDOW) => {
      tracing::info!(window = ?SEARCH_WINDOW, "search window closed");
      window.cancel();
      searching.await
    }
  };
  match searched {
    Ok(_) => {}
    // the window closed; the responses taken so far are the result
    Err(ClientError::Cancelled) if window.is_cancelled() => {}
    Err(error) => return Err(Failure::new("search", error.to_string())),
  }
  let responses = std::mem::take(&mut *responses.lock().unwrap_or_else(PoisonError::into_inner));
  if responses.is_empty() {
    return Err(Failure::new("search", "no user answered"));
  }
  Ok(responses)
}

/// The smallest non-empty file of at most `max_size` bytes offered by a user with a free upload
/// slot, the file name and then the username breaking ties. An empty file is passed over, since
/// it would show nothing about bytes landing.
fn pick(responses: &[SearchResponse], max_size: u64) -> Option<Pick> {
  responses
    .iter()
    .filter(|response| response.has_free_upload_slot)
    .flat_map(|response| response.files.iter().map(move |file| (response, file)))
    .filter(|(_, file)| file.size > 0 && file.size <= max_size)
    .min_by(|(a_response, a), (b_response, b)| {
      a.size
        .cmp(&b.size)
        .then_with(|| a.filename.as_str().cmp(b.filename.as_str()))
        .then_with(|| a_response.username.cmp(&b_response.username))
    })
    .map(|(response, file)| Pick {
      username: response.username.clone(),
      filename: file.filename.clone(),
      size: file.size,
    })
}

/// checks the file at `path` holds the `size` bytes the uploader announced
async fn verify(path: &Path, size: u64, transferred: u64) -> Result<(), Failure> {
  let written = tokio::fs::metadata(path)
    .await
    .map_err(|error| Failure::new("verify", format!("cannot read the download: {error}")))?
    .len();
  if written != size || transferred != size {
    return Err(Failure::new(
      "verify",
      format!(
        "the uploader announced {size} bytes, {transferred} were transferred and the file holds \
         {written}"
      ),
    ));
  }
  tracing::info!(bytes = written, "download verified");
  Ok(())
}

#[cfg(test)]
mod tests {
  use soulseek_proto::peer::File;

  use super::*;

  fn response(username: &str, free_slot: bool, files: &[(&str, u64)]) -> SearchResponse {
    SearchResponse {
      username: username.into(),
      token: 1,
      files: files
        .iter()
        .map(|&(filename, size)| File {
          code: 1,
          filename: filename.into(),
          size,
          extension: String::new(),
          attributes: Vec::new(),
        })
        .collect(),
      has_free_upload_slot: free_slot,
      upload_speed: 0,
      queue_length: 0,
      locked_files: Vec::new(),
    }
  }

  #[test]
  fn pick_takes_the_smallest_allowed_file_and_breaks_ties_by_name_then_user() {
    let cases = [
      (
        "a user without a free slot is passed over",
        vec![
          response("busy", false, &[("a", 10)]),
          response("free", true, &[("b", 20)]),
        ],
        Some(("free", "b")),
      ),
      (
        "an empty file is passed over",
        vec![response("u", true, &[("empty", 0), ("b", 20)])],
        Some(("u", "b")),
      ),
      (
        "a file over the ceiling is passed over, leaving nothing",
        vec![response("over", true, &[("a", 101)])],
        None,
      ),
      (
        "a file at the ceiling is taken",
        vec![response("at", true, &[("b", 100)])],
        Some(("at", "b")),
      ),
      (
        "the smaller of two sizes wins",
        vec![response("u", true, &[("a", 30), ("b", 20)])],
        Some(("u", "b")),
      ),
      (
        "one size: the file name decides, before the username",
        vec![
          response("a", true, &[("y", 20)]),
          response("z", true, &[("x", 20)]),
        ],
        Some(("z", "x")),
      ),
      (
        "one size and name: the username decides",
        vec![
          response("z", true, &[("x", 20)]),
          response("a", true, &[("x", 20)]),
        ],
        Some(("a", "x")),
      ),
    ];
    for (case, responses, expected) in cases {
      let picked = pick(&responses, 100);
      let picked = picked
        .as_ref()
        .map(|pick| (pick.username.as_str(), pick.filename.as_str()));
      assert_eq!(picked, expected, "{case}");
    }
  }
}
