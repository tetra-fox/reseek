//! The profile routes: the text and the picture round trip through the file or an override,
//! a write leaves its value in effect, and what is locked, not allowed, too large or not a
//! picture is refused without changing the picture peers get.

use axum::body::Body;
use image::Rgba;
use serde_json::json;

use crate::testing::{TestApp, app, png, request, with};

/// The color of the pictures uploaded.
const BLUE: Rgba<u8> = Rgba([20, 120, 220, 255]);

/// Loopback is a trusted administrator, and changes are allowed.
const CONFIG: &str =
  "remote_configuration = true\n[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n";

/// A `PUT /profile/picture` of `bytes` sent as `content_type`.
async fn upload(app: &TestApp, bytes: Vec<u8>, content_type: &str) -> crate::testing::Answer {
  let mut put = request("PUT", "/api/v0/profile/picture", None);
  *put.body_mut() = Body::from(bytes);
  app.send(with(put, "content-type", content_type)).await
}

async fn profile(app: &TestApp) -> serde_json::Value {
  let answer = app.send(request("GET", "/api/v0/profile", None)).await;
  assert_eq!(answer.status, 200);
  answer.json()
}

#[tokio::test]
async fn the_text_and_the_picture_round_trip_through_the_file() {
  let app = app(CONFIG, &[]).await;
  let empty = profile(&app).await;
  assert_eq!(empty["description"], "A reseek user.");
  assert_eq!(empty["hasPicture"], false);
  assert_eq!(empty["pictureUrl"], json!(null));
  assert_eq!(
    empty["sources"]["description"],
    json!({ "layer": "default" })
  );
  assert!(empty["uploadSlots"].as_u64().unwrap() > 0);
  assert_eq!(empty["freeUploadSlot"], true);
  assert_eq!(empty["queueLength"], 0);

  let text = json!({ "description": "Jazz, mostly.\nAsk before you queue a lot." });
  let put = app
    .send(request("PUT", "/api/v0/profile", Some(text)))
    .await;
  assert_eq!(put.status, 204, "{:?}", put.body);
  let uploaded = upload(&app, png(1200, 300, BLUE), "image/png").await;
  assert_eq!(uploaded.status, 204, "{:?}", uploaded.body);

  let changed = profile(&app).await;
  assert_eq!(
    changed["description"],
    "Jazz, mostly.\nAsk before you queue a lot."
  );
  assert_eq!(changed["hasPicture"], true);
  assert_eq!(changed["pictureUrl"], "/api/v0/profile/picture");
  let file = json!({ "layer": "file", "file": "config.toml" });
  assert_eq!(changed["sources"]["description"], file);
  assert_eq!(changed["sources"]["picture"], file);
  let written = std::fs::read_to_string(app.dir.path().join("config.toml")).unwrap();
  assert!(written.contains("Jazz, mostly."), "{written}");
  assert!(written.contains("picture.jpg"), "{written}");

  let picture = app
    .send(request("GET", "/api/v0/profile/picture", None))
    .await;
  assert_eq!(picture.status, 200);
  assert_eq!(picture.headers["content-type"], "image/jpeg");
  let decoded = image::load_from_memory(&picture.body).unwrap();
  assert_eq!((decoded.width(), decoded.height()), (512, 128));
  let stored = app.state.paths.data.join("profile/picture.jpg");
  assert_eq!(std::fs::read(&stored).unwrap(), picture.body);

  let removed = app
    .send(request("DELETE", "/api/v0/profile/picture", None))
    .await;
  assert_eq!(removed.status, 204, "{:?}", removed.body);
  assert!(!stored.exists());
  let gone = app
    .send(request("GET", "/api/v0/profile/picture", None))
    .await;
  assert_eq!(gone.status, 404);
  let after = profile(&app).await;
  assert_eq!(after["hasPicture"], false);
  assert_eq!(after["sources"]["picture"], json!({ "layer": "default" }));
}

#[tokio::test]
async fn a_managed_file_takes_the_profile_as_an_override() {
  let app = app(CONFIG, &[("RESEEK_CONFIG_MANAGED_BY", "nixos")]).await;
  let text = json!({ "description": "from the page" });
  let put = app
    .send(request("PUT", "/api/v0/profile", Some(text)))
    .await;
  assert_eq!(put.status, 204, "{:?}", put.body);
  let changed = profile(&app).await;
  assert_eq!(changed["description"], "from the page");
  assert_eq!(changed["sources"]["description"]["layer"], "runtime");
  let overrides = std::fs::read_to_string(app.dir.path().join("overrides.toml")).unwrap();
  assert!(overrides.contains("from the page"), "{overrides}");
  let uploaded = upload(&app, png(8, 8, BLUE), "image/png").await;
  assert_eq!(uploaded.status, 204, "{:?}", uploaded.body);
  assert_eq!(
    profile(&app).await["sources"]["picture"]["layer"],
    "runtime"
  );
}

