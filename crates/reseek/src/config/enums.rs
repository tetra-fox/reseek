//! Options whose value is one of a fixed set of names, read in any case and written in lower
//! case.

/// Declares an option enum: each variant with the name it is written as. Reading ignores case;
/// a name not in the set is refused with the list of names.
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

    impl serde::Serialize for $name {
      fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
      }
    }

    impl<'de> serde::Deserialize<'de> for $name {
      fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
          serde::de::Error::custom(format!(
            "`{text}` is not one of: {}",
            Self::NAMES.join(", ")
          ))
        })
      }
    }

    impl cascade::Cascade for $name {
      fn shape() -> cascade::Shape {
        cascade::Shape::leaf(cascade::Kind::String)
      }
    }
  };
}

option_enum! {
  /// What a principal may do.
  pub enum Role {
    /// read anything, change nothing
    #[default]
    ReadOnly = "readonly",
    /// also search, download, chat and manage transfers
    ReadWrite = "readwrite",
    /// also change the configuration, groups and other principals
    Administrator = "administrator",
  }
}

impl From<Role> for api_types::auth::Role {
  fn from(role: Role) -> Self {
    match role {
      Role::ReadOnly => Self::ReadOnly,
      Role::ReadWrite => Self::ReadWrite,
      Role::Administrator => Self::Administrator,
    }
  }
}

option_enum! {
  /// The role an identity provider's user gets when no group of theirs maps to one, or `deny`
  /// to refuse them.
  pub enum DefaultRole {
    /// refuse the login
    #[default]
    Deny = "deny",
    /// read only
    ReadOnly = "readonly",
    /// read and write
    ReadWrite = "readwrite",
    /// administrator
    Administrator = "administrator",
  }
}

option_enum! {
  /// Which side of a relay this instance is.
  pub enum RelayMode {
    /// takes connections from agents
    #[default]
    Controller = "controller",
    /// connects to a controller and serves its shares through it
    Agent = "agent",
    /// a controller that also connects to itself as an agent, for development
    Debug = "debug",
  }
}

option_enum! {
  /// Where the share index is kept.
  pub enum StorageMode {
    /// in memory, rebuilt by a scan at every start
    #[default]
    Memory = "memory",
    /// in a database file in the cache directory
    Disk = "disk",
  }
}

option_enum! {
  /// What a retried download does with the part already on disk.
  pub enum RetryPartial {
    /// start the file again
    Overwrite = "overwrite",
    /// continue from the size on disk
    #[default]
    Resume = "resume",
  }
}

option_enum! {
  /// What a finished download does when its destination file already exists.
  pub enum DestinationExists {
    /// keep both, giving the new file a numbered name
    #[default]
    Rename = "rename",
    /// replace the existing file
    Overwrite = "overwrite",
  }
}

option_enum! {
  /// The order a group's queued uploads are served in.
  pub enum QueueStrategy {
    /// one file per user in turn
    #[default]
    RoundRobin = "roundrobin",
    /// in the order they were queued
    FirstInFirstOut = "firstinfirstout",
  }
}

option_enum! {
  /// How a download asks the peer for the file.
  pub enum RequestStyle {
    /// the queue request every current client sends; the peer does not answer it
    #[default]
    Queue = "queue",
    /// the older transfer request, which the peer answers with queued, start now or a refusal
    TransferRequest = "transfer_request",
  }
}

option_enum! {
  /// The least severe protocol diagnostic that is logged.
  pub enum DiagnosticLevel {
    /// none
    None = "none",
    /// warnings only
    Warning = "warning",
    /// warnings and information
    #[default]
    Info = "info",
    /// warnings, information and the protocol narrative
    Debug = "debug",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn option_enums_read_any_case_and_write_lower_case() {
    let role: Role = serde_json::from_str("\"ReadWrite\"").unwrap();
    assert_eq!(role, Role::ReadWrite);
    assert_eq!(serde_json::to_string(&role).unwrap(), "\"readwrite\"");
    let e = serde_json::from_str::<Role>("\"guest\"").unwrap_err();
    assert!(
      e.to_string()
        .starts_with("`guest` is not one of: readonly, readwrite, administrator"),
      "{e}"
    );
    assert!(
      serde_json::from_str::<RelayMode>("\"1\"").is_err(),
      "no numbers"
    );
  }
}
