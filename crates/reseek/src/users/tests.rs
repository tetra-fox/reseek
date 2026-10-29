//! The users directory and blocking calls through the API, against the in-memory daemon: the
//! listing's filters and sorts, groups, the blocking conflicts, the expiry sweep, imports and
//! forgetting a user.

use super::store::Change;
use super::touch;
use super::*;
use crate::services::BlockingService;
use crate::testing::{app, request};
use crate::transfers::fixtures::{Row, insert};
use api_types::events::EventType;
use api_types::transfers::{SortOrder, TransferDirection};
use api_types::users::blocking::BlockSource;
use api_types::users::{UserList, UserListQuery};
use api_types::users::{UserPresence, UserSort};
use serde_json::{Value, json};

const CONFIG: &str = "[soulseek]\nusername = \"me\"\n\
                      [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                      [transfers.groups.blacklisted]\nmembers = [\"mallory\"]\n\
                      [transfers.groups.user_defined.friends]\nmembers = [\"bob\"]\n\
                      [transfers.groups.user_defined.vip]\nmembers = []\n";

fn query() -> UserListQuery {
  serde_json::from_value(json!({})).unwrap()
}

fn names(list: UserList) -> Vec<String> {
  list.users.into_iter().map(|u| u.user.username).collect()
}

#[tokio::test]
async fn the_directory_filters_and_sorts_with_pinned_users_first() {
  let app = app(CONFIG, &[]).await;
  let directory = &app.state.users;
  for (name, notes) in [("alice", ""), ("Bob", "Plays JAZZ"), ("carol", "")] {
    directory
      .change(name, Change::Notes(notes.into()))
      .await
      .unwrap();
  }
  directory
    .change("carol", Change::Tags(vec!["jazzy".into()]))
    .await
    .unwrap();
  directory
    .change("alice", Change::Pinned(true))
    .await
    .unwrap();
  let away = touch::Pending {
    seen_at: Some(Utc::now()),
    status: Some((UserPresence::Away, false, Utc::now())),
    ..Default::default()
  };
  let batch = std::collections::BTreeMap::from([("Bob".to_string(), away)]);
  directory
    .db
    .write(move |conn| touch::write(conn, &batch))
    .await
    .unwrap();
  let upload = |name: &str| Row::new(name, TransferDirection::Upload, 48);
  insert(
    &app.state.transfers,
    vec![upload("carol"), upload("carol"), upload("Bob")],
  )
  .await;

  let mut by_name = query();
  by_name.sort = UserSort::Username;
  assert_eq!(
    names(directory.list(by_name.clone()).await.unwrap()),
    ["alice", "Bob", "carol"],
    "pinned first, then by name"
  );
  by_name.order = Some(SortOrder::Desc);
  assert_eq!(
    names(directory.list(by_name).await.unwrap()),
    ["alice", "carol", "Bob"]
  );
  let mut jazz = query();
  jazz.q = Some("JaZz".into());
  assert_eq!(
    names(directory.list(jazz).await.unwrap()),
    ["carol", "Bob"],
    "notes and tags in any case, the latest seen first"
  );
  let mut presence = query();
  presence.presence = Some(UserPresence::Away);
  assert_eq!(names(directory.list(presence).await.unwrap()), ["Bob"]);
  let mut uploads = query();
  uploads.sort = UserSort::Uploads;
  uploads.has_transfers = Some(true);
  let list = directory.list(uploads).await.unwrap();
  assert_eq!(list.total, 2);
  assert_eq!(
    names(list),
    ["carol", "Bob"],
    "most succeeded uploads first"
  );
  let mut friends = query();
  friends.group = Some("friends".into());
  assert!(
    directory.list(friends).await.unwrap().users.is_empty(),
    "Bob is not bob"
  );
  let mut idle = query();
  idle.has_transfers = Some(false);
  idle.limit = 1;
  let page = directory.list(idle).await.unwrap();
  assert_eq!(page.total, 1);
  assert_eq!(names(page), ["alice"]);

  let mut tagged = query();
  tagged.tag = Some("jazzy".into());
  assert_eq!(names(directory.list(tagged).await.unwrap()), ["carol"]);
  let mut pinned = query();
  pinned.pinned = Some(true);
  assert_eq!(names(directory.list(pinned).await.unwrap()), ["alice"]);
  let (status, _) = app
    .call("PUT", "/api/v0/users/carol/block", Some(json!({})))
    .await;
  assert_eq!(status, 200);
  let mut blocked = query();
  blocked.blocked = Some(true);
  assert_eq!(
    names(directory.list(blocked.clone()).await.unwrap()),
    ["carol"]
  );
  blocked.blocked = Some(false);
  assert_eq!(
    names(directory.list(blocked).await.unwrap()),
    ["alice", "Bob"]
  );
  let mut first = query();
  first.sort = UserSort::FirstSeen;
  first.order = Some(SortOrder::Asc);
  assert_eq!(
    names(directory.list(first).await.unwrap()),
    ["alice", "Bob", "carol"],
    "pinned first, then in the order they were first seen"
  );
  insert(
    &app.state.transfers,
    vec![Row::new("carol", TransferDirection::Download, 48)],
  )
  .await;
  let mut downloads = query();
  downloads.sort = UserSort::Downloads;
  assert_eq!(
    names(directory.list(downloads).await.unwrap()),
    ["alice", "carol", "Bob"],
    "most succeeded downloads first after the pinned"
  );
}

