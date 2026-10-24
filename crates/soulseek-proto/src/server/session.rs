//! Session messages: login, the listen port, online status, ping, share counts, being kicked,
//! upload speed and password changes (spec 01 sections 5.1, 5.2, 5.16 to 5.18, 5.20, 5.39,
//! 5.45).

use std::net::Ipv4Addr;

use md5::{Digest, Md5};

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::model::UserPresence;
use crate::wire::{Reader, Secret, Writer};

/// The major version sent in `Login`. 177 is the number the version registry keeps for
/// experimental clients; 170 belongs to Soulseek.NET and must not be reused.
pub const PROTOCOL_MAJOR_VERSION: u32 = 177;
/// The minor version sent in `Login`; the server only requires it to be non-zero
pub const PROTOCOL_MINOR_VERSION: u32 = 760;

/// Lowercase hex md5 of the utf-8 bytes of `username + password`, the `Login` hash field
pub fn login_hash(username: &str, password: &str) -> String {
  let mut md5 = Md5::new();
  md5.update(username.as_bytes());
  md5.update(password.as_bytes());
  format!("{:x}", md5.finalize())
}

wire_struct! {
  /// Server code 1, spec 01 section 5.1
  pub struct LoginRequest {
    pub username: String,
    pub password: Secret,
    pub major_version: u32,
    /// [`login_hash`] of the username and password
    pub hash: Secret,
    pub minor_version: u32,
  }
}

impl LoginRequest {
  /// a `Login` with the hash computed from the credentials
  pub fn new(username: String, password: String, major_version: u32, minor_version: u32) -> Self {
    let hash = Secret::new(login_hash(&username, &password));
    Self {
      username,
      password: Secret::new(password),
      major_version,
      hash,
      minor_version,
    }
  }
}

/// Server code 1, spec 01 section 5.1
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum LoginResponse {
  Success {
    /// the server's multi-line greeting
    greeting: String,
    /// our address as the server sees it
    ip: Ipv4Addr,
    /// md5 of the password we sent, informational
    hash: Secret,
    /// has ever bought privileges
    is_supporter: bool,
  },
  Failure {
    /// `INVALIDUSERNAME`, `INVALIDPASS`, `SVRFULL`, ... shown verbatim
    reason: String,
    detail: Option<String>,
  },
}

impl Wire for LoginResponse {
  const MIN_BYTES: usize = 5;

  /// `success` and `is_supporter` are true only for exactly 1
  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(if r.bool_exact("success")? {
      Self::Success {
        greeting: r.description("greeting")?,
        ip: r.ip4("ip")?,
        hash: Secret::read(r, "hash")?,
        is_supporter: r.bool_exact("is_supporter")?,
      }
    } else {
      Self::Failure {
        reason: r.string("reason")?,
        detail: if r.has_more() {
          Some(r.string("detail")?)
        } else {
          None
        },
      }
    })
  }

  fn write(&self, w: &mut Writer) {
    match self {
      Self::Success {
        greeting,
        ip,
        hash,
        is_supporter,
      } => {
        w.bool(true);
        w.string(greeting);
        w.ip4(*ip);
        hash.write(w);
        w.bool(*is_supporter);
      }
      Self::Failure { reason, detail } => {
        w.bool(false);
        w.string(reason);
        if let Some(detail) = detail {
          w.string(detail);
        }
      }
    }
  }
}

/// Server code 2, spec 01 section 5.2
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct SetListenPort {
  pub port: u32,
  /// `(obfuscation_type, obfuscated_port)`; never sent by this client
  pub obfuscation: Option<(u32, u32)>,
}

impl Wire for SetListenPort {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      port: r.u32("port")?,
      obfuscation: if r.remaining() >= 8 {
        Some((r.u32("obfuscation_type")?, r.u32("obfuscated_port")?))
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.port);
    if let Some((obfuscation_type, obfuscated_port)) = self.obfuscation {
      w.u32(obfuscation_type);
      w.u32(obfuscated_port);
    }
  }
}

wire_struct! {
  /// Server code 28, spec 01 section 5.16
  pub struct SetOnlineStatus {
    pub status: UserPresence,
  }
}

