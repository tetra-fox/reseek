//! Who a request is and how it signs in. This module holds roles and principals as the API shows
//! them; signing in and API tokens have their own modules.

pub mod session;
pub mod tokens;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// What a principal may do. Each role may do everything the one before it may.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
pub enum Role {
  /// read anything, change nothing
  ReadOnly,
  /// also search, download, chat and manage transfers
  ReadWrite,
  /// also change the configuration, groups and other principals
  Administrator,
}

/// How a request's identity was established.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Via {
  /// a session from a local account's password login
  Local,
  /// a session from an OpenID Connect login
  Oidc {
    /// the provider's configured name
    provider: String,
  },
  /// a session from a trusted reverse proxy's forwarded identity
  Proxy,
  /// an API token presented with the request
  #[serde(rename_all = "camelCase")]
  ApiToken {
    /// the token's id
    id: Uuid,
  },
  /// an API key from the configuration presented with the request
  ConfigApiKey {
    /// the key's name in the configuration, `primary` for the single key given by
    /// environment variable or command line
    name: String,
  },
  /// the client's address is in a trusted network, so no credential was needed
  TrustedNetwork {
    /// the configured network the address matched
    cidr: String,
  },
}

/// The identity a request acts as.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
  /// the stable id stored wherever reseek records who did something, such as
  /// `local:admin`, `oidc:authentik:1234`, `token:<id>`, `apikey:<name>` or
  /// `network:192.168.1.0/24`
  #[schema(example = "local:admin")]
  pub subject: String,
  /// the name to show: a username, an OpenID Connect name, a token's name, or
  /// `trusted network`
  #[schema(example = "admin")]
  pub name: String,
  /// what the principal may do
  pub role: Role,
  /// how the identity was established
  pub via: Via,
}

/// A principal's name and role, for places that show who is connected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalSummary {
  /// the name to show
  pub name: String,
  /// what the principal may do
  pub role: Role,
}