#[tokio::test]
async fn what_is_locked_too_large_or_no_picture_is_refused() {
  let app = app(
    CONFIG,
    &[
      ("RESEEK_SLSK_DESCRIPTION", "from env"),
      // any readable file passes the check at startup
      (
        "RESEEK_SLSK_PICTURE",
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
      ),
    ],
  )
  .await;
  let text = json!({ "description": "x" });
  let locked = app
    .send(request("PUT", "/api/v0/profile", Some(text)))
    .await;
  assert_eq!(locked.status, 409);
  assert_eq!(locked.json()["error"], "Set by RESEEK_SLSK_DESCRIPTION");
  let picture = upload(&app, png(8, 8, BLUE), "image/png").await;
  assert_eq!(picture.status, 409);
  assert_eq!(picture.json()["error"], "Set by RESEEK_SLSK_PICTURE");
  let removing = app
    .send(request("DELETE", "/api/v0/profile/picture", None))
    .await;
  assert_eq!(removing.status, 409);

  let open = crate::testing::app(CONFIG, &[]).await;
  assert_eq!(
    upload(&open, png(8, 8, BLUE), "text/plain").await.status,
    415
  );
  assert_eq!(
    upload(&open, png(8, 8, BLUE), "image/svg+xml").await.status,
    415
  );
  let huge = vec![0; super::MAX_UPLOAD + 1];
  assert_eq!(upload(&open, huge, "image/jpeg").await.status, 413);
  let garbage = upload(&open, b"not a picture".to_vec(), "image/png").await;
  assert_eq!(garbage.status, 400);
  assert!(!open.state.paths.data.join("profile/picture.jpg").exists());

  let closed = crate::testing::app(crate::testing::TRUSTED, &[]).await;
  let text = json!({ "description": "x" });
  let refused = closed
    .send(request("PUT", "/api/v0/profile", Some(text)))
    .await;
  assert_eq!(refused.status, 403, "remote configuration is off");
}

/// A configuration that also gives a stored API key the administrator role.
fn keyed() -> String {
  format!(
    "{CONFIG}[web.authentication.api_keys.robot]\nkey = \"robot-key-0123456789\"\n\
     role = \"administrator\"\n"
  )
}

/// `request` sent with the stored API key from an address that is not trusted.
async fn by_key(app: &TestApp, request: axum::http::Request<Body>) -> crate::testing::Answer {
  let keyed = with(request, "x-api-key", "robot-key-0123456789");
  app.send_from("192.0.2.1:1", keyed).await
}

#[tokio::test]
async fn a_stored_api_key_changes_no_file_and_a_refused_upload_no_picture() {
  let app = app(&keyed(), &[]).await;
  let stored = app.state.paths.data.join("profile/picture.jpg");
  let mut put = request("PUT", "/api/v0/profile/picture", None);
  *put.body_mut() = Body::from(png(16, 16, BLUE));
  let first = by_key(&app, with(put, "content-type", "image/png")).await;
  assert_eq!(first.status, 403, "{:?}", first.body);
  assert!(!stored.exists(), "no picture is made for a refused upload");

  assert_eq!(
    upload(&app, png(64, 64, BLUE), "image/png").await.status,
    204
  );
  let before = std::fs::read(&stored).unwrap();
  let mut put = request("PUT", "/api/v0/profile/picture", None);
  *put.body_mut() = Body::from(png(300, 300, BLUE));
  let refused = by_key(&app, with(put, "content-type", "image/png")).await;
  assert_eq!(refused.status, 403, "{:?}", refused.body);
  assert_eq!(
    std::fs::read(&stored).unwrap(),
    before,
    "the picture peers get"
  );
  let text = json!({ "description": "scripted" });
  let text = by_key(&app, request("PUT", "/api/v0/profile", Some(text))).await;
  assert_eq!(text.status, 403);
  let parts = std::fs::read_dir(stored.parent().unwrap()).unwrap().count();
  assert_eq!(parts, 1, "nothing staged is left behind");

  let managed = crate::testing::app(&keyed(), &[("RESEEK_CONFIG_MANAGED_BY", "nixos")]).await;
  let text = json!({ "description": "scripted" });
  let set = by_key(&managed, request("PUT", "/api/v0/profile", Some(text))).await;
  assert_eq!(set.status, 204, "an override needs nobody at the UI");
}