wire_struct! {
  /// Server code 35, spec 01 section 5.18
  pub struct SharedFoldersAndFiles {
    pub directory_count: u32,
    pub file_count: u32,
  }
}

wire_struct! {
  /// Server code 121, spec 01 section 5.39
  pub struct SendUploadSpeed {
    /// bytes/second
    pub speed: u32,
  }
}

wire_struct! {
  /// Server code 142, spec 01 section 5.45
  pub struct NewPassword {
    pub password: Secret,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{IP, decodes, ip, p, vectors};

  const MD5_USERNAME_PASSWORD: &str = "d51c9a7e9353746a6020f9602d452929";

  vectors! {
    login_request:
      Q::Login(LoginRequest::new("username".into(), "password".into(), 177, 760))
      => p().s("username").s("password").u32(177).s(MD5_USERNAME_PASSWORD).u32(760).frame(1);
    login_response_success:
      R::Login(LoginResponse::Success {
        greeting: "welcome\nto soulseek".into(), ip: ip(), hash: Secret::new(MD5_USERNAME_PASSWORD), is_supporter: true,
      })
      => p().u8(1).s("welcome\nto soulseek").ip(IP).s(MD5_USERNAME_PASSWORD).u8(1).frame(1);
    login_response_failure:
      R::Login(LoginResponse::Failure { reason: "INVALIDPASS".into(), detail: None }) => p().u8(0).s("INVALIDPASS").frame(1);
    login_response_failure_with_detail:
      R::Login(LoginResponse::Failure { reason: "INVALIDUSERNAME".into(), detail: Some("Nick too long.".into()) })
      => p().u8(0).s("INVALIDUSERNAME").s("Nick too long.").frame(1);
    set_listen_port_request: Q::SetListenPort(SetListenPort { port: 50_000, obfuscation: None }) => p().u32(50_000).frame(2);
    set_listen_port_request_obfuscated:
      Q::SetListenPort(SetListenPort { port: 50_000, obfuscation: Some((1, 50_001)) })
      => p().u32(50_000).u32(1).u32(50_001).frame(2);
    set_online_status_request:
      Q::SetOnlineStatus(SetOnlineStatus { status: UserPresence::Away }) => p().u32(1).frame(28);
    ping_request: Q::Ping => p().frame(32);
    ping_response: R::Ping => p().frame(32);
    shared_folders_and_files_request:
      Q::SharedFoldersAndFiles(SharedFoldersAndFiles { directory_count: 12, file_count: 345 }) => p().u32(12).u32(345).frame(35);
    kicked_from_server_response: R::KickedFromServer => p().frame(41);
    send_upload_speed_request: Q::SendUploadSpeed(SendUploadSpeed { speed: 123_456 }) => p().u32(123_456).frame(121);
    new_password_request: Q::NewPassword(NewPassword { password: Secret::new("n3w") }) => p().s("n3w").frame(142);
    new_password_response: R::NewPassword(NewPassword { password: Secret::new("n3w") }) => p().s("n3w").frame(142);
  }

  decodes! {
    login_response_success_must_be_exactly_one: R, p().u8(2).s("INVALIDPASS").frame(1)
      => Ok(R::Login(LoginResponse::Failure { reason: "INVALIDPASS".into(), detail: None }));
    ping_response_rejects_bytes_after_the_frame: R, [p().frame(32), vec![0]].concat()
      => Err(DecodeError::TrailingBytes { extra: 1 });
    set_listen_port_response_is_not_a_server_message: R, p().u32(1).frame(2)
      => Err(DecodeError::UnknownCode { code: 2 });
  }

  #[test]
  fn login_hash_is_md5_of_username_and_password() {
    assert_eq!(login_hash("username", "password"), MD5_USERNAME_PASSWORD);
  }

  #[test]
  fn login_request_debug_never_shows_credentials() {
    let login = LoginRequest::new("alice".into(), "hunter2".into(), 1, 2);
    let debug = format!("{login:?}");
    assert!(!debug.contains("hunter2"));
    assert!(!debug.contains(&login_hash("alice", "hunter2")));
    assert!(debug.contains("alice"));
  }
}
