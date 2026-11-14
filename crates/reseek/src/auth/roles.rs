//! Mapping the groups an identity provider or a reverse proxy names to a role.

use api_types::auth::Role;
use serde_json::Value;

use crate::config::enums::DefaultRole;
use crate::config::{Oidc, OidcRoleMapping, RoleMapping};

/// The groups that grant each role, and the role of everyone else.
pub struct Mapping<'a> {
  /// groups whose members are administrators
  pub administrator: &'a [String],
  /// groups whose members may read and write
  pub readwrite: &'a [String],
  /// groups whose members may only read
  pub readonly: &'a [String],
  /// the role of a member of none of them, or a refusal
  pub default: DefaultRole,
}

impl<'a> From<&'a RoleMapping> for Mapping<'a> {
  fn from(mapping: &'a RoleMapping) -> Self {
    Self {
      administrator: &mapping.administrator,
      readwrite: &mapping.readwrite,
      readonly: &mapping.readonly,
      default: mapping.default,
    }
  }
}

// the provider's mapping adds the claim to the same four fields; cascade sections cannot
// flatten a shared struct, so the two config types stay apart
impl<'a> From<&'a OidcRoleMapping> for Mapping<'a> {
  fn from(mapping: &'a OidcRoleMapping) -> Self {
    Self {
      administrator: &mapping.administrator,
      readwrite: &mapping.readwrite,
      readonly: &mapping.readonly,
      default: mapping.default,
    }
  }
}

impl Mapping<'_> {
  /// The highest role `groups` give, `None` when the user is refused.
  pub fn role(&self, groups: &[String]) -> Option<Role> {
    let any = |listed: &[String]| groups.iter().any(|group| listed.contains(group));
    if any(self.administrator) {
      return Some(Role::Administrator);
    }
    if any(self.readwrite) {
      return Some(Role::ReadWrite);
    }
    if any(self.readonly) {
      return Some(Role::ReadOnly);
    }
    match self.default {
      DefaultRole::Deny => None,
      DefaultRole::ReadOnly => Some(Role::ReadOnly),
      DefaultRole::ReadWrite => Some(Role::ReadWrite),
      DefaultRole::Administrator => Some(Role::Administrator),
    }
  }
}

/// The role an identity provider's `options` give its user `subject` with the verified `email`
/// and `groups`; why not when they let the user in with none.
pub fn provider_role(
  options: &Oidc,
  subject: &str,
  email: Option<&str>,
  groups: &[String],
) -> Result<Role, String> {
  if !options.allowed_subjects.is_empty() && !options.allowed_subjects.iter().any(|s| s == subject)
  {
    return Err("the subject is not in allowed_subjects".to_string());
  }
  if !options.allowed_emails.is_empty()
    && !email.is_some_and(|email| options.allowed_emails.iter().any(|e| e == email))
  {
    return Err("no verified email in allowed_emails".to_string());
  }
  Mapping::from(&options.role_mapping)
    .role(groups)
    .ok_or_else(|| "in no group that grants a role".to_string())
}

/// The strings at the dotted `path` in `claims`: a list of strings, or one string.
pub fn claim_strings(claims: &Value, path: &str) -> Vec<String> {
  let mut value = claims;
  for part in path.split('.') {
    match value.get(part) {
      Some(inner) => value = inner,
      None => return Vec::new(),
    }
  }
  match value {
    Value::String(one) => vec![one.clone()],
    Value::Array(items) => items
      .iter()
      .filter_map(|item| item.as_str().map(str::to_string))
      .collect(),
    _ => Vec::new(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn the_highest_group_wins_and_the_default_may_refuse() {
    let admins = vec!["admins".to_string()];
    let users = vec!["users".to_string()];
    let mapping = Mapping {
      administrator: &admins,
      readwrite: &users,
      readonly: &[],
      default: DefaultRole::Deny,
    };
    let groups = |list: &[&str]| list.iter().map(|g| g.to_string()).collect::<Vec<_>>();
    assert_eq!(
      mapping.role(&groups(&["users", "admins"])),
      Some(Role::Administrator)
    );
    assert_eq!(mapping.role(&groups(&["users"])), Some(Role::ReadWrite));
    assert_eq!(
      mapping.role(&groups(&["Users"])),
      None,
      "names match exactly"
    );
    assert_eq!(mapping.role(&[]), None);
    let open = Mapping {
      default: DefaultRole::ReadOnly,
      ..mapping
    };
    assert_eq!(open.role(&[]), Some(Role::ReadOnly));
  }

  #[test]
  fn claims_are_found_by_dotted_path_as_a_list_or_a_string() {
    let claims = json!({
      "groups": ["a", "b", 3],
      "realm_access": { "roles": ["admin"] },
      "team": "ops",
    });
    assert_eq!(claim_strings(&claims, "groups"), ["a", "b"]);
    assert_eq!(claim_strings(&claims, "realm_access.roles"), ["admin"]);
    assert_eq!(claim_strings(&claims, "team"), ["ops"]);
    assert!(claim_strings(&claims, "realm_access.missing").is_empty());
  }
}
