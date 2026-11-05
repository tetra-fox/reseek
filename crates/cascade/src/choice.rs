//! Options whose value is one of a fixed set of names, read in any case and written in lower
//! case.

/// Declares an option enum: each variant with the name it is written as. Reading ignores case;
/// a name not in the set is refused with the list of names.
///
/// ```
/// cascade::option_enum! {
///   /// Where the index is kept.
///   pub enum Storage {
///     /// in memory
///     #[default]
///     Memory = "memory",
///     /// in a file
///     Disk = "disk",
///   }
/// }
///
/// assert_eq!(Storage::parse("DISK"), Some(Storage::Disk));
/// assert_eq!(Storage::default().as_str(), "memory");
/// ```
#[macro_export]
macro_rules! option_enum {
  (
    $(#[$meta:meta])*
    pub enum $name:ident {
      $($(#[$vmeta:meta])* $variant:ident = $text:literal,)+
    }
  ) => {
    $(#[$meta])*
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum $name {
      $($(#[$vmeta])* $variant,)+
    }

    impl $name {
      /// Every name, in declaration order.
      pub const NAMES: &[&str] = &[$($text),+];

      /// The name the value is written as.
      pub fn as_str(self) -> &'static str {
        match self {
          $(Self::$variant => $text,)+
        }
      }

      /// The value named `text`, in any case.
      pub fn parse(text: &str) -> Option<Self> {
        $(if text.eq_ignore_ascii_case($text) {
          return Some(Self::$variant);
        })+
        None
      }
    }

    impl $crate::__private::serde::Serialize for $name {
      fn serialize<S: $crate::__private::serde::Serializer>(
        &self,
        serializer: S,
      ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
      }
    }

    impl<'de> $crate::__private::serde::Deserialize<'de> for $name {
      fn deserialize<D: $crate::__private::serde::Deserializer<'de>>(
        deserializer: D,
      ) -> Result<Self, D::Error> {
        let text = $crate::choice_text(deserializer, Self::NAMES)?;
        Self::parse(&text).ok_or_else(|| {
          <D::Error as $crate::__private::serde::de::Error>::custom(format!(
            "`{text}` is not one of: {}",
            Self::NAMES.join(", ")
          ))
        })
      }
    }

    impl $crate::Cascade for $name {
      fn shape() -> $crate::Shape {
        $crate::Shape::choice(Self::NAMES)
      }
    }
  };
}

/// Reads the text of an option whose value is one of `names`, as written; the caller matches it
/// against the names. A value that is not text is refused with the names, so the message says
/// what to write instead.
///
/// # Errors
///
/// When the value is not a string.
pub fn choice_text<'de, D: serde::Deserializer<'de>>(
  deserializer: D,
  names: &'static [&'static str],
) -> Result<String, D::Error> {
  deserializer.deserialize_str(Names(names))
}

/// Takes a string, and expects one of its names in a refusal.
struct Names(&'static [&'static str]);

impl serde::de::Visitor<'_> for Names {
  type Value = String;

  fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(formatter, "one of {}", self.0.join(", "))
  }

  fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<String, E> {
    Ok(text.to_string())
  }
}

#[cfg(test)]
mod tests {
  crate::option_enum! {
    /// What a principal may do.
    pub enum Role {
      /// read anything
      #[default]
      ReadOnly = "readonly",
      /// also write
      ReadWrite = "readwrite",
    }
  }

  #[test]
  fn option_enums_read_any_case_and_write_lower_case() {
    let role: Role = serde_json::from_str("\"ReadWrite\"").unwrap();
    assert_eq!(role, Role::ReadWrite);
    assert_eq!(serde_json::to_string(&role).unwrap(), "\"readwrite\"");
    let e = serde_json::from_str::<Role>("\"guest\"").unwrap_err();
    assert!(
      e.to_string()
        .starts_with("`guest` is not one of: readonly, readwrite"),
      "{e}"
    );
    assert!(serde_json::from_str::<Role>("\"1\"").is_err(), "no numbers");
    let e = serde_json::from_str::<Role>("true").unwrap_err();
    assert!(
      e.to_string()
        .starts_with("invalid type: boolean `true`, expected one of readonly, readwrite"),
      "{e}"
    );
  }
}
