//! Our own profile: the text and picture other users are sent when they ask for our user info.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::options::OptionSource;

/// The query a picture URL carries, ours or another user's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PictureVersion {
  /// the picture's tag, so the URL changes whenever the picture does and a cached copy is never
  /// stale; the server does not read it
  pub v: Option<String>,
}

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

/// An interest the server recommends, with its score: above zero it is recommended, higher
/// first; zero and below it is recommended against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
  /// the interest
  #[schema(example = "electronic")]
  pub interest: String,
  /// the server's score for it
  pub score: i32,
}

/// Interests the server recommends and those it recommends against, each list best first. The
/// reply of `GET /interests/recommendations`, `GET /interests/popular` and
/// `GET /interests/{interest}/recommendations`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Recommendations {
  /// recommended, highest score first
  pub recommended: Vec<Recommendation>,
  /// recommended against, lowest score first
  pub unrecommended: Vec<Recommendation>,
}

/// A user whose interests are like ours. The items of `GET /interests/similar-users`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimilarUser {
  /// the user
  #[schema(example = "alice")]
  pub username: String,
  /// how alike, as the server rates it; higher is more alike
  pub rating: u32,
}

/// A new password for our Soulseek account, the body of `PUT /application/password`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordChange {
  /// the new password
  pub password: String,
}
