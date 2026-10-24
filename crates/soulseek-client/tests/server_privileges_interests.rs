//! Server messages about privileges and interests, delivered by a fake server to a logged-in
//! client: privilege notifications and replies, recommendations and similar users.

mod support;

use soulseek_client::{Event, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{ServerCode, ServerRequest, ServerResponse};
use support::{Session, user_key};
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn privileged_users_are_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivilegedUsers(PrivilegedUsers {
      usernames: vec!["bob".into(), "carol".into()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivilegedUserListReceived(users) => Some(users),
      _ => None,
    })
    .await;
  assert_eq!(raised, ["bob", "carol"]);
}

#[tokio::test]
async fn add_privileged_user_is_a_notification_without_acknowledgement() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::AddPrivilegedUser(AddPrivilegedUser {
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivilegeNotificationReceived {
        username,
        id,
        requires_acknowledgement,
      } => Some((username, id, requires_acknowledgement)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("bob".into(), None, false));
}

#[tokio::test]
async fn check_privileges_completes_with_seconds() {
  let mut session = Session::start().await;
  let got: u32 = session
    .reply(
      WaitKey::Server(ServerCode::CheckPrivileges),
      ServerResponse::CheckPrivileges(CheckPrivilegesResponse {
        seconds_remaining: 86_400,
      }),
    )
    .await;
  assert_eq!(got, 86_400);
}

#[tokio::test]
async fn user_privileges_completes_with_the_flag() {
  let mut session = Session::start().await;
  let got: bool = session
    .reply(
      user_key(ServerCode::UserPrivileges, "bob"),
      ServerResponse::UserPrivileges(UserPrivilegesResponse {
        username: "bob".into(),
        privileged: true,
      }),
    )
    .await;
  assert!(got);
}

#[tokio::test]
async fn notify_privileges_is_raised_and_acknowledged() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::NotifyPrivileges(NotifyPrivileges {
      id: 9,
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivilegeNotificationReceived {
        username,
        id,
        requires_acknowledgement,
      } => Some((username, id, requires_acknowledgement)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("bob".into(), Some(9), true));
  assert_eq!(
    session.server.request().await,
    ServerRequest::AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges { id: 9 })
  );
}

fn recommendations(unrecommended: bool) -> Recommendations {
  Recommendations {
    recommendations: vec![Recommendation {
      interest: "jazz".into(),
      count: 4,
    }],
    unrecommendations: if unrecommended {
      vec![Recommendation {
        interest: "noise".into(),
        count: -2,
      }]
    } else {
      Vec::new()
    },
  }
}

/// a recommendations frame without the optional second block
fn recommendations_without_second_block(code: u32) -> Vec<u8> {
  let mut payload = code.to_le_bytes().to_vec();
  payload.extend(1u32.to_le_bytes());
  payload.extend(4u32.to_le_bytes());
  payload.extend(b"jazz");
  payload.extend(4i32.to_le_bytes());
  let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
  frame.extend(payload);
  frame
}

#[tokio::test]
async fn personal_and_global_recommendations_with_and_without_the_second_block() {
  let mut session = Session::start().await;
  let got: Recommendations = session
    .reply(
      WaitKey::Server(ServerCode::GetPersonalRecommendations),
      ServerResponse::GetPersonalRecommendations(recommendations(true)),
    )
    .await;
  assert_eq!(got, recommendations(true));
  let got: Recommendations = session
    .reply(
      WaitKey::Server(ServerCode::GetGlobalRecommendations),
      ServerResponse::GetGlobalRecommendations(recommendations(true)),
    )
    .await;
  assert_eq!(got, recommendations(true));

  let wait = session.client.waiter().wait::<Recommendations>(
    WaitKey::Server(ServerCode::GetGlobalRecommendations),
    None,
    CancellationToken::new(),
  );
  session
    .server
    .send_bytes(&recommendations_without_second_block(56))
    .await;
  assert_eq!(wait.await.unwrap(), recommendations(false));
}

#[tokio::test]
async fn get_user_interests_completes_for_the_user() {
  let mut session = Session::start().await;
  let interests = UserInterests {
    username: "bob".into(),
    likes: vec!["jazz".into()],
    hates: vec!["noise".into()],
  };
  let got: UserInterests = session
    .reply(
      user_key(ServerCode::GetUserInterests, "bob"),
      ServerResponse::GetUserInterests(interests.clone()),
    )
    .await;
  assert_eq!(got, interests);
}

#[tokio::test]
async fn similar_users_complete() {
  let mut session = Session::start().await;
  let users = GetPersonalSimilarUsersResponse {
    users: vec![SimilarUser {
      username: "bob".into(),
      rating: 3,
    }],
  };
  let got: GetPersonalSimilarUsersResponse = session
    .reply(
      WaitKey::Server(ServerCode::GetPersonalSimilarUsers),
      ServerResponse::GetPersonalSimilarUsers(users.clone()),
    )
    .await;
  assert_eq!(got, users);
}

#[tokio::test]
async fn interest_recommendations_and_similar_users_complete_for_the_interest() {
  let mut session = Session::start().await;
  let interest_key = |code| WaitKey::ServerInterest {
    code,
    interest: "jazz".into(),
  };
  let recommended = GetInterestRecommendationsResponse {
    interest: "jazz".into(),
    recommendations: recommendations(true),
  };
  let got: GetInterestRecommendationsResponse = session
    .reply(
      interest_key(ServerCode::GetInterestRecommendations),
      ServerResponse::GetInterestRecommendations(recommended.clone()),
    )
    .await;
  assert_eq!(got, recommended);
  let similar = GetInterestSimilarUsersResponse {
    interest: "jazz".into(),
    usernames: vec!["bob".into()],
  };
  let got: GetInterestSimilarUsersResponse = session
    .reply(
      interest_key(ServerCode::GetInterestSimilarUsers),
      ServerResponse::GetInterestSimilarUsers(similar.clone()),
    )
    .await;
  assert_eq!(got, similar);
}
