//! Our own profile: the text and picture other users are sent when they ask for our user info.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::options::OptionSource;

/// Where the text and the picture of our profile come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSources {
  /// the layer that sets `soulseek.description`
  pub description: OptionSource,
  /// the layer that sets `soulseek.picture`
  pub picture: OptionSource,
}

/// Our profile as a user asking for our user info is sent it now, with where the text and the
/// picture come from. The reply of `GET /profile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnProfile {
  /// the profile text
  pub description: String,
  /// a picture is sent with it
  pub has_picture: bool,
  /// where the picture is fetched, a URL that changes whenever the picture does; null without one
  #[schema(
    example = "/api/v0/profile/picture?v=2cea274d0bedc39ec4ab6ba9e59ec889e3ed6fb56a1cf088a64d9b383378dc97"
  )]
  pub picture_url: Option<String>,
  /// the upload slots a user we know nothing about is told of, those of the default group
  pub upload_slots: u32,
  /// whether one of those slots is free
  pub free_upload_slot: bool,
  /// the place in the queue such a user is told a new request would take
  pub queue_length: u32,
  /// the layer each value comes from
  pub sources: ProfileSources,
}

/// The new text of our profile, the body of `PUT /profile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileText {
  /// the text other users read, newlines kept
  pub description: String,
}

/// What we like and what we hate, each in the order listed. The reply of `GET /interests` and the
/// body of `PUT /interests`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Interests {
  /// what we like
  pub liked: Vec<String>,
  /// what we hate
  pub hated: Vec<String>,
}

/// A new password for our Soulseek account, the body of `PUT /application/password`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordChange {
  /// the new password
  pub password: String,
}
