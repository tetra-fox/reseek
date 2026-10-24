/// Why a frame or message was rejected. Remote strings are never carried here, so an error can be
/// logged as is.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
  /// the length prefix is smaller than the code; the stream cannot be resynchronised, close the
  /// connection
  #[error("frame length {len} is below the code width {code_width}")]
  FrameTooShort { len: u32, code_width: usize },
  /// the length prefix is over the limit for the connection kind; close the connection
  #[error("frame length {len} exceeds the limit of {limit} bytes")]
  FrameTooLarge { len: u32, limit: u32 },
  /// the bytes given end before the frame their prefix announces; decoders take whole frames,
  /// so the caller handed over a partial one and the message is dropped
  #[error("the frame needs {needed} bytes but only {available} were given")]
  IncompleteFrame { needed: usize, available: usize },
  /// bytes follow the one frame or raw value a decoder takes; the message is dropped
  #[error("{extra} bytes follow the frame")]
  TrailingBytes { extra: usize },
  /// a code with no known layout; the message is dropped and the connection stays up
  #[error("unknown message code {code}")]
  UnknownCode { code: u32 },
  /// a field of a known message could not be read; the message is dropped
  #[error("{message} {field}: {error}")]
  Field {
    message: &'static str,
    field: &'static str,
    error: FieldError,
  },
}

/// What was wrong with one field of a message. The display text follows the message and field
/// name, as in "Login greeting: needs 12 bytes, 3 remain".
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FieldError {
  #[error("needs {needed} bytes, {remaining} remain")]
  Truncated { needed: usize, remaining: usize },
  #[error("length {len} exceeds the limit of {limit} bytes")]
  StringTooLong { len: u32, limit: usize },
  #[error("{count} elements cannot fit in the {remaining} remaining bytes")]
  CountTooLarge { count: u32, remaining: usize },
  #[error("{value} exceeds the limit of {limit}")]
  LimitExceeded { value: u64, limit: u64 },
  #[error("length differs from the earlier list")]
  ListLengthMismatch,
  #[error("invalid value {value}")]
  InvalidValue { value: u32 },
  #[error("unknown connection type")]
  UnknownConnectionType,
  #[error("not a valid username")]
  InvalidUsername,
  #[error("negative level {level}")]
  NegativeBranchLevel { level: i32 },
  #[error("identifier {identifier} is not 49")]
  BadSearchIdentifier { identifier: u32 },
  #[error("not a zlib stream")]
  InvalidZlib,
  #[error("the zlib stream ends before its end marker")]
  TruncatedZlib,
  #[error("inflates past the limit of {limit} bytes")]
  InflatedTooLarge { limit: usize },
}
