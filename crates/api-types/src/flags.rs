//! Flag sets written as the names of their set flags joined by `", "`, in ascending bit order,
//! with `"None"` for the empty set: the server connection state such as `"Connected, LoggedIn"`,
//! a search's state such as `"Completed, TimedOut"`, and file attributes such as
//! `"ReadOnly, Directory"`.

use bitflags::Flags;

/// The written form of `value`. The flags are declared in ascending bit order, which is the
/// order their names are written in.
pub(crate) fn to_names<F: Flags>(value: &F) -> String {
  if value.is_empty() {
    return "None".to_string();
  }
  value
    .iter_names()
    .map(|(name, _)| name)
    .collect::<Vec<_>>()
    .join(", ")
}

/// The flag set `text` names. Names are matched exactly; `"None"` is the empty set.
pub(crate) fn from_names<F: Flags>(text: &str) -> Result<F, String> {
  if text == "None" {
    return Ok(F::empty());
  }
  text.split(',').try_fold(F::empty(), |set, name| {
    let name = name.trim();
    F::from_name(name)
      .map(|flag| set.union(flag))
      .ok_or_else(|| format!("`{name}` is not a flag name"))
  })
}

/// Implements the written form for a `bitflags` type: `Display`, `Serialize`, `Deserialize` and
/// an OpenAPI string schema with `$example` as its example.
macro_rules! flag_names {
  ($ty:ty, $description:literal, $example:literal) => {
    impl std::fmt::Display for $ty {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&$crate::flags::to_names(self))
      }
    }

    impl serde::Serialize for $ty {
      fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&$crate::flags::to_names(self))
      }
    }

    impl<'de> serde::Deserialize<'de> for $ty {
      fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        $crate::flags::from_names(&text).map_err(serde::de::Error::custom)
      }
    }

    impl utoipa::PartialSchema for $ty {
      fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
          .schema_type(utoipa::openapi::schema::Type::String)
          .description(Some($description))
          .examples([$example])
          .into()
      }
    }

    impl utoipa::ToSchema for $ty {}
  };
}

pub(crate) use flag_names;

#[cfg(test)]
mod tests {
  use std::fmt::Debug;

  use serde::Serialize;
  use serde::de::DeserializeOwned;

  use crate::files::FilesystemAttributes as Fs;
  use crate::search::SearchStates as Search;
  use crate::state::ServerConnectionState as Server;

  fn round_trip<F: Serialize + DeserializeOwned + PartialEq + Debug>(rows: &[(F, &str)]) {
    for (value, text) in rows {
      let json = format!("\"{text}\"");
      assert_eq!(serde_json::to_string(value).unwrap(), json);
      assert_eq!(&serde_json::from_str::<F>(&json).unwrap(), value, "{text}");
    }
  }

  #[test]
  fn flag_sets_are_written_by_name_in_bit_order() {
    round_trip(&[
      (Server::Connected | Server::LoggedIn, "Connected, LoggedIn"),
      (
        Server::LoggingIn | Server::Connected,
        "Connected, LoggingIn",
      ),
      (Server::empty(), "None"),
    ]);
    round_trip(&[
      (Fs::Directory | Fs::ReadOnly, "ReadOnly, Directory"),
      (Fs::ReparsePoint | Fs::Directory, "Directory, ReparsePoint"),
    ]);
    round_trip(&[
      (Search::Requested, "Requested"),
      (Search::Queued, "Queued"),
      (Search::InProgress, "InProgress"),
      (Search::TimedOut | Search::Completed, "Completed, TimedOut"),
      (
        Search::Completed | Search::ResponseLimitReached,
        "Completed, ResponseLimitReached",
      ),
      (Search::Completed | Search::Errored, "Completed, Errored"),
      (Search::empty(), "None"),
    ]);
  }
}
