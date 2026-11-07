//! Our own profile: the text and picture other users are sent when they ask for our user info,
//! and what we like and hate.

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

/// Where the text, the picture and the interests of our profile come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSources {
  /// the layer that sets `soulseek.description`
  pub description: OptionSource,
  /// the layer that sets `soulseek.picture`
  pub picture: OptionSource,
  /// the layer that sets `soulseek.interests.liked`
  pub liked: OptionSource,
  /// the layer that sets `soulseek.interests.hated`
  pub hated: OptionSource,
}

/// Our profile as a user asking for our user info is sent it now, with what we like and hate
/// and where each comes from. The reply of `GET /profile`.
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
  /// what we like and hate, which the server is told
  pub interests: Interests,
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

/// What a user likes and hates, each in the order listed: ours in `GET /profile` and the body of
/// `PUT /profile/interests`, another user's in the reply of `GET /users/{username}/interests`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Interests {
  /// what they like
  pub liked: Vec<String>,
  /// what they hate
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
