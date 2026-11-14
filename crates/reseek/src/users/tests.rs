//! The users directory and blocking calls through the API, against the in-memory daemon: the
//! listing's filters and sorts, groups, the blocking conflicts, the expiry sweep, imports and
//! forgetting a user.

use std::sync::Arc;

use super::store::Change;
use super::touch;
use super::*;
use crate::services::BlockingService;
use crate::testing::{app, request};
use crate::transfers::fixtures::{Row, insert};
use api_types::events::EventType;
use api_types::transfers::{SortOrder, TransferDirection};
use api_types::transfers::{TransferOutcome, TransferStates};
use api_types::users::blocking::BlockSource;
use api_types::users::{Buddy, BuddyChange, UserList, UserListQuery};
use api_types::users::{UserPresence, UserSort};
use serde_json::{Value, json};

const CONFIG: &str = "[soulseek]\nusername = \"me\"\n\
                      [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                      [transfers.groups.blacklisted]\nmembers = [\"mallory\"]\n\
                      [transfers.groups.user_defined.friends]\ncolor = \"teal\"\n\
                      [transfers.groups.user_defined.vip.upload]\nslots = 2\n";

fn query() -> UserListQuery {
  serde_json::from_value(json!({})).unwrap()
}

fn names(list: UserList) -> Vec<String> {
  list.users.into_iter().map(|u| u.user.username).collect()
}

#[tokio::test]
async fn the_directory_filters_and_sorts_with_buddies_first() {
  let app = app(CONFIG, &[]).await;
  let directory = &app.state.users;
  for (name, notes) in [("alice", ""), ("Bob", "Plays JAZZ"), ("carol", "")] {
    directory
      .change(name, Change::Notes(notes.into()))
      .await
      .unwrap();
  }
  directory
    .change("carol", Change::Groups(vec!["jazzy".into()]))
    .await
    .unwrap();
  directory
    .change("alice", Change::Buddy(Some(Buddy::default())))
    .await
    .unwrap();
  // reported an hour ago, so Bob was not seen later than the others
  let reported = Utc::now() - chrono::Duration::hours(1);
  let away = touch::Pending {
    seen_at: Some(reported),
    status: Some((UserPresence::Away, false, reported)),
    ..Default::default()
  };
  let batch = std::collections::BTreeMap::from([("Bob".to_string(), away)]);
  directory
    .db
    .write(move |conn| touch::write(conn, &batch, &Default::default()))
    .await
    .unwrap();
  let upload = |name: &str| {
    Row::new(
      name,
      TransferDirection::Upload,
      TransferStates::Completed(TransferOutcome::Succeeded),
    )
  };
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
    "buddies first, then by name"
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
    "notes and group names in any case, the latest seen first"
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
  let mut idle = query();
  idle.has_transfers = Some(false);
  idle.limit = 1;
  let page = directory.list(idle).await.unwrap();
  assert_eq!(page.total, 1);
  assert_eq!(names(page), ["alice"]);

  let mut grouped = query();
  grouped.group = Some("jazzy".into());
  assert_eq!(
    names(directory.list(grouped.clone()).await.unwrap()),
    ["carol"]
  );
  grouped.group = Some("buddies".into());
  assert_eq!(
    names(directory.list(grouped.clone()).await.unwrap()),
    ["alice"]
  );
  grouped.group = Some("default".into());
  assert_eq!(
    names(directory.list(grouped).await.unwrap()),
    ["alice", "carol", "Bob"],
    "everyone is in default"
  );
  let mut buddies = query();
  buddies.buddy = Some(true);
  assert_eq!(names(directory.list(buddies).await.unwrap()), ["alice"]);
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
    "buddies first, then in the order they were first seen"
  );
  insert(
    &app.state.transfers,
    vec![Row::new(
      "carol",
      TransferDirection::Download,
      TransferStates::Completed(TransferOutcome::Succeeded),
    )],
  )
  .await;
  let mut downloads = query();
  downloads.sort = UserSort::Downloads;
  assert_eq!(
    names(directory.list(downloads).await.unwrap()),
    ["alice", "carol", "Bob"],
    "most succeeded downloads first after the buddies"
  );
}

