//! The files routes through the in-memory daemon: serving with ranges and caching, the
//! incomplete files a download writes, link tokens, listings and deletions inside their roots,
//! archives and log files.

use std::io::Read;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;

use crate::testing::{TRUSTED, TestApp, app, request, with};

/// Loopback is a trusted administrator; deleting is allowed.
const CONFIG: &str = "remote_file_management = true\n[web.authentication]\n\
                      trusted_networks = [\"127.0.0.0/8\"]\n";

fn b64(path: &str) -> String {
  URL_SAFE_NO_PAD.encode(path)
}

fn downloads(app: &TestApp) -> PathBuf {
  app.state.paths.downloads.clone()
}

async fn with_files(config: &str) -> TestApp {
  let app = app(config, &[]).await;
  let root = downloads(&app);
  std::fs::create_dir_all(root.join("Album/Scans")).unwrap();
  std::fs::write(root.join("Album/01.flac"), b"0123456789").unwrap();
  std::fs::write(root.join("Album/Scans/cover.jpg"), b"jpg").unwrap();
  std::fs::write(root.join("page.html"), b"<script></script>").unwrap();
  app
}

#[tokio::test]
async fn a_file_is_served_whole_in_a_range_or_not_at_all_when_unchanged() {
  let app = with_files(CONFIG).await;
  let uri = format!(
    "/api/v0/files/downloads/files/{}/content",
    b64("Album/01.flac")
  );
  let whole = app.send(request("GET", &uri, None)).await;
  assert_eq!(whole.status, 200);
  assert_eq!(&whole.body[..], b"0123456789");
  let headers = &whole.headers;
  assert_eq!(headers["content-type"], "audio/flac");
  assert_eq!(headers["accept-ranges"], "bytes");
  assert_eq!(headers["content-length"], "10");
  assert_eq!(headers["x-content-type-options"], "nosniff");
  assert_eq!(headers["content-security-policy"], "sandbox");
  assert_eq!(headers["cache-control"], "private, no-cache");
  assert_eq!(
    headers["content-disposition"],
    "inline; filename*=UTF-8''01.flac"
  );
  let etag = headers["etag"].to_str().unwrap().to_string();

  let ranged = app
    .send(with(request("GET", &uri, None), "range", "bytes=2-4"))
    .await;
  assert_eq!(ranged.status, 206);
  assert_eq!(&ranged.body[..], b"234");
  assert_eq!(ranged.headers["content-range"], "bytes 2-4/10");
  assert_eq!(ranged.headers["content-length"], "3");
  let tail = app
    .send(with(request("GET", &uri, None), "range", "bytes=-3"))
    .await;
  assert_eq!(&tail.body[..], b"789");
  for range in ["bytes=10-", "bytes=0-1,4-5"] {
    let refused = app
      .send(with(request("GET", &uri, None), "range", range))
      .await;
    assert_eq!(refused.status, 416, "{range}");
    assert_eq!(refused.headers["content-range"], "bytes */10");
  }
  // a range is of the file the caller holds part of; for any other, the whole is sent
  let if_range = |tag: &str| {
    let ranged = with(request("GET", &uri, None), "range", "bytes=2-4");
    with(ranged, "if-range", tag)
  };
  assert_eq!(app.send(if_range(&etag)).await.status, 206);
  let weak = format!("W/{etag}");
  for other in ["\"other\"", &weak, "Wed, 21 Oct 2015 07:28:00 GMT"] {
    let whole = app.send(if_range(other)).await;
    assert_eq!(whole.status, 200, "{other}");
    assert_eq!(&whole.body[..], b"0123456789", "{other}");
  }
  let unchanged = app
    .send(with(request("GET", &uri, None), "if-none-match", &etag))
    .await;
  assert_eq!(unchanged.status, 304);
  assert!(unchanged.body.is_empty());
  let head = app.send(request("HEAD", &uri, None)).await;
  assert_eq!(head.status, 200);
  assert_eq!(head.headers["content-length"], "10");
  assert!(head.body.is_empty());

  // markup can run script on this site, whatever its type says; it is saved, never shown
  std::fs::write(
    downloads(&app).join("feed.xml"),
    br#"<html xmlns="http://www.w3.org/1999/xhtml"><script>alert(1)</script></html>"#,
  )
  .unwrap();
  for name in ["page.html", "feed.xml"] {
    let page = format!(
      "/api/v0/files/downloads/files/{}/content?disposition=inline",
      b64(name)
    );
    let saved = app.send(request("GET", &page, None)).await;
    assert!(
      saved.headers["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment;"),
      "{name} is shown inline"
    );
    assert_eq!(saved.headers["content-security-policy"], "sandbox");
  }
  let missing = format!("/api/v0/files/downloads/files/{}/content", b64("nothing"));
  assert_eq!(app.send(request("GET", &missing, None)).await.status, 404);
}