#[tokio::test]
async fn a_write_removes_an_override_of_the_key_so_its_value_is_in_effect() {
  let app = app(CONFIG, &[]).await;
  assert_eq!(
    upload(&app, png(32, 32, BLUE), "image/png").await.status,
    204
  );
  let path = app.state.paths.data.join("profile/picture.jpg");
  let text = json!({ "description": "in the file" });
  let put = app
    .send(request("PUT", "/api/v0/profile", Some(text)))
    .await;
  assert_eq!(put.status, 204);
  let set = json!({ "set": {
    "soulseek.picture": path.display().to_string(),
    "soulseek.description": "overridden",
  } });
  let patched = app
    .send(request("PATCH", "/api/v0/options", Some(set)))
    .await;
  assert_eq!(patched.status, 200, "{:?}", patched.body);
  let overridden = profile(&app).await;
  assert_eq!(overridden["sources"]["picture"]["layer"], "runtime");
  assert_eq!(overridden["description"], "overridden");

  let text = json!({ "description": "in the file" });
  let same = app
    .send(request("PUT", "/api/v0/profile", Some(text)))
    .await;
  assert_eq!(same.status, 204);
  let removed = app
    .send(request("DELETE", "/api/v0/profile/picture", None))
    .await;
  assert_eq!(removed.status, 204, "{:?}", removed.body);

  let after = profile(&app).await;
  assert_eq!(after["description"], "in the file");
  assert_eq!(after["sources"]["description"]["layer"], "file");
  assert_eq!(after["hasPicture"], false);
  assert_eq!(after["sources"]["picture"], json!({ "layer": "default" }));
  assert!(!path.exists());
  let view = app
    .send(request("GET", "/api/v0/options/view", None))
    .await
    .json();
  assert_eq!(view["overrides"], json!({}));
  app.state.config.reload().await.unwrap();
}

#[tokio::test]
async fn uploads_at_once_each_leave_a_whole_picture() {
  let app = app(CONFIG, &[]).await;
  let (a, b) = tokio::join!(
    upload(&app, png(600, 200, BLUE), "image/png"),
    upload(&app, png(200, 600, BLUE), "image/png")
  );
  assert_eq!(a.status, 204, "{:?}", a.body);
  assert_eq!(b.status, 204, "{:?}", b.body);
  let stored = app.state.paths.data.join("profile/picture.jpg");
  let picture = image::load_from_memory(&std::fs::read(&stored).unwrap()).unwrap();
  let size = (picture.width(), picture.height());
  assert!(size == (512, 171) || size == (171, 512), "{size:?}");
  let left = std::fs::read_dir(stored.parent().unwrap()).unwrap().count();
  assert_eq!(left, 1, "no staged file is left");
}

/// Appends a line to `config.toml` behind the daemon's back, so its next write there is stale.
fn edit_by_hand(app: &TestApp) {
  let path = app.dir.path().join("config.toml");
  let mut text = std::fs::read_to_string(&path).unwrap();
  text.push_str("\n# edited by hand\n");
  std::fs::write(&path, text).unwrap();
}

#[tokio::test]
async fn a_picture_whose_option_write_is_refused_is_not_kept() {
  let app = app(CONFIG, &[]).await;
  let stored = app.state.paths.data.join("profile/picture.jpg");
  let staged = || std::fs::read_dir(stored.parent().unwrap()).unwrap().count();
  edit_by_hand(&app);
  let first = upload(&app, png(16, 16, BLUE), "image/png").await;
  assert_eq!(first.status, 409, "{:?}", first.body);
  assert_eq!(
    first.json()["error"],
    "The configuration file changed on disk"
  );
  assert!(!stored.exists(), "a first picture is taken away again");
  assert_eq!(staged(), 0);

  app.state.config.reload().await.unwrap();
  assert_eq!(
    upload(&app, png(64, 64, BLUE), "image/png").await.status,
    204
  );
  let before = std::fs::read(&stored).unwrap();
  edit_by_hand(&app);
  let second = upload(&app, png(300, 300, BLUE), "image/png").await;
  assert_eq!(second.status, 409, "{:?}", second.body);
  assert_eq!(
    std::fs::read(&stored).unwrap(),
    before,
    "the old picture stays"
  );
  assert_eq!(staged(), 1, "nothing staged is left behind");
}

#[tokio::test]
async fn removing_a_picture_a_read_only_file_sets_under_an_override_is_refused() {
  let managed = format!(
    "{CONFIG}[soulseek]\npicture = \"{}\"\n",
    concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")
  );
  let app = app(&managed, &[("RESEEK_CONFIG_MANAGED_BY", "nixos")]).await;
  assert_eq!(
    upload(&app, png(32, 32, BLUE), "image/png").await.status,
    204
  );
  let uploaded = profile(&app).await;
  assert_eq!(uploaded["sources"]["picture"]["layer"], "runtime");
  let removing = app
    .send(request("DELETE", "/api/v0/profile/picture", None))
    .await;
  assert_eq!(removing.status, 409);
  let body = removing.json();
  assert_eq!(body["error"], "config.toml is read-only");
  assert!(
    body["detail"]
      .as_str()
      .unwrap()
      .starts_with("config.toml sets soulseek.picture, which can not be changed from here"),
    "{body}"
  );
  let after = profile(&app).await;
  assert_eq!(after["sources"], uploaded["sources"], "nothing changed");
  assert!(app.state.paths.data.join("profile/picture.jpg").exists());
}