#[tokio::test]
async fn a_block_request_is_checked_before_it_is_made() {
  let app = app(CONFIG, &[]).await;
  for body in [
    json!({ "reason": "x".repeat(501) }),
    json!({ "expiresIn": "1h", "expiresAt": "2099-01-01T00:00:00Z" }),
    json!({ "expiresIn": "soon" }),
    json!({ "expiresAt": "2001-01-01T00:00:00Z" }),
    json!({ "expiresIn": "0s" }),
    json!({ "scopes": ["everything"] }),
    json!({ "scopes": [] }),
  ] {
    let (status, _) = app
      .call("PUT", "/api/v0/users/alice/block", Some(body.clone()))
      .await;
    assert_eq!(status, 400, "{body}");
  }
  let (status, block) = app
    .call(
      "PUT",
      "/api/v0/users/alice/block",
      Some(json!({ "expiresIn": "1h", "scopes": ["downloads"] })),
    )
    .await;
  assert_eq!(status, 200);
  assert_eq!(block["scopes"], json!(["downloads"]));
}

#[tokio::test]
async fn writes_push_the_users_summary_and_forgetting_pushes_the_removal() {
  let app = app(CONFIG, &[]).await;
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=users", None),
    )
    .await;
  stream.until("hello").await;
  let (status, _) = app
    .call(
      "PUT",
      "/api/v0/users/alice/notes",
      Some(json!({ "notes": "kind\r\nhelpful\nslow at night" })),
    )
    .await;
  assert_eq!(status, 204);
  let updated = stream.until("users.updated").await.pop().unwrap();
  assert_eq!(updated.data["username"], "alice");
  assert_eq!(
    updated.data["notes"], "kind\nhelpful",
    "the first two lines"
  );
  assert_eq!(
    updated.data["description"],
    json!(null),
    "no info was ever fetched"
  );
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  let removed = stream.until("users.removed").await.pop().unwrap();
  assert_eq!(removed.data, json!({ "username": "alice" }));
}

#[tokio::test]
async fn users_not_seen_for_the_retention_are_forgotten_unless_they_hold_something() {
  let app = app(&format!("[retention]\nusers = \"1d\"\n{CONFIG}"), &[]).await;
  let directory = &app.state.users;
  let old = Utc::now() - chrono::Duration::days(3);
  let seen = |at| touch::Pending {
    seen_at: Some(at),
    creates: true,
    ..Default::default()
  };
  let batch = std::collections::BTreeMap::from([
    ("gone".to_string(), seen(old)),
    ("noted".to_string(), seen(old)),
    ("recent".to_string(), seen(Utc::now())),
  ]);
  directory
    .db
    .write(move |conn| touch::write(conn, &batch))
    .await
    .unwrap();
  directory
    .db
    .write(|conn| {
      conn.execute(
        "UPDATE users SET notes = 'keep' WHERE username = 'noted'",
        [],
      )
    })
    .await
    .unwrap();
  directory.forget_unseen().await;
  let left: Vec<String> = directory
    .db
    .read(|conn| {
      let mut statement = conn.prepare("SELECT username FROM users ORDER BY username")?;
      statement.query_map([], |row| row.get(0))?.collect()
    })
    .await
    .unwrap();
  assert_eq!(left, ["noted", "recent"]);
}