#[tokio::test]
async fn the_directory_never_lists_the_servers_reserved_name() {
  let app = app(CONFIG, &[]).await;
  let directory = &app.state.users;
  for name in ["server", "serverfan"] {
    directory
      .change(name, Change::Buddy(Some(Buddy::default())))
      .await
      .unwrap();
  }
  let mut asked = query();
  asked.q = Some("server".into());
  assert_eq!(names(directory.list(asked).await.unwrap()), ["serverfan"]);
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
async fn the_users_retention_sets_the_cutoff_of_forgetting() {
  // the cleanup the schedule would run at the start is off, so only the test's call forgets
  let config = format!("[schedules]\ncleanup = \"off\"\n[retention]\nusers = \"1d\"\n{CONFIG}");
  let app = app(&config, &[]).await;
  let directory = &app.state.users;
  let seen = |days| touch::Pending {
    seen_at: Some(Utc::now() - chrono::Duration::days(days)),
    creates: true,
    ..Default::default()
  };
  let batch = std::collections::BTreeMap::from([
    ("gone".to_string(), seen(3)),
    ("recent".to_string(), seen(0)),
  ]);
  directory
    .db
    .write(move |conn| touch::write(conn, &batch, &Default::default()))
    .await
    .unwrap();
  assert_eq!(directory.forget_unseen().await.unwrap(), 1);
}

#[tokio::test]
async fn groups_are_set_per_user_and_moved_or_let_go_together() {
  let app = app(CONFIG, &[]).await;
  let set = async |names: &[&str]| {
    let body = Some(json!({ "groups": names }));
    app.call("PUT", "/api/v0/users/alice/groups", body).await.0
  };
  let groups_of = async || {
    let (_, summary) = app.call("GET", "/api/v0/users/alice/summary", None).await;
    let names: Vec<String> = summary["groups"]
      .as_array()
      .unwrap()
      .iter()
      .map(|g| g["name"].as_str().unwrap().to_string())
      .collect();
    (names, summary["uploads"].clone())
  };
  assert_eq!(set(&["vip", "nope"]).await, 400, "only a defined group");
  assert_eq!(set(&["buddies"]).await, 400, "buddies is the buddy list");
  assert_eq!(set(&["vip"]).await, 204);
  assert_eq!(
    groups_of().await.1,
    json!({ "queue": "vip", "slots": 2, "speed": "default", "speedLimit": null, "limits": [] })
  );
  assert_eq!(set(&["vip", "friends", "vip"]).await, 204);
  let (names, upload) = groups_of().await;
  assert_eq!(names, ["friends", "vip"]);
  assert_eq!(
    upload["queue"], "vip",
    "friends sets nothing, so the slots are still vip's"
  );
  let (_, listed) = app.call("GET", "/api/v0/groups", None).await;
  let summary = |list: &Value| -> Vec<(String, u64, bool)> {
    list["groups"]
      .as_array()
      .unwrap()
      .iter()
      .map(|g| {
        (
          g["name"].as_str().unwrap().to_string(),
          g["members"].as_u64().unwrap(),
          g["defined"].as_bool().unwrap(),
        )
      })
      .collect()
  };
  let row = |name: &str, members: u64, defined: bool| (name.to_string(), members, defined);
  assert_eq!(
    summary(&listed),
    [
      row("privileged", 0, true),
      row("buddies", 0, true),
      row("leechers", 0, true),
      row("friends", 1, true),
      row("vip", 1, true),
      row("default", 1, true),
    ]
  );

  let without_vip = CONFIG.replace(
    "[transfers.groups.user_defined.vip.upload]\nslots = 2\n",
    "",
  );
  std::fs::write(app.dir.path().join("config.toml"), without_vip).unwrap();
  app.state.config.reload().await.unwrap();
  let (names, upload) = groups_of().await;
  assert_eq!(
    names,
    ["friends", "vip"],
    "kept, so the page can say it is gone"
  );
  assert_eq!(upload["queue"], "default");
  assert_eq!(
    set(&["vip"]).await,
    204,
    "a gone group the user is in stays"
  );
  assert_eq!(set(&["vip", "gone"]).await, 400);
  let (_, listed) = app.call("GET", "/api/v0/groups", None).await;
  assert_eq!(summary(&listed).last(), Some(&row("vip", 1, false)));

  let moved = Some(json!({ "to": "friends" }));
  let (status, body) = app
    .call("POST", "/api/v0/groups/vip/members/move", moved)
    .await;
  assert_eq!((status, &body["users"]), (200, &json!(1)));
  assert_eq!(groups_of().await.0, ["friends"]);
  let (status, body) = app
    .call("DELETE", "/api/v0/groups/friends/members", None)
    .await;
  assert_eq!((status, &body["users"]), (200, &json!(1)));
  assert!(groups_of().await.0.is_empty());
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
    addresses: Vec::new(),
    created_at: Some(Utc::now()),
    expires_at: Some(Utc::now() - chrono::Duration::seconds(1)),
    created_by: Some("admin".into()),
    source: BlockSource::Api,
  };
  let stored = block.clone();
  directory
    .db
    .write(move |conn| blocking::store(conn, &stored, Utc::now(), &Default::default()))
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
async fn an_import_that_does_not_read_is_logged_without_its_values() {
  let app = app(CONFIG, &[]).await;
  let (logs, _capturing) = crate::testing::captured_logs();
  let blocks = json!([{ "username": 4815162342_u64 }]);
  let answer = app
    .send(request(
      "POST",
      "/api/v0/users/blocked/import",
      Some(blocks),
    ))
    .await;
  assert_eq!(answer.status, 400);
  let logged = logs.snapshot();
  assert!(
    logged
      .iter()
      .any(|line| line.message.contains("[0].username does not fit")),
    "{logged:?}"
  );
  assert!(
    !logged
      .iter()
      .any(|line| line.message.contains("4815162342")),
    "{logged:?}"
  );
}

#[tokio::test]
async fn forgetting_a_user_drops_their_block_and_purges_their_transfers() {
  let app = app(CONFIG, &[]).await;
  app
    .call("PUT", "/api/v0/users/alice/block", Some(json!({})))
    .await;
  let download = Row::new(
    "alice",
    TransferDirection::Download,
    TransferStates::Completed(TransferOutcome::Succeeded),
  );
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

#[tokio::test]
async fn a_picture_too_large_or_not_cached_has_no_url() {
  let sent = |picture: &[u8]| info::PeerInfo {
    description: String::new(),
    picture: Some(picture.to_vec()),
    upload_slots: 1,
    queue_length: 0,
    has_free_slot: true,
  };
  let profile_info = async |app: &crate::testing::TestApp, name: &str| {
    let path = format!("/api/v0/users/{name}/profile");
    let (_, profile) = app.call("GET", &path, None).await;
    (
      profile["info"]["hasPicture"].clone(),
      profile["info"]["pictureUrl"].clone(),
    )
  };
  let app = app(
    &format!("{CONFIG}[users]\npicture_max_bytes = \"8B\"\n"),
    &[],
  )
  .await;
  app
    .state
    .users
    .store_info("bob", sent(b"GIF89a, too large"), true)
    .await;
  assert_eq!(
    profile_info(&app, "bob").await,
    (json!(true), Value::Null),
    "a picture too large is not kept"
  );

  let unkept =
    crate::testing::app(&format!("{CONFIG}[users]\ncache_pictures = false\n"), &[]).await;
  unkept
    .state
    .users
    .store_info("alice", sent(b"GIF89a"), true)
    .await;
  assert_eq!(
    profile_info(&unkept, "alice").await,
    (json!(true), Value::Null),
    "pictures are not kept"
  );
}

#[tokio::test]
async fn info_that_only_fills_in_makes_no_row() {
  let app = app(CONFIG, &[]).await;
  let users = &app.state.users;
  let said = || info::PeerInfo {
    description: "hello".into(),
    picture: None,
    upload_slots: 1,
    queue_length: 0,
    has_free_slot: true,
  };
  users.store_info("alice", said(), false).await;
  assert_eq!(alice(&app).await["known"], false);
  users.set_notes("alice", "kind".into()).await.unwrap();
  users.store_info("alice", said(), false).await;
  assert_eq!(alice(&app).await["info"]["description"], "hello");
}

#[tokio::test]
async fn a_buddy_change_keeps_what_it_leaves_out_and_sets_what_they_see() {
  let config =
    format!("{CONFIG}[shares.visible_to]\nRare = [\"buddies\"]\nPrivate = [\"friends\"]\n");
  let app = app(&config, &[]).await;
  let buddy = async |body: Option<Value>| {
    let (status, _) = app.call("PUT", "/api/v0/users/alice/buddy", body).await;
    assert_eq!(status, 204);
    let (_, summary) = app.call("GET", "/api/v0/users/alice/summary", None).await;
    summary["buddy"].clone()
  };
  let quiet = json!({ "notifyPresence": false });
  assert_eq!(buddy(None).await, quiet, "a new buddy is not followed");
  let buddies = std::collections::BTreeSet::from(["buddies".to_string()]);
  assert_eq!(app.state.users.sees("alice"), buddies);
  assert_eq!(
    buddy(Some(json!({ "notifyPresence": true }))).await,
    json!({ "notifyPresence": true })
  );
  assert_eq!(
    buddy(Some(json!({}))).await,
    json!({ "notifyPresence": true }),
    "left out, kept"
  );
  assert_eq!(
    buddy(Some(Value::Null)).await,
    json!({ "notifyPresence": true }),
    "null is no change"
  );
  let (status, _) = app.call("PUT", "/api/v0/users/me/buddy", None).await;
  assert_eq!(status, 409, "our own account is no buddy");
  let (status, _) = app.call("DELETE", "/api/v0/users/alice/buddy", None).await;
  assert_eq!(status, 204);
  let (_, summary) = app.call("GET", "/api/v0/users/alice/summary", None).await;
  assert_eq!(summary["buddy"], Value::Null);
  assert_eq!(buddy(None).await, quiet, "added again from scratch");
  assert!(app.state.users.sees("me").is_empty());
  app
    .state
    .users
    .set_groups("bob", vec!["vip".into()])
    .await
    .unwrap();
  assert!(
    app.state.users.sees("bob").is_empty(),
    "no share is shown to vip, so nothing is confirmed"
  );
  app
    .state
    .users
    .set_groups("bob", vec!["vip".into(), "friends".into()])
    .await
    .unwrap();
  assert_eq!(
    app.state.users.sees("bob"),
    std::collections::BTreeSet::from(["friends".to_string()]),
    "only the group a share is shown to"
  );
}

#[tokio::test]
async fn a_change_to_what_a_user_sees_forgets_the_kept_browse_views() {
  let config = format!("{CONFIG}[shares.visible_to]\nPrivate = [\"friends\"]\n");
  let app = app(&config, &[]).await;
  let (users, shares) = (&app.state.users, &app.state.shares);
  let everyone = shares.view(&std::collections::BTreeSet::new());
  let file = || shares.browse_cache(&everyone);
  let first = file();
  users.set_groups("bob", vec!["vip".into()]).await.unwrap();
  users
    .set_buddy("bob", BuddyChange::default())
    .await
    .unwrap();
  assert_eq!(file(), first, "no share is shown to vip or buddies");
  users
    .set_groups("bob", vec!["vip".into(), "friends".into()])
    .await
    .unwrap();
  let second = file();
  assert_ne!(second, first, "bob sees Private now");
  users.let_go_members("friends").await.unwrap();
  assert_ne!(file(), second, "and no longer");
}

#[tokio::test]
async fn a_buddys_presence_moved_while_disconnected_is_told_once_whichever_reply_comes_first() {
  let app = app(CONFIG, &[]).await;
  let (seen, mut received) = tokio::sync::mpsc::unbounded_channel();
  app.state.events.subscribe(
    "test",
    Some(EventType::BuddyPresenceChanged),
    move |record| {
      let seen = seen.clone();
      async move {
        seen.send(record).unwrap();
        Ok(())
      }
    },
  );
  let users = &app.state.users;
  let notify = BuddyChange {
    notify_presence: Some(true),
  };
  for name in ["alice", "bob"] {
    users.set_buddy(name, notify).await.unwrap();
  }
  // a user in a group is known, so a status the server pushes is kept
  users
    .set_groups("alice", vec!["friends".into()])
    .await
    .unwrap();
  users
    .status_received("alice", UserPresence::Online, false)
    .await;
  users.session_ended();
  users
    .status_received("bob", UserPresence::Online, false)
    .await;
  let first = users
    .status_received("alice", UserPresence::Offline, false)
    .await;
  assert_eq!(
    first.previous, None,
    "told of here, not again by the caller"
  );
  let again = users
    .status_received("alice", UserPresence::Offline, false)
    .await;
  assert_eq!(again.previous, Some(UserPresence::Offline));
  // the reply to our own request is taken by the watch before the event reaches the directory
  users.session_ended();
  users
    .status_fetched("alice", UserPresence::Online, false)
    .await;
  let event = users
    .status_received("alice", UserPresence::Online, false)
    .await;
  assert_eq!(event.previous, Some(UserPresence::Online));
  users.presence_moved("alice", UserPresence::Online, UserPresence::Away);
  let mut told = Vec::new();
  for _ in 0..3 {
    let record = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
      .await
      .unwrap()
      .unwrap();
    match &record.event {
      Event::BuddyPresenceChanged(changed) => {
        told.push((changed.username.clone(), changed.previous, changed.presence));
      }
      other => panic!("{other:?}"),
    }
  }
  let alice = |previous, presence| ("alice".to_string(), previous, presence);
  assert_eq!(
    told,
    [
      alice(UserPresence::Online, UserPresence::Offline),
      alice(UserPresence::Offline, UserPresence::Online),
      alice(UserPresence::Online, UserPresence::Away),
    ],
    "bob's presence was not known before, and each of alice's changes is told once"
  );
}

#[tokio::test]
async fn a_buddy_online_across_a_reconnect_is_told_of_no_change() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let (seen, mut received) = tokio::sync::mpsc::unbounded_channel();
  app.state.events.subscribe(
    "test",
    Some(EventType::BuddyPresenceChanged),
    move |record| {
      let seen = seen.clone();
      async move {
        seen.send(record).unwrap();
        Ok(())
      }
    },
  );
  let users = &app.state.users;
  let notify = BuddyChange {
    notify_presence: Some(true),
  };
  users.set_buddy("alice", notify).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![online().1],
    ServerRequest::GetStatus(_) => vec![online().0],
    _ => Vec::new(),
  })
  .await;
  let watched = || async {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
      while !users.is_watched("alice") || alice_cached(&app).await["status"].is_null() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      }
    })
    .await
    .expect("the buddy was never watched");
  };
  server.login(&app).await;
  watched().await;
  server.drop_session(&app).await;
  server.login(&app).await;
  watched().await;
  // a change pushed after the login's watch is the first one told
  server.send(vec![alice_is(soulseek_proto::UserPresence::Away).0]);
  let record = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
    .await
    .unwrap()
    .unwrap();
  let Event::BuddyPresenceChanged(changed) = &record.event else {
    panic!("{:?}", record.event);
  };
  assert_eq!(
    (changed.previous, changed.presence),
    (UserPresence::Online, UserPresence::Away),
    "online before the reconnect and after it is no change"
  );
}

