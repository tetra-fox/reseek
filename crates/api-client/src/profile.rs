//! The calls on our own profile: the text and picture other users are sent, and our interests.

use api_types::options::OptionsChanged;
use api_types::profile::{Interests, OwnProfile, PasswordChange, ProfileText};
use bytes::Bytes;

use super::{Client, Result, Route, bytes, checked, done, json};

impl Client {
  /// `GET /profile`: our profile as other users are sent it, with where it comes from.
  pub async fn profile(&self) -> Result<OwnProfile> {
    json(self.get(Route::new("profile"))).await
  }

  /// `PUT /profile`: changes the profile text. With `revision`, the `optionsRevision` of the
  /// profile it was read from, a change made since is refused with 409; without, it is
  /// overwritten.
  pub async fn set_profile_text(
    &self,
    description: &str,
    revision: Option<&str>,
  ) -> Result<OptionsChanged> {
    let text = ProfileText {
      description: description.to_string(),
    };
    let route = Route::new("profile").query(&checked(revision))?;
    json(self.put(route).json(&text)).await
  }

  /// `PUT /profile/interests`: replaces what we like and hate, checked against `revision` as
  /// [`Client::set_profile_text`] is.
  pub async fn set_interests(
    &self,
    interests: &Interests,
    revision: Option<&str>,
  ) -> Result<OptionsChanged> {
    let route = Route::new("profile/interests").query(&checked(revision))?;
    json(self.put(route).json(interests)).await
  }

  /// `PUT /application/password`: changes our Soulseek account's password, at the server first
  /// and then where the option is kept.
  pub async fn change_password(&self, password: &str) -> Result<()> {
    let change = PasswordChange {
      password: password.to_string(),
    };
    done(self.put(Route::new("application/password")).json(&change)).await
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