#[tokio::test]
async fn a_group_the_configuration_contradicts_is_refused() {
  let app = app(CONFIG, &[]).await;
  let group = |name: &str| Some(json!({ "group": name }));
  let (status, body) = app
    .call("PUT", "/api/v0/users/bob/group", group("vip"))
    .await;
  assert_eq!(status, 409);
  assert!(
    body["error"].as_str().unwrap().contains("friends"),
    "{body}"
  );
  let (status, _) = app
    .call("PUT", "/api/v0/users/bob/group", group("friends"))
    .await;
  assert_eq!(status, 204, "the group the configuration puts the user in");
  let (status, _) = app
    .call("PUT", "/api/v0/users/alice/group", group("vip"))
    .await;
  assert_eq!(status, 204);
  let (_, summary) = app.call("GET", "/api/v0/users/alice/summary", None).await;
  assert_eq!(
    summary["group"],
    json!({ "name": "vip", "source": "override", "priority": 1, "color": "blue" })
  );
  assert_eq!(summary["groupOverride"], "vip");
  assert_eq!(summary["known"], true);
  let without_vip = CONFIG.replace("[transfers.groups.user_defined.vip]\nmembers = []\n", "");
  std::fs::write(app.dir.path().join("config.toml"), without_vip).unwrap();
  app.state.config.reload().await.unwrap();
  let (_, profile) = app.call("GET", "/api/v0/users/alice/profile", None).await;
  assert_eq!(profile["group"]["source"], "builtin");
  assert_eq!(
    profile["groupOverride"], "vip",
    "kept, so the page can say its group is gone"
  );
}

/// Each listed block's user and source.
fn sources(listed: &Value) -> Vec<(&str, &str)> {
  listed["users"]
    .as_array()
    .unwrap()
    .iter()
    .map(|b| {
      (
        b["username"].as_str().unwrap(),
        b["source"].as_str().unwrap(),
      )
    })
    .collect()
}

#[tokio::test]
async fn blocking_ourselves_or_a_configured_block_is_a_conflict() {
  let app = app(CONFIG, &[]).await;
  let (status, _) = app
    .call("PUT", "/api/v0/users/me/block", Some(json!({})))
    .await;
  assert_eq!(status, 409);
  let (status, body) = app
    .call("DELETE", "/api/v0/users/mallory/block", None)
    .await;
  assert_eq!(status, 409);
  assert_eq!(body["error"], "Blocked by the configuration");
  let (status, _) = app.call("DELETE", "/api/v0/users/alice/block", None).await;
  assert_eq!(status, 404);

  let expiring = Some(json!({ "expiresIn": "1h" }));
  let (status, block) = app.call("PUT", "/api/v0/users/alice/block", expiring).await;
  assert_eq!(status, 200);
  assert_eq!(
    block["scopes"],
    json!(["uploads", "searches", "browse", "messages", "rooms"])
  );
  assert_eq!(block["source"], "api");
  assert!(
    app
      .state
      .users
      .is_blocked("alice", None, BlockScope::Messages)
  );
  let (_, listed) = app.call("GET", "/api/v0/users/blocked", None).await;
  assert_eq!(sources(&listed), [("alice", "api"), ("mallory", "config")]);
  let (_, summary) = app.call("GET", "/api/v0/users/mallory/summary", None).await;
  assert_eq!(summary["block"]["source"], "config");
  let (status, _) = app
    .call("PUT", "/api/v0/users/mallory/block", Some(json!({})))
    .await;
  assert_eq!(status, 200);
  let (_, listed) = app.call("GET", "/api/v0/users/blocked", None).await;
  assert_eq!(
    sources(&listed),
    [("alice", "api"), ("mallory", "api")],
    "a member we also block is listed once"
  );
  let (status, _) = app.call("DELETE", "/api/v0/users/alice/block", None).await;
  assert_eq!(status, 204);
  assert!(
    !app
      .state
      .users
      .is_blocked("alice", None, BlockScope::Messages)
  );
}

