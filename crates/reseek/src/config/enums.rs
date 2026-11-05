//! Options whose value is one of a fixed set of names, read in any case and written in lower
//! case.

pub(crate) use cascade::option_enum;

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

/// Which side of a relay the daemon is: the controller, the one side it plays. The relay agent
/// is the separate `reseek-agent` program, so `agent` and `debug`, the daemon's roles before
/// it, are refused with a pointer to it rather than read as something else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelayMode {
  /// takes connections from agents
  #[default]
  Controller,
}

impl RelayMode {
  /// Every name, in declaration order.
  pub const NAMES: &[&str] = &["controller"];

  /// The roles the daemon no longer plays, which the relay agent program took over.
  const MOVED: &[&str] = &["agent", "debug"];
}

impl serde::Serialize for RelayMode {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(Self::NAMES[0])
  }
}

impl<'de> serde::Deserialize<'de> for RelayMode {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let text = cascade::choice_text(deserializer, Self::NAMES)?;
    if text.eq_ignore_ascii_case(Self::NAMES[0]) {
      return Ok(Self::Controller);
    }
    let message = if Self::MOVED.iter().any(|m| text.eq_ignore_ascii_case(m)) {
      format!(
        "`{text}` is no longer a relay mode of the daemon: run the relay agent as the \
         reseek-agent program, and leave relay.mode unset or `controller`"
      )
    } else {
      format!("`{text}` is not one of: {}", Self::NAMES.join(", "))
    };
    Err(serde::de::Error::custom(message))
  }
}

impl cascade::Cascade for RelayMode {
  fn shape() -> cascade::Shape {
    cascade::Shape::choice(Self::NAMES)
  }
}

pub use reseek_shares::options::StorageMode;

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
  /// The least severe level logged, from the least to the most verbose.
  pub enum LogLevel {
    /// errors only
    Error = "error",
    /// warnings and errors
    Warn = "warn",
    /// what a user wants in a normal log
    #[default]
    Info = "info",
    /// the narrative of why things happen
    Debug = "debug",
    /// every detail
    Trace = "trace",
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

option_enum! {
  /// How the spectrogram's frequency axis is drawn.
  pub enum SpectrogramScale {
    /// octaves take the same height
    #[default]
    Log = "log",
    /// every kilohertz takes the same height
    Linear = "linear",
    /// equal steps of pitch as heard take the same height
    Mel = "mel",
  }
}
