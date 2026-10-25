//! A Soulseek client: the server session (connect, log in, stay connected, disconnect), the
//! operations that ask the server for something, connections to other users (directly or
//! through the server's relay, cached one per user for messages), downloads and uploads,
//! searches, browsing users and fetching their info, the distributed search tree, answering
//! other users' searches, the listener that accepts peers, and the keyed waits that match
//! replies to requests.
//!
//! [`Client`] is the entry point. It reports what happens as [`Event`]s on a broadcast channel
//! and answers peers through the application's [`Resolvers`].

mod browse;
mod bucket;
mod cache;
mod client;
mod conn;
mod distributed;
mod error;
mod events;
mod inbound;
mod limit;
mod listener;
mod log;
mod options;
mod patch;
mod peers;
mod resolvers;
mod responder;
mod search;
mod state;
mod token;
mod transfers;
mod user_info;
mod waiter;

use std::sync::{Mutex, MutexGuard, PoisonError};

pub use browse::{BrowseOptions, BrowseProgress, BrowseProgressUpdated};
pub use client::{Client, DEFAULT_SERVER_ADDRESS, DEFAULT_SERVER_PORT};
pub use conn::{
  Conn, ConnHandle, ConnKey, ConnKind, ConnReadHalf, ConnState, ConnWriteHalf, Disconnect,
  EncodedFrame, MessageConn, ReadEvent,
};
pub use error::{ClientError, ConnectionErrorKind, ErrorSource, MessageErrorKind, RoomErrorKind};
pub use events::{
  DistributedParent, EVENT_CAPACITY, Event, PrivateMessage, RoomMembers, SearchDelivery,
};
pub use log::LogLimiter;
pub use options::{
  ClientLimits, ClientOptions, ConnectionOptions, KeepAlive, ProxyCredentials, ProxyOptions,
};
pub use patch::ClientOptionsPatch;
pub use peers::IncomingBrowse;
pub use resolvers::{
  BrowseReply, BrowseResponseResolver, DirectoryContentsResolver, EnqueueDownload,
  PlaceInQueueResolver, ResolverFuture, Resolvers, SearchReply, SearchResponseResolver,
  UserInfoResolver,
};
pub use search::{
  FileFilter, ResponseFilter, Search, SearchOptions, SearchResponseReceived, SearchScope,
  SearchState, SearchStateChanged,
};
pub use state::{ClientState, DistributedNetworkInfo, ServerInfo};
pub use token::MAX_TOKEN;
pub use transfers::{
  DownloadOutput, DownloadRequest, DownloadSink, Governor, InputFactory, OutputFactory,
  ProgressUpdated, Reporter, RequestStyle, SizeMismatchResolver, SlotAwaiter, SlotReleased,
  StateChanged, Transfer, TransferFuture, TransferOptions, TransferState, TransferTask,
  UploadInput, UploadRequest, UploadSource,
};
pub use waiter::{Scope, WaitKey, Waiter};

/// locks a mutex whose critical sections cannot leave its data half-updated
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