#[tokio::test]
async fn a_status_request_given_up_leaves_the_next_pushed_status_unasked() {
  let app = crate::testing::soulseek::app(CONFIG).await;
  // alice's status is never answered
  let server = crate::testing::soulseek::Server::silent().await;
  server.login(&app).await;
  let users = &app.state.users;
  let client = app.state.network.client();
  let asking = users.status_of(client, "alice");
  let given_up = tokio::time::timeout(std::time::Duration::from_millis(100), asking).await;
  assert!(given_up.is_err());
  // the request ends on its own task once it sees the caller gave up, well before the client's
  // own five-second wait would end it
  tokio::time::timeout(std::time::Duration::from_secs(1), async {
    while !users.tasks.is_empty() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("the request given up never ended");
  let pushed = users
    .status_received("alice", UserPresence::Away, false)
    .await;
  assert!(
    !pushed.asked,
    "taken as the answer to a request given up on"
  );
}

#[tokio::test]
async fn of_two_status_requests_one_dropped_after_its_reply_leaves_the_others_reply_asked() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  // alice's status is answered only when the test says
  let mut server = crate::testing::soulseek::Server::silent().await;
  server.login(&app).await;
  let users = &app.state.users;
  // known to the cache, so the status the follower handles is kept where the test sees it
  users.privileged_listed(&["alice".to_string()]).await;
  let client = app.state.network.client();
  let asked_of_alice = |request: &ServerRequest| {
    matches!(request, ServerRequest::GetStatus(asked) if asked.username == "alice").then_some(())
  };
  let mut first = Box::pin(users.status_of(client, "alice"));
  let mut second = Box::pin(users.status_of(client, "alice"));
  for asking in [&mut first, &mut second] {
    tokio::select! {
      biased;
      _ = asking => panic!("answered before the server was asked"),
      () = server.received(asked_of_alice) => {}
    }
  }
  // the first request's reply comes, and its event is handled before the request is polled
  server.send(vec![alice_is(soulseek_proto::UserPresence::Online).0]);
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while alice_cached(&app).await["status"]["presence"] != "Online" {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the reply's event was never handled");
  drop(first);
  // the second request's reply
  let replied = users
    .status_received("alice", UserPresence::Online, false)
    .await;
  assert!(replied.asked, "taken for a status the server sent unasked");
  drop(second);
}

/// Alice, whose row says she is privileged when `stored`, and of whom nothing was heard in this
/// run, has her status asked, which says the opposite: her summary is pushed with it.
async fn a_status_asked_moves_alice(stored: bool) {
  use soulseek_proto::server::UserStatus;
  use soulseek_proto::{ServerRequest, ServerResponse};

  let app = crate::testing::soulseek::app(CONFIG).await;
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::GetStatus(asked) => vec![ServerResponse::GetStatus(UserStatus {
      username: asked.username.clone(),
      presence: soulseek_proto::UserPresence::Online,
      is_privileged: !stored,
    })],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let users = &app.state.users;
  // as written in an earlier run
  let before = Utc::now() - chrono::Duration::days(1);
  let row = touch::Pending {
    seen_at: Some(before),
    creates: true,
    status: Some((UserPresence::Offline, stored, before)),
    ..Default::default()
  };
  let batch = std::collections::BTreeMap::from([("alice".to_string(), row)]);
  users
    .db
    .write(move |conn| touch::write(conn, &batch, &Default::default()))
    .await
    .unwrap();
  // the rows read at startup are known
  users.known.added("alice");
  let summary = || async { app.call("GET", "/api/v0/users/alice/summary", None).await.1 };
  assert_eq!(summary().await["isPrivileged"], stored);
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=users", None),
    )
    .await;
  stream.until("hello").await;
  users
    .status_of(app.state.network.client(), "alice")
    .await
    .unwrap();
  let pushed = tokio::time::timeout(
    std::time::Duration::from_secs(5),
    stream.until("users.updated"),
  )
  .await
  .expect("alice's summary was not pushed");
  assert_eq!(pushed.last().unwrap().data["isPrivileged"], !stored);
  assert_eq!(summary().await["isPrivileged"], !stored);
}

#[tokio::test]
async fn a_status_asked_that_grants_privileges_pushes_the_users_summary() {
  a_status_asked_moves_alice(false).await;
}

#[tokio::test]
async fn a_status_asked_of_a_user_privileged_in_an_earlier_run_pushes_their_summary() {
  a_status_asked_moves_alice(true).await;
}

/// The next summary `stream` pushes.
async fn next_pushed(stream: &mut crate::testing::Stream) -> Value {
  let pushed = tokio::time::timeout(
    std::time::Duration::from_secs(5),
    stream.until("users.updated"),
  )
  .await
  .expect("no summary was pushed");
  pushed.last().unwrap().data.clone()
}

#[tokio::test]
async fn what_the_server_says_unasked_that_moves_a_users_groups_pushes_their_summary() {
  use soulseek_proto::server::UserStats;

  let app = app(CONFIG, &[]).await;
  let users = &app.state.users;
  users.set_notes("alice", "kind".into()).await.unwrap();
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=users", None),
    )
    .await;
  stream.until("hello").await;
  users.privileged_listed(&["alice".to_string()]).await;
  assert_eq!(next_pushed(&mut stream).await["isPrivileged"], true);
  // alice is not watched
  users
    .status_received("alice", UserPresence::Online, false)
    .await;
  assert_eq!(next_pushed(&mut stream).await["isPrivileged"], false);
  users
    .statistics_received("alice", &UserStats::default())
    .await;
  let groups = next_pushed(&mut stream).await["groups"].clone();
  assert!(
    groups
      .as_array()
      .unwrap()
      .iter()
      .any(|g| g["name"] == "leechers"),
    "{groups}"
  );
  // a presence alone moves no group
  users
    .status_received("alice", UserPresence::Away, false)
    .await;
  users.set_notes("alice", "kinder".into()).await.unwrap();
  assert_eq!(next_pushed(&mut stream).await["notes"], "kinder");
}

