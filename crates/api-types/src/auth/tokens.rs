//! API tokens for scripts, and the config API keys listed beside them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::Role;

/// An API token, without its secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Token {
  /// the token's id
  pub id: Uuid,
  /// the name its owner gave it
  #[schema(example = "backup script")]
  pub name: String,
  /// the first characters of the secret after `slsk_`, to tell tokens apart
  #[schema(example = "a1B2c3D4")]
  pub prefix: String,
  /// what a request with the token may do
  pub role: Role,
  /// the subject of the principal that created it, such as `local:admin`
  #[schema(example = "local:admin")]
  pub owner: String,
  /// the networks the token is accepted from, comma-separated; null for anywhere
  #[schema(example = "192.168.1.0/24")]
  pub cidr: Option<String>,
  /// when it was created
  pub created_at: DateTime<Utc>,
  /// when it stops being accepted, null for never
  pub expires_at: Option<DateTime<Utc>>,
  /// when it was last used, to within a minute; null when never
  pub last_used_at: Option<DateTime<Utc>>,
  /// the client address it was last used from, null when never
  pub last_used_ip: Option<String>,
}

/// An API key from the configuration, listed read-only beside the tokens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigApiKey {
  /// the key's name in the configuration, `primary` for the single key given by environment
  /// variable or command line
  pub name: String,
  /// what a request with the key may do
  pub role: Role,
  /// the networks the key is accepted from, comma-separated
  #[schema(example = "0.0.0.0/0,::/0")]
  pub cidr: String,
}

/// An entry of `GET /tokens`, told apart by `source`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ListedToken {
  /// a token created through the API
  Api(Token),
  /// a key from the configuration, which the API can not change
  Config(ConfigApiKey),
}

/// The body of `POST /tokens`. At most one of `expiresIn` and `expiresAt` is given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateToken {
  /// a name to tell the token apart by, 1 to 64 characters
  #[schema(example = "backup script")]
  pub name: String,
  /// what a request with the token may do, at most the creator's own role
  pub role: Role,
  /// how long the token lasts from now, such as `"30d"`; null for no expiry unless
  /// `expiresAt` is given
  #[schema(example = "30d")]
  pub expires_in: Option<String>,
  /// when the token stops being accepted; null for no expiry unless `expiresIn` is given
  pub expires_at: Option<DateTime<Utc>>,
  /// the networks the token is accepted from, comma-separated; null for anywhere
  #[schema(example = "192.168.1.0/24")]
  pub cidr: Option<String>,
}

/// A token just created, with the secret that is shown this once and never again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedToken {
  /// the token
  pub token: Token,
  /// the secret to send as `Authorization: Bearer <secret>`
  #[schema(example = "slsk_a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8s9T0")]
  pub secret: String,
}

/// The body of `PUT /tokens/{id}`. A field left null keeps its value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateToken {
  /// the new name, 1 to 64 characters
  #[schema(example = "backup script")]
  pub name: Option<String>,
  /// the new expiry
  pub expires_at: Option<DateTime<Utc>>,
}
