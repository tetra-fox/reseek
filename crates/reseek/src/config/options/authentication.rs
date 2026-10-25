//! Who may use the API and the web UI: local accounts, API keys, OpenID Connect providers, a
//! reverse proxy's forwarded identity and trusted networks.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::cidr::ANY;
use crate::config::enums::{DefaultRole, Role};

/// Who may use the API and the web UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Authentication {
  /// Lets every request in without a credential, as an administrator; the same as trusting
  /// every network.
  #[cascade(env = "RESEEK_NO_AUTH", cli = "-X|--no-auth", apply = "restart")]
  pub disabled: bool,
  /// The administrator account's username.
  #[cascade(env = "RESEEK_USERNAME", cli = "-u|--username")]
  pub username: String,
  /// The administrator account's password.
  #[cascade(env = "RESEEK_PASSWORD", cli = "-p|--password", secret)]
  pub password: String,
  /// The administrator account's password as an argon2id hash, used instead of `password`
  /// when set.
  #[cascade(env = "RESEEK_PASSWORD_HASH", cli = "--password-hash", secret)]
  pub password_hash: Option<String>,
  /// One API key given outside the files: the key alone, or `;`-separated parts of
  /// `role=<role>`, `cidr=<networks>` and the key, such as
  /// `role=readonly;cidr=192.168.1.0/24;<key>`. An administrator key from anywhere by default.
  #[cascade(
    env = "RESEEK_API_KEY",
    cli = "-k|--api-key",
    apply = "restart",
    secret
  )]
  pub api_key: Option<String>,
  /// API keys by name.
  #[cascade(user_keyed)]
  pub api_keys: BTreeMap<String, ApiKey>,
  /// Networks whose requests need no credential.
  #[cascade(env = "RESEEK_TRUSTED_NETWORKS", cli = "--trusted-networks")]
  pub trusted_networks: Vec<String>,
  /// The role of a request from a trusted network.
  #[cascade(env = "RESEEK_TRUSTED_ROLE", cli = "--trusted-role")]
  pub trusted_role: Role,
  /// How long a session from a local login lasts.
  #[cascade(env = "RESEEK_SESSION_TTL", cli = "--session-ttl")]
  #[serde(with = "humantime_serde")]
  pub session_ttl: Duration,
  /// How long a session lasts without being used.
  #[cascade(env = "RESEEK_SESSION_IDLE", cli = "--session-idle")]
  #[serde(with = "humantime_serde")]
  pub session_idle: Duration,
  /// Password login.
  pub local: Local,
  /// OpenID Connect providers by name.
  #[cascade(user_keyed)]
  pub oidc: BTreeMap<String, Oidc>,
  /// A reverse proxy's forwarded identity.
  pub proxy: Proxy,
}

impl Default for Authentication {
  fn default() -> Self {
    Self {
      disabled: false,
      username: "reseek".to_string(),
      password: "reseek".to_string(),
      password_hash: None,
      api_key: None,
      api_keys: BTreeMap::new(),
      trusted_networks: Vec::new(),
      trusted_role: Role::Administrator,
      session_ttl: Duration::from_secs(7 * 24 * 3600),
      session_idle: Duration::from_secs(30 * 24 * 3600),
      local: Local::default(),
      oidc: BTreeMap::new(),
      proxy: Proxy::default(),
    }
  }
}

/// An API key from the configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ApiKey {
  /// The key, 16 to 255 characters.
  #[cascade(secret)]
  pub key: Option<String>,
  /// What the key may do.
  pub role: Role,
  /// The networks the key may be used from, comma-separated.
  pub cidr: String,
}

impl Default for ApiKey {
  fn default() -> Self {
    Self {
      key: None,
      role: Role::ReadOnly,
      cidr: ANY.to_string(),
    }
  }
}

/// Password login.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Local {
  /// Offers the password login.
  #[cascade(env = "RESEEK_LOCAL_LOGIN", cli = "--local-login")]
  pub enabled: bool,
  /// More local accounts by username.
  #[cascade(user_keyed)]
  pub users: BTreeMap<String, LocalUser>,
  /// The networks password login is allowed from; anywhere when empty.
  #[cascade(env = "RESEEK_LOCAL_LOGIN_ONLY_FROM", cli = "--local-login-only-from")]
  pub only_from: Vec<String>,
}

impl Default for Local {
  fn default() -> Self {
    Self {
      enabled: true,
      users: BTreeMap::new(),
      only_from: Vec::new(),
    }
  }
}

/// A local account besides the administrator.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct LocalUser {
  /// The password as an argon2id hash.
  #[cascade(secret)]
  pub password_hash: Option<String>,
  /// The password in plain text, accepted with a warning at startup.
  #[cascade(secret)]
  pub password: Option<String>,
  /// What the account may do.
  pub role: Role,
}

