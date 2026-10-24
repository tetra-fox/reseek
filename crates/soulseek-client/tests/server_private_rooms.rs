//! Server messages about private rooms, delivered by a fake server to a logged-in client: member
//! and operator lists and changes, our own membership and moderation, and the invitation setting.

mod support;

use soulseek_client::{Event, RoomMembers, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{ServerCode, ServerResponse};
use support::{Session, room_key, room_user_key};
#[tokio::test]
async fn private_room_member_and_operator_lists_are_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateRoomUsers(PrivateRoomUsers {
      room: "secret".into(),
      usernames: vec!["carol".into()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomUserListReceived(members) => Some(members),
      _ => None,
    })
    .await;
  assert_eq!(
    raised,
    RoomMembers {
      room_name: "secret".into(),
      users: vec!["carol".into()]
    }
  );
  session
    .server
    .send(&ServerResponse::PrivateRoomOwned(PrivateRoomOwned {
      room: "secret".into(),
      operators: vec!["dave".into()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomModeratedUserListReceived(members) => Some(members),
      _ => None,
    })
    .await;
  assert_eq!(
    raised,
    RoomMembers {
      room_name: "secret".into(),
      users: vec!["dave".into()]
    }
  );
}

#[tokio::test]
async fn private_room_member_changes_complete_and_are_raised() {
  let mut session = Session::start().await;
  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomAddUser, "secret", "carol"),
      ServerResponse::PrivateRoomAddUser(PrivateRoomAddUser {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomUserAdded {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));

  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomRemoveUser, "secret", "carol"),
      ServerResponse::PrivateRoomRemoveUser(PrivateRoomRemoveUser {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomUserRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));
}

#[tokio::test]
async fn private_room_operator_changes_complete_and_are_raised() {
  let mut session = Session::start().await;
  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomAddOperator, "secret", "carol"),
      ServerResponse::PrivateRoomAddOperator(PrivateRoomAddOperator {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomOperatorAdded {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));

  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomRemoveOperator, "secret", "carol"),
      ServerResponse::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomOperatorRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));
}

#[tokio::test]
async fn our_private_room_membership_is_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateRoomAdded(PrivateRoomAdded {
      room: "secret".into(),
    }))
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomMembershipAdded(room) if room == "secret").then_some(())
    })
    .await;
  let () = session
    .reply(
      room_key(ServerCode::PrivateRoomRemoved, "secret"),
      ServerResponse::PrivateRoomRemoved(PrivateRoomRemoved {
        room: "secret".into(),
      }),
    )
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomMembershipRemoved(room) if room == "secret").then_some(())
    })
    .await;
}

#[tokio::test]
async fn our_private_room_moderation_is_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateRoomOperatorAdded(
      PrivateRoomOperatorAdded {
        room: "secret".into(),
      },
    ))
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomModerationAdded(room) if room == "secret").then_some(())
    })
    .await;
  let () = session
    .reply(
      room_key(ServerCode::PrivateRoomOperatorRemoved, "secret"),
      ServerResponse::PrivateRoomOperatorRemoved(PrivateRoomOperatorRemoved {
        room: "secret".into(),
      }),
    )
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomModerationRemoved(room) if room == "secret").then_some(())
    })
    .await;
}

#[tokio::test]
async fn private_room_toggle_completes_with_the_setting() {
  let mut session = Session::start().await;
  let got: bool = session
    .reply(
      WaitKey::Server(ServerCode::PrivateRoomToggle),
      ServerResponse::PrivateRoomToggle(PrivateRoomToggle {
        accept_invitations: true,
      }),
    )
    .await;
  assert!(got);
}
