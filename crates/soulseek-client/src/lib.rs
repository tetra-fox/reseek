//! A Soulseek client: the server session (connect, log in, stay connected, disconnect), the
//! connections underneath it (direct or through a SOCKS5 proxy, framed, with inactivity timeouts
//! and bounded write queues), the listener that accepts peers, and the keyed waits that match
//! replies to requests.
//!
//! [`Client`] is the entry point. It reports what happens as [`Event`]s on a broadcast channel,
//! and hands server messages and accepted connections that belong to the peer, distributed and
//! search components to the receivers in [`Handoffs`].

mod cache;
mod client;
mod conn;
mod error;
mod events;
mod inbound;
mod limit;
mod listener;
mod log;
mod options;
mod patch;
mod resolvers;
mod state;
mod token;
mod waiter;

use std::sync::{Mutex, MutexGuard, PoisonError};

pub use client::{Client, DEFAULT_SERVER_ADDRESS, DEFAULT_SERVER_PORT, Handoffs, ServerHandoff};
pub use conn::{
  Conn, ConnHandle, ConnKey, ConnKind, ConnReadHalf, ConnState, ConnWriteHalf, Disconnect,
  EncodedFrame, MessageConn, ReadEvent,
};
pub use error::{
  ClientError, ConnectionErrorKind, ErrorSource, MessageErrorKind, RoomErrorKind,
  UserEndPointErrorKind,
};
pub use events::{
  DistributedParent, EVENT_CAPACITY, Event, PrivateMessage, RoomMembers, SearchDelivery,
};
pub use inbound::InboundConnection;
pub use options::{
  ClientLimits, ClientOptions, ConnectionOptions, KeepAlive, ProxyCredentials, ProxyOptions,
};
pub use patch::ClientOptionsPatch;
pub use resolvers::{
  BrowseResponseResolver, CachedSearchResponse, DirectoryContentsResolver, EnqueueDownload,
  PlaceInQueueResolver, ResolverFuture, Resolvers, SearchResponseCache, SearchResponseResolver,
  UserEndPointCache, UserInfoResolver,
};
pub use state::{ClientState, DistributedNetworkInfo, ServerInfo};
pub use waiter::{Scope, WaitKey, Waiter};

/// locks a mutex whose critical sections cannot leave its data half-updated
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