#[tokio::test]
async fn an_incomplete_file_is_served_only_once_no_download_writes_it() {
  let app = with_files(CONFIG).await;
  let file = app.state.paths.incomplete.join("partial.flac");
  std::fs::write(&file, b"part").unwrap();
  let uri = format!(
    "/api/v0/files/incomplete/files/{}/content",
    b64("partial.flac")
  );
  let delete = format!("/api/v0/files/incomplete/files/{}", b64("partial.flac"));
  let guard = app.state.files.writing(&file);
  assert_eq!(app.send(request("GET", &uri, None)).await.status, 409);
  assert_eq!(app.send(request("DELETE", &delete, None)).await.status, 409);
  drop(guard);
  assert_eq!(app.send(request("GET", &uri, None)).await.status, 200);
  assert_eq!(app.send(request("DELETE", &delete, None)).await.status, 204);
  assert!(!file.exists());
}

#[tokio::test]
async fn an_incomplete_archive_leaves_out_the_files_a_download_writes() {
  let app = with_files(CONFIG).await;
  let album = app.state.paths.incomplete.join("Album");
  std::fs::create_dir_all(&album).unwrap();
  std::fs::write(album.join("done.flac"), b"done").unwrap();
  std::fs::write(album.join("partial.flac"), b"part").unwrap();
  let guard = app.state.files.writing(&album.join("partial.flac"));
  let uri = format!(
    "/api/v0/files/incomplete/directories/{}/archive",
    b64("Album")
  );
  let answer = app.send(request("GET", &uri, None)).await;
  assert_eq!(answer.status, 200);
  let zip = zip::ZipArchive::new(std::io::Cursor::new(answer.body.to_vec())).unwrap();
  let names: Vec<_> = zip
    .file_names()
    .map(|name| name.unwrap().into_owned())
    .collect();
  assert_eq!(names, ["done.flac"]);
  drop(guard);
}

#[tokio::test]
async fn a_link_token_opens_its_one_file_without_a_session() {
  let app = with_files(CONFIG).await;
  let path = format!("downloads/{}", b64("Album/01.flac"));
  let made = app
    .send(request(
      "POST",
      "/api/v0/session/file-token",
      Some(json!({ "path": path })),
    ))
    .await;
  assert_eq!(made.status, 200, "{:?}", made.body);
  let token = made.json()["token"].as_str().unwrap().to_string();
  let content = |file: &str, token: Option<&str>| {
    let mut uri = format!("/api/v0/files/downloads/files/{}/content", b64(file));
    if let Some(token) = token {
      uri.push_str(&format!("?t={token}"));
    }
    request("GET", &uri, None)
  };
  let stranger = "192.0.2.1:1";
  let opened = app
    .send_from(stranger, content("Album/01.flac", Some(&token)))
    .await;
  assert_eq!(opened.status, 200);
  assert_eq!(&opened.body[..], b"0123456789");
  let other = app
    .send_from(stranger, content("page.html", Some(&token)))
    .await;
  assert_eq!(other.status, 401, "made for another file");
  let none = app
    .send_from(stranger, content("Album/01.flac", None))
    .await;
  assert_eq!(none.status, 401);
  let forged = app
    .send_from(stranger, content("Album/01.flac", Some("a.b.c")))
    .await;
  assert_eq!(forged.status, 401);
  let missing = app
    .send(request(
      "POST",
      "/api/v0/session/file-token",
      Some(json!({ "path": format!("downloads/{}", b64("nothing")) })),
    ))
    .await;
  assert_eq!(missing.status, 404);
  let archive = format!(
    "/api/v0/files/downloads/directories/{}/archive?t={token}",
    b64("Album")
  );
  assert_eq!(
    app
      .send_from(stranger, request("GET", &archive, None))
      .await
      .status,
    401,
    "a token opens no archive"
  );
}

