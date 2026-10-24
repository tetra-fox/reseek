//! A Soulseek client. So far: the error every operation returns, and the keyed waits that match
//! replies to requests.

mod error;
mod waiter;

pub use error::{
  ClientError, ConnectionErrorKind, ErrorSource, MessageErrorKind, RoomErrorKind,
  UserEndPointErrorKind,
};
pub use waiter::{Scope, WaitKey, Waiter};