#[tokio::test]
async fn a_status_asked_of_a_user_without_a_row_pushes_nothing() {
  use soulseek_proto::server::UserStatus;
  use soulseek_proto::{ServerRequest, ServerResponse};

  let app = crate::testing::soulseek::app(CONFIG).await;
  let server = crate::testing::soulseek::Server::start(|request| match request {
    ServerRequest::GetStatus(asked) => vec![ServerResponse::GetStatus(UserStatus {
      username: asked.username.clone(),
      presence: soulseek_proto::UserPresence::Online,
      is_privileged: true,
    })],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let users = &app.state.users;
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=users", None),
    )
    .await;
  stream.until("hello").await;
  users
    .status_of(app.state.network.client(), "bob")
    .await
    .unwrap();
  users.set_notes("alice", "kind".into()).await.unwrap();
  assert_eq!(
    next_pushed(&mut stream).await["username"],
    "alice",
    "bob has no row to show"
  );
}

#[tokio::test]
async fn two_buddy_changes_made_at_once_both_hold() {
  let app = app(CONFIG, &[]).await;
  let users = &app.state.users;
  let notify = BuddyChange {
    notify_presence: Some(true),
  };
  let (a, b) = tokio::join!(
    users.set_buddy("alice", BuddyChange::default()),
    users.set_buddy("alice", notify)
  );
  a.unwrap();
  b.unwrap();
  let followed = Buddy {
    notify_presence: true,
  };
  assert_eq!(users.buddy("alice"), Some(followed));
  let (_, summary) = app.call("GET", "/api/v0/users/alice/summary", None).await;
  assert_eq!(summary["buddy"], json!({ "notifyPresence": true }));
}

#[tokio::test]
async fn the_blocklist_turned_on_in_the_file_blocks_at_once_and_waits_for_no_restart() {
  let files = tempfile::tempdir().unwrap();
  let list = files.path().join("blocklist");
  std::fs::write(&list, "203.0.113.0/24\n").unwrap();
  let config = |enabled: bool| {
    format!(
      "[blacklist]\nenabled = {enabled}\nfile = {:?}\n",
      list.to_str().unwrap()
    )
  };
  let app = app(&config(false), &[]).await;
  let ip = "203.0.113.7".parse().unwrap();
  let blocked = || {
    let users = &app.state.users;
    users.is_blocked("alice", Some(ip), BlockScope::Uploads)
  };
  assert!(!blocked());
  app.configure_live(&config(true), "blacklist.enabled").await;
  let loaded = tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while !blocked() {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await;
  assert!(loaded.is_ok(), "the blocklist was not loaded");
}

/// The scripted server's word that alice is `presence`, as a status and as the answer to a watch.
fn alice_is(
  presence: soulseek_proto::UserPresence,
) -> (
  soulseek_proto::ServerResponse,
  soulseek_proto::ServerResponse,
) {
  use soulseek_proto::ServerResponse;
  use soulseek_proto::server::{UserStats, UserStatus, WatchUserResponse, WatchedUser};

  let status = ServerResponse::GetStatus(UserStatus {
    username: "alice".into(),
    presence,
    is_privileged: false,
  });
  let watched = ServerResponse::WatchUser(WatchUserResponse {
    username: "alice".into(),
    user: Some(WatchedUser {
      status: presence,
      stats: UserStats::default(),
      country_code: Some(String::new()),
    }),
  });
  (status, watched)
}

/// What `GET /users/alice/profile` answers.
async fn alice(app: &crate::testing::TestApp) -> Value {
  let (status, profile) = app.call("GET", "/api/v0/users/alice/profile", None).await;
  assert_eq!(status, 200);
  profile
}

// on several threads, as the daemon runs, where the status can be handled before the watch ends
#[tokio::test(flavor = "multi_thread")]
async fn a_buddy_watched_at_login_is_shown_with_the_presence_the_server_gives_then() {
  use soulseek_proto::ServerRequest;

  // the status comes behind the answer to the watch, as our own request's answer, or ahead of
  // it; behind, two tasks race for it, so that order is tried several times
  for ahead in std::iter::repeat_n(false, 7).chain([true]) {
    let app = crate::testing::soulseek::app(CONFIG).await;
    let users = &app.state.users;
    users.set_buddy("alice", Default::default()).await.unwrap();
    let online = || alice_is(soulseek_proto::UserPresence::Online);
    let server = crate::testing::soulseek::Server::start(move |request| match request {
      ServerRequest::WatchUser(_) if ahead => vec![online().0, online().1],
      ServerRequest::WatchUser(_) => vec![online().1],
      ServerRequest::GetStatus(_) => vec![online().0],
      _ => Vec::new(),
    })
    .await;
    server.login(&app).await;
    let shown = tokio::time::timeout(std::time::Duration::from_secs(4), async {
      while alice(&app).await["presence"] != json!(UserPresence::Online) {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
      }
    })
    .await;
    assert!(
      shown.is_ok(),
      "shown {} with the status {} the watch's answer",
      alice(&app).await["presence"],
      if ahead { "ahead of" } else { "behind" }
    );
  }
}

#[tokio::test]
async fn forgetting_a_watched_user_unwatches_them_and_a_later_status_makes_no_row() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![online().1],
    ServerRequest::GetStatus(_) => vec![online().0],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let watched = || async { app.call("GET", "/api/v0/users/watched", None).await.1 };
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while watched().await != json!(["alice"]) {
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await
  .expect("the buddy was never watched");

  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  server.send(vec![alice_is(soulseek_proto::UserPresence::Away).0]);
  // longer than the writer gathers what it writes together
  let back = tokio::time::timeout(touch::WINDOW * 2, async {
    while alice(&app).await["known"] != true {
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await;
  assert!(
    back.is_err(),
    "the forgotten user is in the directory again"
  );
  assert_eq!(watched().await, json!([]));
}

#[tokio::test]
async fn a_status_handled_while_a_watched_user_is_forgotten_makes_no_row() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![online().1],
    ServerRequest::GetStatus(_) => vec![online().0],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let watched = || async { app.call("GET", "/api/v0/users/watched", None).await.1 };
  let within = std::time::Duration::from_secs(5);
  let pause = || tokio::time::sleep(std::time::Duration::from_millis(10));
  tokio::time::timeout(within, async {
    while watched().await != json!(["alice"]) {
      pause().await;
    }
  })
  .await
  .expect("the buddy was never watched");

  // the writer is held, so the forget waits at the row's delete
  let (held, holding) = tokio::sync::oneshot::channel();
  let (release, released) = std::sync::mpsc::channel::<()>();
  let db = users.database().clone();
  let hold = tokio::spawn(async move {
    db.write(move |_| {
      held.send(()).expect("the test waits for the hold");
      released.recv().expect("the test releases the hold");
      Ok(())
    })
    .await
  });
  holding.await.unwrap();
  let meanwhile = async {
    let unwatched = tokio::time::timeout(within, async {
      while watched().await != json!([]) {
        pause().await;
      }
    })
    .await;
    server.send(vec![alice_is(soulseek_proto::UserPresence::Away).0]);
    // the follower keeps the status in the cache before it would touch the row
    let taken = tokio::time::timeout(within, async {
      while alice_cached(&app).await["status"]["presence"] != "Away" {
        pause().await;
      }
    })
    .await;
    release.send(()).unwrap();
    (unwatched, taken)
  };
  let ((status, _), (unwatched, taken)) =
    tokio::join!(app.call("DELETE", "/api/v0/users/alice", None), meanwhile);
  hold.await.unwrap().unwrap();
  assert!(unwatched.is_ok(), "still watched while the row was deleted");
  assert!(taken.is_ok(), "the status never reached the cache");
  assert_eq!(status, 204);
  let back = tokio::time::timeout(touch::WINDOW * 2, async {
    while alice(&app).await["known"] != true {
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await;
  assert!(
    back.is_err(),
    "the forgotten user is in the directory again"
  );
  assert_eq!(watched().await, json!([]));
}

/// What `GET /users/cache` holds of alice.
async fn alice_cached(app: &crate::testing::TestApp) -> Value {
  let (_, cache) = app.call("GET", "/api/v0/users/cache", None).await;
  let all = cache.as_array().unwrap();
  all
    .iter()
    .find(|user| user["username"] == "alice")
    .unwrap()
    .clone()
}

#[tokio::test]
async fn while_a_new_username_waits_for_the_reconnect_we_are_the_name_logged_in_as() {
  let config = CONFIG.replace("username = \"me\"", "username = \"renamed\"");
  let app = crate::testing::soulseek::app(&config).await;
  let server = crate::testing::soulseek::Server::silent().await;
  server.login(&app).await;
  assert_eq!(
    app.state.network.client().username().as_deref(),
    Some(crate::testing::soulseek::US)
  );
  let (ours, _) = app
    .call("PUT", "/api/v0/users/me/block", Some(json!({})))
    .await;
  assert_eq!(ours, 409, "ourselves");
}

/// Whether `GET /users/alice/profile` says alice is in the directory at any time within two
/// windows of the touch writer.
async fn alice_comes_back(app: &crate::testing::TestApp) -> bool {
  tokio::time::timeout(touch::WINDOW * 2, async {
    while alice(app).await["known"] != true {
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await
  .is_ok()
}

#[tokio::test]
async fn statistics_asked_before_a_watched_user_is_forgotten_and_answered_after_make_no_row() {
  use soulseek_proto::server::{UserStatistics, UserStats};
  use soulseek_proto::{ServerRequest, ServerResponse};

  let app = crate::testing::soulseek::app(CONFIG).await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  // alice's statistics are answered only when the test says
  let mut server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![online().1],
    ServerRequest::GetStatus(_) => vec![online().0],
    ServerRequest::GetUserStats(asked) if asked.username != "alice" => {
      vec![ServerResponse::GetUserStats(UserStatistics {
        username: asked.username.clone(),
        stats: UserStats::default(),
      })]
    }
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while !users.is_watched("alice") {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the buddy was never watched");

  // a presence pushed on its own has her statistics asked for again
  server.send(vec![alice_is(soulseek_proto::UserPresence::Away).0]);
  server
    .received(|request| {
      matches!(request, ServerRequest::GetUserStats(asked) if asked.username == "alice")
        .then_some(())
    })
    .await;
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  assert_eq!(alice(&app).await["known"], false);
  server.send(vec![ServerResponse::GetUserStats(UserStatistics {
    username: "alice".into(),
    stats: UserStats::default(),
  })]);
  assert!(
    !alice_comes_back(&app).await,
    "the forgotten user is in the directory again"
  );
}

#[tokio::test]
async fn a_profile_refresh_answered_after_the_user_is_forgotten_makes_no_row() {
  use soulseek_proto::server::GetPeerAddressResponse;
  use soulseek_proto::{ServerRequest, ServerResponse};

  let app = crate::testing::soulseek::app(CONFIG).await;
  // alice is offline to peers, so her info fails at once, and her status is answered only when
  // the test says
  let mut server = crate::testing::soulseek::Server::start(|request| match request {
    ServerRequest::GetPeerAddress(asked) => {
      vec![ServerResponse::GetPeerAddress(GetPeerAddressResponse {
        username: asked.username.clone(),
        ip: std::net::Ipv4Addr::UNSPECIFIED,
        port: 0,
        obfuscation: None,
      })]
    }
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  app
    .state
    .users
    .set_notes("alice", "kind".into())
    .await
    .unwrap();
  let refreshing = app.call("GET", "/api/v0/users/alice/profile?refresh=true", None);
  let forgetting = async {
    server
      .received(|request| {
        matches!(request, ServerRequest::GetStatus(asked) if asked.username == "alice")
          .then_some(())
      })
      .await;
    let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
    assert_eq!(status, 204);
    server.send(vec![alice_is(soulseek_proto::UserPresence::Online).0]);
  };
  let ((status, _), ()) = tokio::join!(refreshing, forgetting);
  assert_eq!(status, 200);
  assert_eq!(
    alice(&app).await["known"],
    false,
    "the refresh made the forgotten user's row again"
  );
}

/// Alice, who has a row, has her client asked for its info by `GET path`, and is forgotten
/// before it answers; the answer makes her no row.
async fn info_answered_after_alice_is_forgotten_makes_no_row(path: &str) {
  use soulseek_proto::frame::FrameKind;
  use soulseek_proto::peer::UserInfo;
  use soulseek_proto::server::GetPeerAddressResponse;
  use soulseek_proto::{PeerMessage, ServerRequest, ServerResponse};
  use tokio::io::AsyncWriteExt;

  // alice's client, which the daemon dials for her info
  let peer = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = peer.local_addr().unwrap().port();
  let app = crate::testing::soulseek::app(CONFIG).await;
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::GetPeerAddress(asked) => {
      vec![ServerResponse::GetPeerAddress(GetPeerAddressResponse {
        username: asked.username.clone(),
        ip: std::net::Ipv4Addr::LOCALHOST,
        port: port.into(),
        obfuscation: None,
      })]
    }
    ServerRequest::GetStatus(_) => vec![alice_is(soulseek_proto::UserPresence::Online).0],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  app
    .state
    .users
    .set_notes("alice", "kind".into())
    .await
    .unwrap();
  let asking = app.call("GET", path, None);
  // her info is answered once she is forgotten
  let forgetting = async {
    let (mut alice_client, _) = peer.accept().await.unwrap();
    let mut buffer = Vec::new();
    let next = crate::testing::soulseek::next_frame;
    next(&mut alice_client, &mut buffer, FrameKind::Init).await;
    let asked = next(&mut alice_client, &mut buffer, FrameKind::Peer).await;
    assert_eq!(
      PeerMessage::decode(&asked).unwrap(),
      PeerMessage::InfoRequest
    );
    let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
    assert_eq!(status, 204);
    let info = PeerMessage::InfoResponse(UserInfo {
      description: "hello".into(),
      picture: None,
      upload_slots: 1,
      queue_length: 0,
      has_free_upload_slot: true,
      upload_permitted: None,
    });
    alice_client.write_all(&info.encode()).await.unwrap();
    alice_client
  };
  let ((status, _), _alice_client) = tokio::join!(asking, forgetting);
  assert_eq!(status, 200);
  assert_eq!(
    alice(&app).await["known"],
    false,
    "the info made the forgotten user's row again"
  );
}

#[tokio::test]
async fn a_profile_refresh_whose_info_comes_after_the_user_is_forgotten_makes_no_row() {
  info_answered_after_alice_is_forgotten_makes_no_row("/api/v0/users/alice/profile?refresh=true")
    .await;
}

#[tokio::test]
async fn info_asked_for_that_comes_after_the_user_is_forgotten_makes_no_row() {
  info_answered_after_alice_is_forgotten_makes_no_row("/api/v0/users/alice/info").await;
}

#[tokio::test]
async fn statistics_asked_for_that_come_after_the_user_is_forgotten_make_no_row() {
  let app = crate::testing::soulseek::app(CONFIG).await;
  let server = crate::testing::soulseek::Server::silent().await;
  server.login(&app).await;
  let users = &app.state.users;
  users.set_notes("alice", "kind".into()).await.unwrap();
  // the writer is held, so the forget waits at the row's delete and the touches the answer
  // makes queue behind it
  let (held, holding) = tokio::sync::oneshot::channel();
  let (release, released) = std::sync::mpsc::channel::<()>();
  let db = users.database().clone();
  let hold = tokio::spawn(async move {
    db.write(move |_| {
      held.send(()).expect("the test waits for the hold");
      released.recv().expect("the test releases the hold");
      Ok(())
    })
    .await
  });
  holding.await.unwrap();
  let asking = async {
    let asked = users
      .statistics_of(app.state.network.client(), "alice")
      .await;
    release.send(()).unwrap();
    asked
  };
  // the forget is sent to the writer on its first poll, before the statistics are asked for
  let (forgotten, asked) = tokio::join!(users.remove("alice", false), asking);
  hold.await.unwrap().unwrap();
  forgotten.unwrap();
  asked.unwrap();
  assert!(
    !alice_comes_back(&app).await,
    "the statistics made the forgotten user's row again"
  );
}

#[tokio::test]
async fn a_watch_answered_after_the_user_is_forgotten_watches_and_touches_no_one() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  // the watch the login starts is answered only when the test says
  let mut server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::GetStatus(_) => vec![online().0],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  server
    .received(|request| matches!(request, ServerRequest::WatchUser(_)).then_some(()))
    .await;
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  server.send(vec![online().1]);
  assert!(
    !alice_comes_back(&app).await,
    "the forgotten user is in the directory again"
  );
  assert!(!users.is_watched("alice"));
}

/// The scripted server's answer to a watch of `name`, online.
fn watch_answer(name: &str) -> soulseek_proto::ServerResponse {
  use soulseek_proto::server::{UserStats, WatchUserResponse, WatchedUser};
  soulseek_proto::ServerResponse::WatchUser(WatchUserResponse {
    username: name.into(),
    user: Some(WatchedUser {
      status: soulseek_proto::UserPresence::Online,
      stats: UserStats::default(),
      country_code: Some(String::new()),
    }),
  })
}

#[tokio::test]
async fn a_buddy_forgotten_while_the_login_watches_the_buddies_is_not_watched_or_known() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let users = &app.state.users;
  users.set_buddy("aaron", Default::default()).await.unwrap();
  users.set_buddy("alice", Default::default()).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  // aaron's watch, the first of the login's, is answered only when the test says
  let mut server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(asked) if asked.username == "alice" => vec![online().1],
    ServerRequest::GetStatus(asked) if asked.username == "alice" => vec![online().0],
    ServerRequest::GetStatus(asked) => {
      vec![soulseek_proto::ServerResponse::GetStatus(
        soulseek_proto::server::UserStatus {
          username: asked.username.clone(),
          presence: soulseek_proto::UserPresence::Online,
          is_privileged: false,
        },
      )]
    }
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  server
    .received(|request| {
      matches!(request, ServerRequest::WatchUser(asked) if asked.username == "aaron").then_some(())
    })
    .await;
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  assert_eq!(alice(&app).await["known"], false);
  assert!(users.buddy("alice").is_none());
  server.send(vec![watch_answer("aaron")]);
  assert!(
    !alice_comes_back(&app).await,
    "the forgotten user is in the directory again"
  );
  assert!(!users.is_watched("alice"));
}

/// A daemon whose touches sent so far are all written: the writer takes them in order, so a
/// touch that makes a row of a new name, sent after them and seen in the directory, was written
/// after them.
async fn touches_written(app: &crate::testing::TestApp) {
  static MARKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
  let marker = format!(
    "marker{}",
    MARKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
  );
  app.state.users.touch(&marker, Touch::Transfer).await;
  let path = format!("/api/v0/users/{marker}/profile");
  tokio::time::timeout(touch::WINDOW * 5, async {
    while app.call("GET", &path, None).await.1["known"] != true {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the touch writer never wrote the marker");
}

/// A logged-in daemon whose conversation with alice is open, she watched and known, then
/// forgotten; the server gives her address as `port` on this host, and her presence as away
/// once the returned flag is set.
async fn alice_messaged_then_forgotten(
  port: u16,
) -> (
  crate::testing::TestApp,
  crate::testing::soulseek::Server,
  Arc<std::sync::atomic::AtomicBool>,
) {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let away = Arc::new(std::sync::atomic::AtomicBool::new(false));
  let presence = {
    let away = away.clone();
    move || {
      alice_is(if away.load(std::sync::atomic::Ordering::SeqCst) {
        soulseek_proto::UserPresence::Away
      } else {
        soulseek_proto::UserPresence::Online
      })
    }
  };
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![presence().1],
    ServerRequest::GetStatus(_) => vec![presence().0],
    ServerRequest::GetPeerAddress(asked) => vec![soulseek_proto::ServerResponse::GetPeerAddress(
      soulseek_proto::server::GetPeerAddressResponse {
        username: asked.username.clone(),
        ip: std::net::Ipv4Addr::LOCALHOST,
        port: port.into(),
        obfuscation: None,
      },
    )],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  server.send(vec![message_from_alice(7)]);
  let users = &app.state.users;
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while !users.is_watched("alice") || alice(&app).await["known"] != true {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the peer was never watched and known");
  // the watch's own status and statistics are in the row
  assert!(alice_filled_in(&app, UserPresence::Online).await);
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  assert_eq!(alice(&app).await["known"], false);
  assert!(!users.is_watched("alice"));
  (app, server, away)
}

/// A new private message from alice, `id`.
fn message_from_alice(id: u32) -> soulseek_proto::ServerResponse {
  use soulseek_proto::server::PrivateMessageResponse;
  soulseek_proto::ServerResponse::PrivateMessage(PrivateMessageResponse {
    id,
    timestamp: 1_700_000_000,
    username: "alice".into(),
    message: "hi".into(),
    is_new: true,
  })
}

/// Waits until alice is in the directory with `presence` and statistics.
async fn alice_filled_in(app: &crate::testing::TestApp, presence: UserPresence) -> bool {
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    loop {
      let profile = alice(app).await;
      if profile["known"] == true
        && profile["presence"] == json!(presence)
        && profile["statistics"] != Value::Null
      {
        return;
      }
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await
  .is_ok()
}

#[tokio::test]
async fn a_conversation_peer_forgotten_is_not_known_again_after_the_next_login() {
  let (app, server, _) = alice_messaged_then_forgotten(1).await;
  assert!(!alice_comes_back(&app).await, "back before the reconnect");
  server.drop_session(&app).await;
  server.login(&app).await;
  assert!(
    !alice_comes_back(&app).await,
    "the forgotten user is in the directory again after a reconnect"
  );
}

#[tokio::test]
async fn a_forgotten_user_who_writes_again_is_known_again_with_their_presence() {
  let (app, server, _) = alice_messaged_then_forgotten(1).await;
  server.send(vec![message_from_alice(8)]);
  assert!(alice_filled_in(&app, UserPresence::Online).await);
}

#[tokio::test]
async fn a_forgotten_user_made_a_buddy_again_is_known_again_with_their_presence() {
  let (app, _server, _) = alice_messaged_then_forgotten(1).await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  assert!(alice_filled_in(&app, UserPresence::Online).await);
}

#[tokio::test]
async fn a_forgotten_user_a_download_is_asked_of_is_known_again() {
  use crate::transfers::downloads::{FileRequest, Group};

  // alice's client takes our connection and says nothing
  let peer = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = peer.local_addr().unwrap().port();
  tokio::spawn(async move {
    let mut held = Vec::new();
    while let Ok((stream, _)) = peer.accept().await {
      held.push(stream);
    }
  });
  let (app, _server, _) = alice_messaged_then_forgotten(port).await;
  let group = Group {
    username: "alice".into(),
    files: vec![FileRequest {
      filename: "@@a\\Album\\01.flac".into(),
      size: 1,
      position: None,
    }],
    destination: None,
    folder: None,
    search_id: None,
  };
  let enqueued = app
    .state
    .downloads
    .enqueue(uuid::Uuid::new_v4(), vec![group], true)
    .await;
  assert!(enqueued[0].is_ok(), "{:?}", enqueued[0].as_ref().err());
  assert!(
    alice_comes_back(&app).await,
    "no row for a download from her"
  );
}

/// The daemon of [`alice_messaged_then_forgotten`] after a lost session and a new login, which
/// watches alice again for her open conversation; she is away now, and the watch's statistics
/// and status are written, to no row, before this returns.
async fn alice_forgotten_then_watched_again()
-> (crate::testing::TestApp, crate::testing::soulseek::Server) {
  let (app, server, away) = alice_messaged_then_forgotten(1).await;
  away.store(true, std::sync::atomic::Ordering::SeqCst);
  server.drop_session(&app).await;
  server.login(&app).await;
  let users = &app.state.users;
  // the status the watch asks for is kept here, and its touch sent, in one step of the
  // follower, after the watch answer's statistics were sent
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    let away_now = || {
      let facts = users.groups.facts("alice");
      facts.and_then(|facts| facts.presence) == Some(UserPresence::Away)
    };
    while !users.is_watched("alice") || !away_now() {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the open conversation's peer was not watched again at the login");
  touches_written(&app).await;
  assert_eq!(alice(&app).await["known"], false);
  (app, server)
}

#[tokio::test]
async fn a_forgotten_peer_watched_again_at_login_who_writes_again_is_filled_in() {
  let (app, server) = alice_forgotten_then_watched_again().await;
  server.send(vec![message_from_alice(8)]);
  assert!(
    alice_filled_in(&app, UserPresence::Away).await,
    "{}",
    alice(&app).await
  );
}

#[tokio::test]
async fn a_forgotten_peer_watched_again_at_login_made_a_buddy_is_filled_in() {
  let (app, _server) = alice_forgotten_then_watched_again().await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  assert!(
    alice_filled_in(&app, UserPresence::Away).await,
    "{}",
    alice(&app).await
  );
}

#[tokio::test]
async fn a_user_watched_through_the_api_whose_statistics_came_before_the_row_has_them() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  // the status the watch asks for is answered only when the test says
  let mut server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![online().1],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let answer_later = async {
    // the watch answer's statistics are sent before the status is asked for; once they are
    // written, to no row, the status comes, and the watch's reason for the row after it
    server
      .received(|request| matches!(request, ServerRequest::GetStatus(_)).then_some(()))
      .await;
    touches_written(&app).await;
    assert_eq!(alice(&app).await["known"], false);
    server.send(vec![online().0]);
  };
  let ((status, _), ()) = tokio::join!(
    app.call("PUT", "/api/v0/users/alice/watch", None),
    answer_later
  );
  assert_eq!(status, 204);
  assert!(
    alice_comes_back(&app).await,
    "no row for a watch asked by hand"
  );
  assert_ne!(alice(&app).await["statistics"], Value::Null);
}

#[tokio::test]
async fn a_row_made_for_a_user_nobody_watches_has_no_presence() {
  let app = app(CONFIG, &[]).await;
  let users = &app.state.users;
  // what the server said of her once, as an answer to a question about her
  users
    .status_fetched("alice", UserPresence::Online, false)
    .await;
  users
    .change("alice", Change::Notes("met in #indie".into()))
    .await
    .unwrap();
  let profile = alice(&app).await;
  assert_eq!(profile["known"], true);
  assert_eq!(profile["presence"], Value::Null, "{profile}");
}

#[tokio::test]
async fn a_buddy_forgotten_online_and_noted_in_a_later_session_has_no_presence() {
  use soulseek_proto::ServerRequest;

  let app = crate::testing::soulseek::app(CONFIG).await;
  let users = &app.state.users;
  users.set_buddy("alice", Default::default()).await.unwrap();
  let online = || alice_is(soulseek_proto::UserPresence::Online);
  let server = crate::testing::soulseek::Server::start(move |request| match request {
    ServerRequest::WatchUser(_) => vec![online().1],
    ServerRequest::GetStatus(_) => vec![online().0],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  assert!(alice_filled_in(&app, UserPresence::Online).await);
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  server.drop_session(&app).await;
  server.login(&app).await;
  assert!(!users.is_watched("alice"));
  users
    .change("alice", Change::Notes("was online once".into()))
    .await
    .unwrap();
  let profile = alice(&app).await;
  assert_eq!(profile["known"], true);
  assert_eq!(profile["presence"], Value::Null, "{profile}");
}

/// A daemon where alice has a row, and nothing else.
async fn alice_in_the_directory() -> crate::testing::TestApp {
  let app = app(CONFIG, &[]).await;
  app
    .state
    .users
    .change("alice", Change::Notes("met in #indie".into()))
    .await
    .unwrap();
  app
}

#[tokio::test]
async fn a_message_touch_queued_before_a_forget_is_dropped_whatever_comes_after_it() {
  let app = alice_in_the_directory().await;
  let users = &app.state.users;
  users.touch("alice", Touch::MessageIn).await;
  let (status, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!(status, 204);
  // statistics asked for before the forget and answered after it
  let statistics = api_types::state::UserStatistics {
    average_speed: 1,
    directory_count: 1,
    file_count: 1,
    upload_count: 1,
  };
  users.touch("alice", Touch::Statistics(statistics)).await;
  assert!(
    !alice_comes_back(&app).await,
    "the forgotten user is in the directory again"
  );
}

#[tokio::test]
async fn a_second_forget_that_finds_no_row_leaves_the_first_one_standing() {
  let app = alice_in_the_directory().await;
  app.state.users.touch("alice", Touch::MessageIn).await;
  let (first, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  let (second, _) = app.call("DELETE", "/api/v0/users/alice", None).await;
  assert_eq!((first, second), (204, 404));
  assert!(
    !alice_comes_back(&app).await,
    "the forgotten user is in the directory again"
  );
}