#[tokio::test]
async fn nothing_is_served_while_serving_files_is_off() {
  let app = with_files(&format!("{CONFIG}[web.files]\nserve = false\n")).await;
  let file = b64("Album/01.flac");
  for (method, uri, body) in [
    (
      "GET",
      format!("/api/v0/files/downloads/files/{file}/content"),
      None,
    ),
    (
      "GET",
      format!(
        "/api/v0/files/downloads/directories/{}/archive",
        b64("Album")
      ),
      None,
    ),
    (
      "POST",
      "/api/v0/session/file-token".to_string(),
      Some(json!({ "path": format!("downloads/{file}") })),
    ),
  ] {
    assert_eq!(
      app.send(request(method, &uri, body)).await.status,
      403,
      "{uri}"
    );
  }
  let listed = app
    .send(request("GET", "/api/v0/files/downloads/directories", None))
    .await;
  assert_eq!(listed.status, 200, "listing stays");
}

#[tokio::test]
async fn listings_and_deletions_stay_inside_their_roots() {
  let app = with_files(CONFIG).await;
  let root = downloads(&app);
  let listed = app
    .send(request("GET", "/api/v0/files/downloads/directories", None))
    .await;
  assert_eq!(listed.status, 200);
  let body = listed.json();
  assert_eq!(body["fullName"], "");
  assert_eq!(body["directories"][0]["name"], "Album");
  assert_eq!(body["files"][0]["name"], "page.html");
  let delete = |path: &str| {
    request(
      "DELETE",
      &format!("/api/v0/files/downloads/files/{}", b64(path)),
      None,
    )
  };
  #[cfg(unix)]
  {
    let outside = app.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("passwd"), b"x").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    assert_eq!(app.send(delete("link/passwd")).await.status, 403);
    assert!(outside.join("passwd").exists());
    let through = format!("/api/v0/files/downloads/directories/{}", b64("link"));
    assert_eq!(app.send(request("GET", &through, None)).await.status, 403);
  }
  assert_eq!(app.send(delete("Album/01.flac")).await.status, 204);
  assert_eq!(app.send(delete("Album/01.flac")).await.status, 404);
  let directory = format!("/api/v0/files/downloads/directories/{}", b64("Album"));
  assert_eq!(
    app.send(request("DELETE", &directory, None)).await.status,
    204
  );
  assert!(!root.join("Album").exists());

  let off = with_files(TRUSTED).await;
  assert_eq!(
    off.send(delete("page.html")).await.status,
    403,
    "remote file management is off"
  );
}

#[tokio::test]
async fn a_refusal_names_the_path_under_its_root_not_on_the_server() {
  let app = with_files(CONFIG).await;
  let root = downloads(&app).to_string_lossy().into_owned();
  let missing = b64("Album/02.flac");
  let folder = b64("Album");
  for (method, uri, body, message) in [
    (
      "GET",
      format!("/api/v0/files/downloads/files/{missing}/content"),
      None,
      "The file '/Album/02.flac' does not exist",
    ),
    (
      "DELETE",
      format!("/api/v0/files/downloads/files/{missing}"),
      None,
      "The file '/Album/02.flac' does not exist",
    ),
    (
      "GET",
      format!("/api/v0/files/downloads/directories/{}", b64("Gone")),
      None,
      "The directory '/Gone' does not exist",
    ),
    (
      "GET",
      format!(
        "/api/v0/files/downloads/directories/{}/archive",
        b64("Gone")
      ),
      None,
      "The directory '/Gone' does not exist",
    ),
    (
      "GET",
      format!("/api/v0/files/downloads/files/{folder}/content"),
      None,
      "'/Album' is not a file",
    ),
    (
      "POST",
      format!("/api/v0/files/downloads/files/{folder}/analysis"),
      None,
      "'/Album' is not a file",
    ),
    (
      "POST",
      "/api/v0/session/file-token".to_string(),
      Some(json!({ "path": format!("downloads/{folder}") })),
      "'/Album' is not a file",
    ),
  ] {
    let answer = app.send(request(method, &uri, body)).await;
    assert_eq!(answer.status, 404, "{method} {uri}");
    let error = answer.json()["error"].as_str().unwrap().to_string();
    assert_eq!(error, message, "{method} {uri}");
    assert!(!error.contains(&root), "{error}");
  }
}

