//! Who may use the API and the web UI: local accounts, API keys, OpenID Connect providers, a
//! reverse proxy's forwarded identity and trusted networks.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::cidr::ANY;
use crate::config::enums::{DefaultRole, Role};

/// Access to the API and the web UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Authentication {
  /// Disables authentication. Every request is accepted as an administrator, the same as
  /// trusting every network.
  #[cascade(env = "RESEEK_NO_AUTH", cli = "-X|--no-auth", apply = "restart")]
  pub disabled: bool,
  /// Administrator username.
  #[cascade(env = "RESEEK_USERNAME", cli = "-u|--username")]
  pub username: String,
  /// Administrator password.
  #[cascade(env = "RESEEK_PASSWORD", cli = "-p|--password", secret)]
  pub password: String,
  /// Administrator password as an argon2id hash. Used instead of `password` when set.
  #[cascade(env = "RESEEK_PASSWORD_HASH", cli = "--password-hash", secret)]
  pub password_hash: Option<String>,
  /// An API key set outside the configuration files: the key alone, or `;`-separated parts
  /// `role=<role>`, `cidr=<networks>` and the key, such as
  /// `role=readonly;cidr=192.168.1.0/24;<key>`. A key without parts has the administrator
  /// role from any network.
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
  /// Networks whose requests need no credentials.
  #[cascade(env = "RESEEK_TRUSTED_NETWORKS", cli = "--trusted-networks")]
  pub trusted_networks: Vec<String>,
  /// Role granted to requests from trusted networks.
  #[cascade(env = "RESEEK_TRUSTED_ROLE", cli = "--trusted-role")]
  pub trusted_role: Role,
  /// Lifetime of a session from a password login.
  #[cascade(env = "RESEEK_SESSION_TTL", cli = "--session-ttl")]
  #[serde(with = "humantime_serde")]
  pub session_ttl: Duration,
  /// Time after which an unused session expires.
  #[cascade(env = "RESEEK_SESSION_IDLE", cli = "--session-idle")]
  #[serde(with = "humantime_serde")]
  pub session_idle: Duration,
  /// Password login.
  pub local: Local,
  /// OpenID Connect providers by name.
  #[cascade(user_keyed)]
  pub oidc: BTreeMap<String, Oidc>,
  /// Identity forwarded by a reverse proxy.
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
  /// API key, 16 to 255 characters.
  #[cascade(secret)]
  pub key: Option<String>,
  /// Role of the key.
  pub role: Role,
  /// Networks the key may be used from, comma-separated.
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
  /// Enables password login.
  #[cascade(env = "RESEEK_LOCAL_LOGIN", cli = "--local-login")]
  pub enabled: bool,
  /// Additional local accounts by username.
  #[cascade(user_keyed)]
  pub users: BTreeMap<String, LocalUser>,
  /// Networks password login is allowed from. Any network when empty.
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

/// A local account other than the administrator.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct LocalUser {
  /// Password as an argon2id hash.
  #[cascade(secret)]
  pub password_hash: Option<String>,
  /// Password in plain text. Accepted with a warning at startup.
  #[cascade(secret)]
  pub password: Option<String>,
  /// Role of the account.
  pub role: Role,
}

/// An OpenID Connect provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Oidc {
  /// Issuer URL. Discovery is read from under it.
  pub issuer: Option<String>,
  /// Client ID registered with the provider.
  pub client_id: Option<String>,
  /// Client secret, for a confidential client.
  #[cascade(secret)]
  pub client_secret: Option<String>,
  /// Label of the login button. The provider name is used when not set.
  pub display_name: Option<String>,
  /// Requested scopes.
  pub scopes: Vec<String>,
  /// Mapping of provider groups to roles.
  pub role_mapping: OidcRoleMapping,
  /// Subjects allowed to log in. Anyone when empty.
  pub allowed_subjects: Vec<String>,
  /// Verified email addresses allowed to log in. Anyone when empty.
  pub allowed_emails: Vec<String>,
  /// Claim that holds the username. Falls back to the email address, then the subject.
  pub username_claim: String,
  /// Redirects the login page to this provider when it is the only one.
  pub auto_login: bool,
  /// Lifetime of a session from this provider.
  #[serde(with = "humantime_serde")]
  pub session_ttl: Duration,
  /// Logout at the provider.
  pub logout: OidcLogout,
  /// PEM file with an additional certificate authority, for a private provider.
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

/// Mapping of provider groups to roles. The highest role granted by any group of a user
/// applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct OidcRoleMapping {
  /// Claim that holds the groups of the user. A dotted path selects a nested claim.
  pub claim: String,
  /// Groups granted the administrator role.
  pub administrator: Vec<String>,
  /// Groups granted read and write access.
  pub readwrite: Vec<String>,
  /// Groups granted read-only access.
  pub readonly: Vec<String>,
  /// Role for users in none of the groups, or `deny` to refuse them.
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

/// Logout at an OpenID Connect provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct OidcLogout {
  /// Redirects the browser to the logout page of the provider on logout, when the provider
  /// has one.
  pub rp_initiated: bool,
  /// Accepts back-channel logout requests from the provider.
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

/// Identity forwarded by a reverse proxy, accepted only from trusted proxies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Proxy {
  /// Accepts the identity forwarded by the proxy.
  #[cascade(env = "RESEEK_PROXY_AUTH", cli = "--proxy-auth")]
  pub enabled: bool,
  /// Header that holds the username.
  #[cascade(env = "RESEEK_PROXY_AUTH_USER_HEADER")]
  pub user_header: String,
  /// Header that holds the groups of the user, separated by `|` or `,`.
  #[cascade(env = "RESEEK_PROXY_AUTH_GROUPS_HEADER")]
  pub groups_header: String,
  /// Header that holds the email address.
  #[cascade(env = "RESEEK_PROXY_AUTH_EMAIL_HEADER")]
  pub email_header: String,
  /// Mapping of forwarded groups to roles.
  pub role_mapping: RoleMapping,
  /// Starts a session when the web UI is served to a user named by the proxy and the browser
  /// has no session, so that later requests from the page work without the proxy headers. API
  /// requests rely on the headers alone. When off, the headers are read on every request.
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

/// Mapping of forwarded groups to roles. The highest role granted by any group of a user
/// applies.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RoleMapping {
  /// Groups granted the administrator role.
  pub administrator: Vec<String>,
  /// Groups granted read and write access.
  pub readwrite: Vec<String>,
  /// Groups granted read-only access.
  pub readonly: Vec<String>,
  /// Role for users in none of the groups, or `deny` to refuse them.
  pub default: DefaultRole,
}