#[tokio::test]
async fn an_expired_block_is_swept_with_its_event() {
  let app = app(CONFIG, &[]).await;
  let (seen, mut received) = tokio::sync::mpsc::unbounded_channel();
  app
    .state
    .events
    .subscribe("test", Some(EventType::UserUnblocked), move |record| {
      let seen = seen.clone();
      async move {
        seen.send(record).unwrap();
        Ok(())
      }
    });
  let directory = &app.state.users;
  let block = Block {
    username: "alice".into(),
    reason: String::new(),
    scopes: vec![BlockScope::Uploads],
    created_at: Some(Utc::now()),
    expires_at: Some(Utc::now() - chrono::Duration::seconds(1)),
    created_by: Some("admin".into()),
    source: BlockSource::Api,
  };
  let stored = block.clone();
  directory
    .db
    .write(move |conn| blocking::store(conn, &stored, Utc::now()))
    .await
    .unwrap();
  directory.blocks.set(&block);
  assert!(
    !directory.is_blocked("alice", None, BlockScope::Uploads),
    "expired already"
  );
  let (status, _) = app.call("DELETE", "/api/v0/users/alice/block", None).await;
  assert_eq!(
    status, 404,
    "an expired block is not lifted, the sweep ends it"
  );
  directory.expire_blocks().await;
  let record = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
    .await
    .unwrap()
    .unwrap();
  match &record.event {
    Event::UserUnblocked(unblocked) => {
      assert_eq!(unblocked.username, "alice");
      assert_eq!(unblocked.reason, UnblockReason::Expired);
    }
    other => panic!("{other:?}"),
  }
  let left = directory
    .db
    .read(|conn| blocking::stored_of(conn, "alice"))
    .await;
  assert_eq!(left.unwrap(), None);
}

#[tokio::test]
async fn an_import_counts_what_it_added_and_skipped() {
  let app = app(CONFIG, &[]).await;
  app
    .call("PUT", "/api/v0/users/carol/block", Some(json!({})))
    .await;
  let mut import = request("POST", "/api/v0/users/blocked/import", None);
  *import.body_mut() = axum::body::Body::from("alice\n# spam\nme\ncarol\nbob\n");
  import
    .headers_mut()
    .insert("content-type", "text/plain".parse().unwrap());
  let answer = app.send(import).await;
  assert_eq!(answer.status, 200);
  assert_eq!(answer.json(), json!({ "added": 2, "skipped": 2 }));
  let exported = app.state.users.export().await.unwrap();
  let names: Vec<_> = exported.iter().map(|b| b.username.as_str()).collect();
  assert_eq!(names, ["alice", "bob", "carol"]);

  let blocks = json!([
    { "username": "dave", "reason": "x".repeat(501), "scopes": ["uploads"], "source": "api" },
    { "username": "erin", "reason": "x".repeat(500), "scopes": ["uploads"], "source": "api" },
  ]);
  let mut import = request("POST", "/api/v0/users/blocked/import", Some(blocks));
  import
    .headers_mut()
    .insert("content-type", "application/json".parse().unwrap());
  let answer = app.send(import).await;
  assert_eq!(answer.status, 200);
  assert_eq!(
    answer.json(),
    json!({ "added": 1, "skipped": 1 }),
    "a reason over 500 characters is skipped"
  );
}

#[tokio::test]
async fn forgetting_a_user_drops_their_block_and_purges_their_transfers() {
  let app = app(CONFIG, &[]).await;
  app
    .call("PUT", "/api/v0/users/alice/block", Some(json!({})))
    .await;
  let download = Row::new("alice", TransferDirection::Download, 48);
  insert(&app.state.transfers, vec![download]).await;
  let (status, _) = app
    .call("DELETE", "/api/v0/users/alice?purge=true", None)
    .await;
  assert_eq!(status, 204);
  assert!(
    !app
      .state
      .users
      .is_blocked("alice", None, BlockScope::Uploads)
  );
  let (_, listing) = app.call("GET", "/api/v0/users/alice/transfers", None).await;
  assert_eq!(listing["transfers"][0]["removed"], true);
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 404);
  let (_, profile) = app.call("GET", "/api/v0/users/alice/profile", None).await;
  assert_eq!(profile["known"], false);
  let refresh = "/api/v0/users/alice/profile?refresh=true";
  let (status, body) = app.call("GET", refresh, None).await;
  assert_eq!(status, 200, "offline, nothing could be asked");
  assert_eq!(body["known"], false);
}
