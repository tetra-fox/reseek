/// Why a frame or message was rejected. Remote strings are never carried here, so an error can be
/// logged as is.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
  #[error("frame length {len} is below the code width {code_width}")]
  FrameTooShort { len: u32, code_width: usize },
  #[error("frame length {len} exceeds the limit of {limit} bytes")]
  FrameTooLarge { len: u32, limit: u32 },
  #[error("the frame needs {needed} bytes but only {available} were given")]
  IncompleteFrame { needed: usize, available: usize },
  #[error("{extra} bytes follow the frame")]
  TrailingBytes { extra: usize },
  #[error("unknown message code {code}")]
  UnknownCode { code: u32 },
  #[error("the message ended early: {needed} bytes needed, {remaining} remaining")]
  Truncated { needed: usize, remaining: usize },
  #[error("a string of {len} bytes exceeds the limit of {limit}")]
  StringTooLong { len: u32, limit: usize },
  #[error("{count} {what} cannot fit in the {remaining} remaining bytes")]
  CountTooLarge {
    what: &'static str,
    count: u32,
    remaining: usize,
  },
  #[error("{what} {value} exceeds the limit of {limit}")]
  LimitExceeded {
    what: &'static str,
    value: u64,
    limit: u64,
  },
  #[error("parallel lists of different lengths: {what}")]
  ListLengthMismatch { what: &'static str },
  #[error("invalid {what} value {value}")]
  InvalidValue { what: &'static str, value: u32 },
  #[error("unknown peer connection type")]
  UnknownConnectionType,
  #[error("invalid username in {what}")]
  InvalidUsername { what: &'static str },
  #[error("negative branch level {level}")]
  NegativeBranchLevel { level: i32 },
  #[error("distributed search identifier {identifier} is not 49")]
  BadSearchIdentifier { identifier: u32 },
  #[error("invalid zlib stream")]
  InvalidZlib,
  #[error("the zlib stream ended before its end marker")]
  TruncatedZlib,
  #[error("the inflated payload exceeds the limit of {limit} bytes")]
  InflatedTooLarge { limit: usize },
}
