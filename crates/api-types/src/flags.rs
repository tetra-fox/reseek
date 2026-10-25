//! Flag sets written as the names of their set flags joined by `", "`, in ascending bit order,
//! with `"None"` for the empty set, such as `"Completed, Succeeded"` or `"Connected, LoggedIn"`.

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

/// Implements the written form for a `bitflags` type: `Serialize`, `Deserialize` and an
/// OpenAPI string schema with `$example` as its example.
macro_rules! flag_names {
  ($ty:ty, $description:literal, $example:literal) => {
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
