//! The engines behind the API, one trait per domain. A handler checks its request, calls its
//! domain's service and turns the result into a response; what a call does is the service's
//! business.
//!
//! Every method has a body that answers `503 soulseek_unavailable` with `"<domain> not
//! implemented yet"`, so [`Unavailable`] implements every trait without a line of its own and
//! an engine overrides the methods it does. [`Services::unavailable`] installs it for every
//! domain; an engine replaces its domain's entry.

mod analysis;
mod application;
mod blocking;
mod events;
mod files;
mod messaging;
mod relay;
mod reports;
mod searches;
mod shares;
mod transfers;
mod users;

use std::sync::Arc;

pub use analysis::{AnalysisService, Piece};
pub use application::{ApplicationService, Connected};
pub use blocking::{BlockImport, BlockingService};
pub use events::EventService;
pub use files::{ContentRequest, FileService, FileTarget};
pub use messaging::{Joined, MessagingService};
pub use relay::{AgentCredential, RelayService};
pub use reports::ReportService;
pub use searches::{Canceled, SearchService};
pub use shares::ShareService;
pub use transfers::TransferService;
pub use users::UserService;

use crate::api::error::ApiError;

/// The service of every domain.
#[derive(Clone)]
pub struct Services {
  /// connecting to the Soulseek server, checking for a newer release, and the database files
  pub application: Arc<dyn ApplicationService>,
  /// searches we start
  pub searches: Arc<dyn SearchService>,
  /// downloads and uploads
  pub transfers: Arc<dyn TransferService>,
  /// other users on the network and the directory of users we know
  pub users: Arc<dyn UserService>,
  /// blocked users
  pub blocking: Arc<dyn BlockingService>,
  /// private conversations and rooms
  pub messaging: Arc<dyn MessagingService>,
  /// what we share
  pub shares: Arc<dyn ShareService>,
  /// the files under the downloads, incomplete and log directories
  pub files: Arc<dyn FileService>,
  /// audio analysis of downloaded files
  pub analysis: Arc<dyn AnalysisService>,
  /// relay controller and agent
  pub relay: Arc<dyn RelayService>,
  /// the event log
  pub events: Arc<dyn EventService>,
  /// metrics and the reports over the transfer history
  pub reports: Arc<dyn ReportService>,
}

impl Services {
  /// Every domain answered by [`Unavailable`].
  pub fn unavailable() -> Self {
    let stub = Arc::new(Unavailable);
    Self {
      application: stub.clone(),
      searches: stub.clone(),
      transfers: stub.clone(),
      users: stub.clone(),
      blocking: stub.clone(),
      messaging: stub.clone(),
      shares: stub.clone(),
      files: stub.clone(),
      analysis: stub.clone(),
      relay: stub.clone(),
      events: stub.clone(),
      reports: stub,
    }
  }
}

/// The service of a domain that is not built yet: every call answers 503.
pub struct Unavailable;

impl ApplicationService for Unavailable {}
impl SearchService for Unavailable {}
impl TransferService for Unavailable {}
impl UserService for Unavailable {}
impl BlockingService for Unavailable {}
impl MessagingService for Unavailable {}
impl ShareService for Unavailable {}
impl FileService for Unavailable {}
impl AnalysisService for Unavailable {}
impl RelayService for Unavailable {}
impl EventService for Unavailable {}
impl ReportService for Unavailable {}

/// The answer of a call `domain` has no engine for yet.
fn not_yet(domain: &str) -> ApiError {
  ApiError::unavailable(Some(format!("{domain} is not implemented")))
}