/// An OpenID Connect provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Oidc {
  /// The issuer URL; discovery is read from under it.
  pub issuer: Option<String>,
  /// The client id registered with the provider.
  pub client_id: Option<String>,
  /// The client secret, for a confidential client.
  #[cascade(secret)]
  pub client_secret: Option<String>,
  /// The login button's label; the provider's name when not set.
  pub display_name: Option<String>,
  /// The scopes asked for.
  pub scopes: Vec<String>,
  /// How the provider's groups map to roles.
  pub role_mapping: OidcRoleMapping,
  /// The only subjects allowed to log in; anyone when empty.
  pub allowed_subjects: Vec<String>,
  /// The only verified email addresses allowed to log in; anyone when empty.
  pub allowed_emails: Vec<String>,
  /// The claim that names the user, falling back to the email and then the subject.
  pub username_claim: String,
  /// Sends the login page straight to this provider when it is the only one.
  pub auto_login: bool,
  /// How long a session from this provider lasts.
  #[serde(with = "humantime_serde")]
  pub session_ttl: Duration,
  /// Logging out at the provider.
  pub logout: OidcLogout,
  /// A PEM file with an extra certificate authority for a private provider.
  pub ca_certificate: Option<PathBuf>,
  /// Refuses an `http://` issuer that is not on this machine.
  pub require_https: bool,
}

impl Default for Oidc {
  fn default() -> Self {
    Self {
      issuer: None,
      client_id: None,
      client_secret: None,
      display_name: None,
      scopes: ["openid", "profile", "email", "groups"]
        .map(str::to_string)
        .to_vec(),
      role_mapping: OidcRoleMapping::default(),
      allowed_subjects: Vec::new(),
      allowed_emails: Vec::new(),
      username_claim: "preferred_username".to_string(),
      auto_login: false,
      session_ttl: Duration::from_secs(12 * 3600),
      logout: OidcLogout::default(),
      ca_certificate: None,
      require_https: true,
    }
  }
}

/// How an OpenID Connect provider's groups map to roles; the highest role a user's groups
/// give wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct OidcRoleMapping {
  /// The claim with the user's groups, a dotted path for a nested claim.
  pub claim: String,
  /// Groups whose users are administrators.
  pub administrator: Vec<String>,
  /// Groups whose users may read and write.
  pub readwrite: Vec<String>,
  /// Groups whose users may only read.
  pub readonly: Vec<String>,
  /// The role of a user in none of the groups, or `deny` to refuse them.
  pub default: DefaultRole,
}

impl Default for OidcRoleMapping {
  fn default() -> Self {
    Self {
      claim: "groups".to_string(),
      administrator: Vec::new(),
      readwrite: Vec::new(),
      readonly: Vec::new(),
      default: DefaultRole::Deny,
    }
  }
}

/// Logging out at an OpenID Connect provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct OidcLogout {
  /// Sends the browser to the provider's logout page on logout when it has one.
  pub rp_initiated: bool,
  /// Accepts the provider's back-channel logout calls.
  pub back_channel: bool,
}

impl Default for OidcLogout {
  fn default() -> Self {
    Self {
      rp_initiated: true,
      back_channel: true,
    }
  }
}

/// A reverse proxy's forwarded identity, believed only from trusted proxies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Proxy {
  /// Takes the identity the proxy forwards.
  #[cascade(env = "RESEEK_PROXY_AUTH", cli = "--proxy-auth")]
  pub enabled: bool,
  /// The header with the username.
  #[cascade(env = "RESEEK_PROXY_AUTH_USER_HEADER")]
  pub user_header: String,
  /// The header with the user's groups, separated by `|` or `,`.
  #[cascade(env = "RESEEK_PROXY_AUTH_GROUPS_HEADER")]
  pub groups_header: String,
  /// The header with the user's email address.
  #[cascade(env = "RESEEK_PROXY_AUTH_EMAIL_HEADER")]
  pub email_header: String,
  /// How the forwarded groups map to roles.
  pub role_mapping: RoleMapping,
  /// Starts a session when the web UI's page is served to a user the proxy names and the
  /// browser has no session yet, so the page's later requests work without the proxy's
  /// headers; API requests act on the headers alone. Off, the headers are read on every
  /// request.
  #[cascade(env = "RESEEK_PROXY_AUTH_SESSION")]
  pub session: bool,
}

impl Default for Proxy {
  fn default() -> Self {
    Self {
      enabled: false,
      user_header: "X-Authentik-Username".to_string(),
      groups_header: "X-Authentik-Groups".to_string(),
      email_header: "X-Authentik-Email".to_string(),
      role_mapping: RoleMapping::default(),
      session: true,
    }
  }
}

/// How forwarded groups map to roles; the highest role a user's groups give wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RoleMapping {
  /// Groups whose users are administrators.
  pub administrator: Vec<String>,
  /// Groups whose users may read and write.
  pub readwrite: Vec<String>,
  /// Groups whose users may only read.
  pub readonly: Vec<String>,
  /// The role of a user in none of the groups, or `deny` to refuse them.
  pub default: DefaultRole,
}
