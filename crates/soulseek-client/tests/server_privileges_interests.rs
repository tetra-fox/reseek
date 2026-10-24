//! Privileges and interests on soulfind: the privileged list pushed at login, our own remaining
//! privileges, another user's flag, interests, recommendations and similar users, and the
//! password change echo.

mod support;

use soulseek_client::{Event, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{Secret, ServerCode, ServerRequest};
use support::options;
use support::soulfind::{self, User};

fn interest_key(code: ServerCode, interest: &str) -> WaitKey {
  WaitKey::ServerInterest {
    code,
    interest: interest.into(),
  }
}

/// `user` likes `liked` and hates `hated`, and the server has recorded both
async fn interests(user: &User, liked: &str, hated: &str) {
  user
    .send(ServerRequest::LikedInterestAdd(LikedInterestAdd {
      interest: liked.into(),
    }))
    .await;
  user
    .send(ServerRequest::HatedInterestAdd(HatedInterestAdd {
      interest: hated.into(),
    }))
    .await;
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
  let seconds: u32 = alice
    .ask(
      WaitKey::Server(ServerCode::CheckPrivileges),
      ServerRequest::CheckPrivileges,
    )
    .await;
  assert_eq!(seconds, 0);
  let privileged: bool = alice
    .ask(
      support::user_key(ServerCode::UserPrivileges, &bob.username),
      ServerRequest::UserPrivileges(UserPrivilegesRequest {
        username: bob.username.clone(),
      }),
    )
    .await;
  assert!(!privileged);
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

  let got: UserInterests = bob
    .ask(
      support::user_key(ServerCode::GetUserInterests, &alice.username),
      ServerRequest::GetUserInterests(GetUserInterestsRequest {
        username: alice.username.clone(),
      }),
    )
    .await;
  assert_eq!(got.likes, std::slice::from_ref(&liked));
  assert_eq!(got.hates, std::slice::from_ref(&hated));

  let global: Recommendations = alice
    .ask(
      WaitKey::Server(ServerCode::GetGlobalRecommendations),
      ServerRequest::GetGlobalRecommendations,
    )
    .await;
  assert!(
    global
      .recommendations
      .iter()
      .any(|recommendation| recommendation.interest == liked && recommendation.count == 2),
    "{global:?}"
  );
  let _: Recommendations = alice
    .ask(
      WaitKey::Server(ServerCode::GetPersonalRecommendations),
      ServerRequest::GetPersonalRecommendations,
    )
    .await;

  let similar: GetPersonalSimilarUsersResponse = alice
    .ask(
      WaitKey::Server(ServerCode::GetPersonalSimilarUsers),
      ServerRequest::GetPersonalSimilarUsers,
    )
    .await;
  assert!(
    similar
      .users
      .iter()
      .any(|user| user.username == bob.username),
    "{similar:?}"
  );
  let by_interest: GetInterestSimilarUsersResponse = alice
    .ask(
      interest_key(ServerCode::GetInterestSimilarUsers, &liked),
      ServerRequest::GetInterestSimilarUsers(GetInterestSimilarUsersRequest {
        interest: liked.clone(),
      }),
    )
    .await;
  assert_eq!(by_interest.interest, liked);
  assert!(by_interest.usernames.contains(&bob.username));
  let related: GetInterestRecommendationsResponse = alice
    .ask(
      interest_key(ServerCode::GetInterestRecommendations, &liked),
      ServerRequest::GetInterestRecommendations(GetInterestRecommendationsRequest {
        interest: liked.clone(),
      }),
    )
    .await;
  assert_eq!(related.interest, liked);
}

#[tokio::test]
async fn a_password_change_is_echoed() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let echo: Secret = alice
    .ask(
      WaitKey::Server(ServerCode::NewPassword),
      ServerRequest::NewPassword(NewPassword {
        new_password: Secret::new("n3w password"),
      }),
    )
    .await;
  assert_eq!(echo.expose(), "n3w password");
}