#[tokio::test]
async fn a_directory_is_sent_as_a_zip_one_at_a_time_and_within_the_limit() {
  let app = with_files(CONFIG).await;
  let uri = format!(
    "/api/v0/files/downloads/directories/{}/archive",
    b64("Album")
  );
  let answer = app.send(request("GET", &uri, None)).await;
  assert_eq!(answer.status, 200);
  assert_eq!(answer.headers["content-type"], "application/zip");
  assert_eq!(
    answer.headers["content-disposition"],
    "attachment; filename=\"Album.zip\""
  );
  let mut zip = zip::ZipArchive::new(std::io::Cursor::new(answer.body.to_vec())).unwrap();
  let names: Vec<_> = zip
    .file_names()
    .map(|name| name.unwrap().into_owned())
    .collect();
  assert_eq!(names, ["01.flac", "Scans/cover.jpg"]);
  let mut data = Vec::new();
  zip
    .by_name("01.flac")
    .unwrap()
    .read_to_end(&mut data)
    .unwrap();
  assert_eq!(data, b"0123456789");

  // a file too large for the writer's backlog keeps the first archive open
  std::fs::write(downloads(&app).join("Album/big.wav"), vec![0u8; 4 << 20]).unwrap();
  let first = app
    .stream_from("127.0.0.1:1", request("GET", &uri, None))
    .await;
  assert_eq!(first.status, 200);
  let second = app.send(request("GET", &uri, None)).await;
  assert_eq!(second.status, 429);
  drop(first);
  // the slot is freed once the writer sees the first stream gone
  let mut third = 429;
  for _ in 0..200 {
    third = app.send(request("GET", &uri, None)).await.status.as_u16();
    if third != 429 {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
  }
  assert_eq!(third, 200, "a finished archive frees its principal's slot");

  let small = with_files(&format!(
    "{CONFIG}[web.files]\narchive_max_bytes = \"5B\"\n"
  ))
  .await;
  assert_eq!(small.send(request("GET", &uri, None)).await.status, 413);
}

#[tokio::test]
async fn log_files_are_listed_and_read_in_ranges() {
  let app = with_files(CONFIG).await;
  let logs = app.state.paths.logs.clone();
  std::fs::write(
    logs.join("reseek-20261025.log"),
    b"first line\nsecond line\n",
  )
  .unwrap();
  std::fs::write(logs.join("notes.txt"), b"").unwrap();
  let listed = app.send(request("GET", "/api/v0/logs/files", None)).await;
  assert_eq!(listed.status, 200);
  let names: Vec<_> = listed
    .json()
    .as_array()
    .unwrap()
    .iter()
    .map(|f| f["name"].clone())
    .collect();
  assert_eq!(names, [json!("reseek-20261025.log")]);
  let uri = "/api/v0/logs/files/reseek-20261025.log";
  let read = app
    .send(with(request("GET", uri, None), "range", "bytes=11-"))
    .await;
  assert_eq!(read.status, 206);
  assert_eq!(&read.body[..], b"second line\n");
  assert_eq!(read.headers["content-type"], "text/plain; charset=utf-8");
  let missing = app
    .send(request("GET", "/api/v0/logs/files/gone.log", None))
    .await;
  assert_eq!(missing.status, 404);
}

/// The WAV of `name` under the downloads directory, as the URI that loads it.
fn wav_uri(name: &str) -> String {
  format!("/api/v0/files/downloads/files/{}/wav", b64(name))
}

#[tokio::test]
async fn aiff_is_served_as_little_endian_pcm_and_a_range_lands_on_its_place() {
  let app = with_files(CONFIG).await;
  let samples: Vec<u8> = (0..24).collect();
  let file = crate::files::aiff_file(2, 16, &samples);
  std::fs::write(app.state.paths.downloads.join("a.aiff"), file).unwrap();
  let uri = wav_uri("a.aiff");

  let whole = app.send(request("GET", &uri, None)).await;
  assert_eq!(whole.status, 200);
  assert_eq!(whole.headers["content-type"], "audio/wav");
  assert_eq!(whole.headers["accept-ranges"], "bytes");
  assert_eq!(whole.headers["content-length"], "68");
  assert_eq!(
    whole.headers["content-disposition"],
    "inline; filename*=UTF-8''a.wav"
  );
  assert_eq!(&whole.body[..4], b"RIFF");
  let swapped: Vec<u8> = samples
    .chunks(2)
    .flat_map(|sample| [sample[1], sample[0]])
    .collect();
  assert_eq!(&whole.body[44..], swapped);

  // a range starts and ends inside a sample, and one crosses the header's end
  for (first, last) in [(49, 54), (40, 47), (0, 3), (67, 67)] {
    let range = format!("bytes={first}-{last}");
    let part = app
      .send(with(request("GET", &uri, None), "range", &range))
      .await;
    assert_eq!(part.status, 206, "{range}");
    assert_eq!(part.body, whole.body[first..=last], "{range}");
    assert_eq!(
      part.headers["content-range"],
      format!("bytes {first}-{last}/68")
    );
  }
  let head = app.send(request("HEAD", &uri, None)).await;
  assert_eq!(head.headers["content-length"], "68");
  assert!(head.body.is_empty());
}

#[tokio::test]
async fn alac_decodes_to_pcm_whose_ranges_match_the_whole_across_packets() {
  let app = with_files(CONFIG).await;
  let tone = include_bytes!("../../files/fixtures/tone.m4a");
  std::fs::write(app.state.paths.downloads.join("tone.m4a"), tone).unwrap();
  let uri = wav_uri("tone.m4a");
  let whole = app.send(request("GET", &uri, None)).await;
  assert_eq!(
    whole.status,
    200,
    "{:?}",
    String::from_utf8_lossy(&whole.body)
  );
  // two seconds of two 16-bit channels at 8000 Hz
  assert_eq!(whole.body.len(), 44 + 2 * 8000 * 4);
  let silent = whole.body[44..].iter().filter(|b| **b == 0).count();
  assert!(silent < whole.body.len() / 2, "the tone came out silent");
  for (first, last) in [
    (44, 47),
    (10_001, 10_500),
    (30_000, 60_001),
    (32_000, 32_043),
  ] {
    let range = format!("bytes={first}-{last}");
    let part = app
      .send(with(request("GET", &uri, None), "range", &range))
      .await;
    assert_eq!(part.status, 206, "{range}");
    assert_eq!(part.body, whole.body[first..=last], "{range}");
  }
}

#[tokio::test]
async fn what_cannot_be_served_plainly_is_refused_with_the_reason() {
  let app = with_files(CONFIG).await;
  let compressed = crate::files::aiff_c_file(b"ulaw", &[0; 16]);
  std::fs::write(app.state.paths.downloads.join("c.aifc"), compressed).unwrap();
  let refused = app.send(request("GET", &wav_uri("c.aifc"), None)).await;
  assert_eq!(refused.status, 415);
  assert!(
    refused.json()["error"]
      .as_str()
      .unwrap()
      .contains("compressed as ulaw")
  );
  let flac = app
    .send(request("GET", &wav_uri("Album/01.flac"), None))
    .await;
  assert_eq!(flac.status, 415);
}
