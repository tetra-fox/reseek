//! Private rooms on soulfind: creating one, the invitation setting, and members and operators
//! added and removed as the owner and the member each see it.

mod support;

use soulseek_client::{Event, RoomMembers, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{RoomType, ServerCode, ServerRequest};
use support::soulfind::{self, User};
use support::{options, room_key, room_user_key};

async fn join(user: &User, room: &str, room_type: RoomType) -> RoomData {
  user
    .ask(
      room_key(ServerCode::JoinRoom, room),
      ServerRequest::JoinRoom(JoinRoomRequest {
        room: room.into(),
        room_type,
      }),
    )
    .await
}

/// waits for the room-and-user event `pick` names for `room`
async fn room_user(
  user: &mut User,
  mut pick: impl FnMut(&Event) -> Option<(&String, &String)>,
) -> (String, String) {
  user
    .event(|event| pick(&event).map(|(room, username)| (room.clone(), username.clone())))
    .await
}

#[tokio::test]
async fn creating_a_private_room_lists_its_owner_and_empty_lists() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("private");
  let mut alice = server.login("owner", options()).await;
  let data = join(&alice, &room, RoomType::Private).await;
  let private = data.private.expect("the room is private");
  assert_eq!(private.owner, alice.username);
  assert!(private.operators.is_empty());
  let members = alice
    .event(|event| match event {
      Event::PrivateRoomUserListReceived(members) => Some(members),
      _ => None,
    })
    .await;
  assert_eq!(
    members,
    RoomMembers {
      room_name: room.clone(),
      users: Vec::new()
    }
  );
  let operators = alice
    .event(|event| match event {
      Event::PrivateRoomModeratedUserListReceived(operators) => Some(operators),
      _ => None,
    })
    .await;
  assert_eq!(
    operators,
    RoomMembers {
      room_name: room,
      users: Vec::new()
    }
  );
}

#[tokio::test]
async fn members_and_operators_are_added_and_removed_as_the_room_sees_it() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("private");
  let mut alice = server.login("owner", options()).await;
  let mut bob = server.login("member", options()).await;
  join(&alice, &room, RoomType::Private).await;
  let accepting: bool = bob
    .ask(
      WaitKey::Server(ServerCode::PrivateRoomToggle),
      ServerRequest::PrivateRoomToggle(PrivateRoomToggle {
        accept_invitations: true,
      }),
    )
    .await;
  assert!(accepting);

  let () = alice
    .ask(
      room_user_key(ServerCode::PrivateRoomAddUser, &room, &bob.username),
      ServerRequest::PrivateRoomAddUser(PrivateRoomAddUser {
        room: room.clone(),
        username: bob.username.clone(),
      }),
    )
    .await;
  let expected = (room.clone(), bob.username.clone());
  for user in [&mut alice, &mut bob] {
    let added = room_user(user, |event| match event {
      Event::PrivateRoomUserAdded {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
    assert_eq!(added, expected);
  }
  bob
    .event(|event| {
      matches!(event, Event::PrivateRoomMembershipAdded(added) if added == room).then_some(())
    })
    .await;

  let () = alice
    .ask(
      room_user_key(ServerCode::PrivateRoomAddOperator, &room, &bob.username),
      ServerRequest::PrivateRoomAddOperator(PrivateRoomAddOperator {
        room: room.clone(),
        username: bob.username.clone(),
      }),
    )
    .await;
  let added = room_user(&mut bob, |event| match event {
    Event::PrivateRoomOperatorAdded {
      room_name,
      username,
    } => Some((room_name, username)),
    _ => None,
  })
  .await;
  assert_eq!(added, expected);
  bob
    .event(|event| {
      matches!(event, Event::PrivateRoomModerationAdded(added) if added == room).then_some(())
    })
    .await;
  let operators = bob
    .event(|event| match event {
      Event::PrivateRoomModeratedUserListReceived(operators) if !operators.users.is_empty() => {
        Some(operators)
      }
      _ => None,
    })
    .await;
  assert_eq!(operators.users, [bob.username.clone()]);

  let data = join(&bob, &room, RoomType::Public).await;
  let private = data.private.expect("the room is private");
  assert_eq!(private.owner, alice.username);
  assert_eq!(private.operators, [bob.username.clone()]);

  let () = alice
    .ask(
      room_user_key(ServerCode::PrivateRoomRemoveOperator, &room, &bob.username),
      ServerRequest::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator {
        room: room.clone(),
        username: bob.username.clone(),
      }),
    )
    .await;
  let removed = room_user(&mut bob, |event| match event {
    Event::PrivateRoomOperatorRemoved {
      room_name,
      username,
    } => Some((room_name, username)),
    _ => None,
  })
  .await;
  assert_eq!(removed, expected);
  bob
    .event(|event| {
      matches!(event, Event::PrivateRoomModerationRemoved(removed) if removed == room).then_some(())
    })
    .await;

  let () = alice
    .ask(
      room_user_key(ServerCode::PrivateRoomRemoveUser, &room, &bob.username),
      ServerRequest::PrivateRoomRemoveUser(PrivateRoomRemoveUser {
        room: room.clone(),
        username: bob.username.clone(),
      }),
    )
    .await;
  let removed = room_user(&mut alice, |event| match event {
    Event::PrivateRoomUserRemoved {
      room_name,
      username,
    } => Some((room_name, username)),
    _ => None,
  })
  .await;
  assert_eq!(removed, expected);
  bob
    .event(|event| {
      matches!(event, Event::PrivateRoomMembershipRemoved(removed) if removed == room).then_some(())
    })
    .await;
}
