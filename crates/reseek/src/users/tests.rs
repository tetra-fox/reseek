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
    .write(move |conn| touch::write(conn, &batch))
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
    .store_info("bob", sent(b"GIF89a, too large"))
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
    .store_info("alice", sent(b"GIF89a"))
    .await;
  assert_eq!(
    profile_info(&unkept, "alice").await,
    (json!(true), Value::Null),
    "pictures are not kept"
  );
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
  users.status_received("alice", UserPresence::Online, false);
  users.session_ended();
  users.status_received("bob", UserPresence::Online, false);
  let first = users.status_received("alice", UserPresence::Offline, false);
  assert_eq!(
    first.previous, None,
    "told of here, not again by the caller"
  );
  let again = users.status_received("alice", UserPresence::Offline, false);
  assert_eq!(again.previous, Some(UserPresence::Offline));
  // the reply to our own request is taken by the watch before the event reaches the directory
  users.session_ended();
  users.status_fetched("alice", UserPresence::Online, false);
  let event = users.status_received("alice", UserPresence::Online, false);
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
