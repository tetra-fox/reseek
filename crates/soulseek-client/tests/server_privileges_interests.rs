//! Privileges and interests on soulfind: the privileged list pushed at login, our own remaining
//! privileges, another user's flag, interests, recommendations, similar users,
//! and changing the password.

mod support;

use std::time::Duration;

use soulseek_client::{Client, ClientError, Event};
use support::options;
use support::soulfind::{self, PASSWORD, User};

/// `user` likes `liked` and hates `hated`, and the server has recorded both
async fn interests(user: &User, liked: &str, hated: &str) {
  user.client.add_liked_interest(liked).await.unwrap();
  user.client.add_hated_interest(hated).await.unwrap();
  user.settle().await;
}

#[tokio::test]
async fn the_privileged_list_is_pushed_at_login() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut alice = server.login("alice", options()).await;
  alice
    .event(|event| matches!(event, Event::PrivilegedUserListReceived(_)).then_some(()))
    .await;
}

#[tokio::test]
async fn privileges_of_a_new_account_and_of_another_user() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let bob = server.login("bob", options()).await;
  assert_eq!(alice.client.get_privileges().await.unwrap(), Duration::ZERO);
  assert!(
    !alice
      .client
      .get_user_privileged(&bob.username)
      .await
      .unwrap()
  );
  // nothing to give, which the server reports later, if at all, as a private message
  alice
    .client
    .grant_user_privileges(&bob.username, 1)
    .await
    .unwrap();
  assert!(matches!(
    alice.client.grant_user_privileges(&bob.username, 0).await,
    Err(ClientError::InvalidArgument(message)) if message == "The number of days granted must be greater than zero"
  ));
}

#[tokio::test]
async fn interests_recommendations_and_similar_users() {
  let Some(server) = soulfind::server() else {
    return;
  };
  // interests are server-wide, so each run uses its own
  let liked = soulfind::username("jazz");
  let hated = soulfind::username("noise");
  let alice = server.login("alice", options()).await;
  let bob = server.login("bob", options()).await;
  interests(&alice, &liked, &hated).await;
  interests(&bob, &liked, &soulfind::username("polka")).await;

  let got = bob
    .client
    .get_user_interests(&alice.username)
    .await
    .unwrap();
  assert_eq!(got.likes, std::slice::from_ref(&liked));
  assert_eq!(got.hates, std::slice::from_ref(&hated));
  assert_eq!(alice.client.get_interests().await.unwrap(), got);

  let global = alice.client.get_global_recommendations().await.unwrap();
  assert!(
    global
      .recommendations
      .iter()
      .any(|recommendation| recommendation.interest == liked && recommendation.count == 2),
    "{global:?}"
  );
  alice.client.get_personal_recommendations().await.unwrap();

  let similar = alice.client.get_similar_users().await.unwrap();
  assert!(
    similar.iter().any(|user| user.username == bob.username),
    "{similar:?}"
  );
  let by_interest = alice
    .client
    .get_interest_similar_users(&liked)
    .await
    .unwrap();
  assert_eq!(by_interest.interest, liked);
  assert!(by_interest.usernames.contains(&bob.username));
  let related = alice
    .client
    .get_interest_recommendations(&liked)
    .await
    .unwrap();
  assert_eq!(related.interest, liked);

  alice.client.remove_liked_interest(&liked).await.unwrap();
  alice.client.remove_hated_interest(&hated).await.unwrap();
  alice.settle().await;
  let emptied = alice.client.get_interests().await.unwrap();
  assert!(
    emptied.likes.is_empty() && emptied.hates.is_empty(),
    "{emptied:?}"
  );
}

#[tokio::test]
async fn a_changed_password_is_the_one_the_next_login_needs() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  alice.client.change_password("n3w password").await.unwrap();
  alice.client.disconnect(None, None).await;
  let again = Client::new(options()).unwrap();
  assert!(matches!(
    server.connect(&again, &alice.username, PASSWORD).await,
    Err(ClientError::LoginRejected(_))
  ));
  server
    .connect(&again, &alice.username, "n3w password")
    .await
    .unwrap();
}
