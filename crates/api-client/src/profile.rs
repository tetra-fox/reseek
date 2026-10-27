//! The calls on our own profile: the text and picture other users are sent.

use api_types::profile::{OwnProfile, ProfileText};
use bytes::Bytes;

use super::{Client, Result, Route, bytes, done, json};

impl Client {
  /// `GET /profile`: our profile as other users are sent it, with where it comes from.
  pub async fn profile(&self) -> Result<OwnProfile> {
    json(self.get(Route::new("profile"))).await
  }

  /// `PUT /profile`: changes the profile text.
  pub async fn set_profile_text(&self, description: &str) -> Result<()> {
    let text = ProfileText {
      description: description.to_string(),
    };
    done(self.put(Route::new("profile")).json(&text)).await
  }

  /// `GET /profile/picture`: the picture other users are sent.
  pub async fn profile_picture(&self) -> Result<Bytes> {
    bytes(self.get(Route::new("profile/picture"))).await
  }

  /// `PUT /profile/picture`: sets the picture from an image of `content_type`, such as
  /// `image/png`.
  pub async fn set_profile_picture(&self, image: Vec<u8>, content_type: &str) -> Result<()> {
    let request = self
      .put(Route::new("profile/picture"))
      .header("content-type", content_type)
      .body(image);
    done(request).await
  }

  /// `DELETE /profile/picture`: stops sending a picture.
  pub async fn remove_profile_picture(&self) -> Result<()> {
    done(self.delete(Route::new("profile/picture"))).await
  }
}
