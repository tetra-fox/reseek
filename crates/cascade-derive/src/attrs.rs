//! Reads the `#[cascade(...)]` attributes of a struct and its fields, and the few `#[serde(...)]`
//! attributes that decide a field's key.

use syn::meta::ParseNestedMeta;
use syn::{Attribute, Expr, Field, Ident, LitStr, Token, token};

/// The kinds a field can name with `kind = "..."`, as the `Kind` variant each one becomes.
pub(crate) const KINDS: &[(&str, &str)] = &[
  ("bool", "Bool"),
  ("integer", "Integer"),
  ("float", "Float"),
  ("string", "String"),
  ("duration", "Duration"),
  ("size", "Size"),
];

/// The apply classes a field can name with `apply = "..."`, as the `Apply` variant each one becomes.
pub(crate) const APPLY: &[(&str, &str)] = &[
  ("live", "Live"),
  ("reconnect", "Reconnect"),
  ("restart", "Restart"),
];

/// What `#[cascade(...)]` on the struct says.
#[derive(Default)]
pub(crate) struct Container {
  /// `(from, to)`: every env var under the struct named `from...` is also read as `to...`
  pub(crate) alias_prefixes: Vec<(String, String)>,
}

/// What the attributes of one field say.
pub(crate) struct FieldAttrs {
  /// the key segment: the `serde(rename)` if there is one, else the field's name
  pub(crate) name: String,
  pub(crate) env: Option<String>,
  pub(crate) env_aliases: Vec<String>,
  pub(crate) long: Option<String>,
  pub(crate) short: Option<char>,
  /// the flag turns a boolean off rather than on
  pub(crate) negated: bool,
  /// the `Apply` variant
  pub(crate) apply: Option<&'static str>,
  pub(crate) secret: bool,
  /// a filesystem path, counted from where the layer that sets it is
  pub(crate) path: bool,
  pub(crate) doc: String,
  /// the `Kind` variant
  pub(crate) kind: Option<&'static str>,
  pub(crate) user_keyed: bool,
  /// `serde(skip)`: the field is not part of the configuration
  pub(crate) skip: bool,
}

/// Collects every error instead of stopping at the first, so one build shows them all.
#[derive(Default)]
pub(crate) struct Errors(Option<syn::Error>);

impl Errors {
  pub(crate) fn push(&mut self, error: syn::Error) {
    match &mut self.0 {
      Some(first) => first.combine(error),
      None => self.0 = Some(error),
    }
  }

  pub(crate) fn finish(self) -> syn::Result<()> {
    self.0.map_or(Ok(()), Err)
  }
}

pub(crate) fn container(attrs: &[Attribute], errors: &mut Errors) -> Container {
  let mut container = Container::default();
  for attr in attrs {
    if attr.path().is_ident("cascade") {
      let parsed = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("env_alias_prefix") {
          container.alias_prefixes.push(alias_prefix(&meta)?);
          Ok(())
        } else {
          Err(meta.error("unknown cascade attribute on a struct, expected `env_alias_prefix`"))
        }
      });
      if let Err(e) = parsed {
        errors.push(e);
      }
    } else if attr.path().is_ident("serde") {
      let parsed = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("rename_all") {
          return Err(meta.error(
            "cascade keys follow the field names, rename fields one by one with \
             #[serde(rename = \"...\")] instead of rename_all",
          ));
        }
        skip_meta(&meta)
      });
      if let Err(e) = parsed {
        errors.push(e);
      }
    }
  }
  container
}

/// `env_alias_prefix(from = "A_", to = "B_")`
fn alias_prefix(meta: &ParseNestedMeta) -> syn::Result<(String, String)> {
  let mut from = None;
  let mut to = None;
  meta.parse_nested_meta(|inner| {
    let slot = if inner.path.is_ident("from") {
      &mut from
    } else if inner.path.is_ident("to") {
      &mut to
    } else {
      return Err(inner.error("expected `from` or `to`"));
    };
    let lit: LitStr = inner.value()?.parse()?;
    check_env_name(&lit)?;
    *slot = Some(lit.value());
    Ok(())
  })?;
  match (from, to) {
    (Some(from), Some(to)) => Ok((from, to)),
    _ => Err(meta.error("env_alias_prefix needs both `from = \"...\"` and `to = \"...\"`")),
  }
}

pub(crate) fn field(field: &Field, errors: &mut Errors) -> FieldAttrs {
  let ident = field.ident.as_ref().expect("named fields have names");
  let mut attrs = FieldAttrs {
    name: ident_name(ident),
    env: None,
    env_aliases: Vec::new(),
    long: None,
    short: None,
    negated: false,
    apply: None,
    secret: false,
    path: false,
    doc: doc_comment(&field.attrs),
    kind: None,
    user_keyed: false,
    skip: false,
  };
  let mut doc_given = false;
  for attr in &field.attrs {
    if attr.path().is_ident("cascade") {
      let parsed = attr.parse_nested_meta(|meta| cascade_meta(&meta, &mut attrs, &mut doc_given));
      if let Err(e) = parsed {
        errors.push(e);
      }
    } else if attr.path().is_ident("serde") {
      let parsed = attr.parse_nested_meta(|meta| serde_meta(&meta, &mut attrs));
      if let Err(e) = parsed {
        errors.push(e);
      }
    }
  }
  if attrs.user_keyed && attrs.kind.is_some() {
    errors.push(syn::Error::new_spanned(
      ident,
      "a user_keyed map is its own kind, drop `kind`",
    ));
  }
  if attrs.user_keyed && (attrs.env.is_some() || attrs.long.is_some()) {
    errors.push(syn::Error::new_spanned(
      ident,
      "a user_keyed map can only be set in files, drop `env` and `cli`",
    ));
  }
  if attrs.path && attrs.user_keyed {
    errors.push(syn::Error::new_spanned(
      ident,
      "a user_keyed map is not a path, mark the paths inside its entries",
    ));
  }
  if attrs.negated && attrs.long.is_none() {
    errors.push(syn::Error::new_spanned(
      ident,
      "`negated` says what a `cli` flag does, give the flag",
    ));
  }
  if !attrs.env_aliases.is_empty() && attrs.env.is_none() {
    errors.push(syn::Error::new_spanned(
      ident,
      "`env_alias` needs an `env` it is an alias of",
    ));
  }
  attrs
}

fn cascade_meta(
  meta: &ParseNestedMeta,
  attrs: &mut FieldAttrs,
  doc_given: &mut bool,
) -> syn::Result<()> {
  let path = &meta.path;
  if path.is_ident("env") {
    let lit: LitStr = meta.value()?.parse()?;
    check_env_name(&lit)?;
    once(meta, attrs.env.is_some())?;
    attrs.env = Some(lit.value());
  } else if path.is_ident("env_alias") {
    let lit: LitStr = meta.value()?.parse()?;
    check_env_name(&lit)?;
    attrs.env_aliases.push(lit.value());
  } else if path.is_ident("cli") {
    let lit: LitStr = meta.value()?.parse()?;
    once(meta, attrs.long.is_some())?;
    let (short, long) = parse_cli(&lit)?;
    attrs.short = short;
    attrs.long = Some(long);
  } else if path.is_ident("negated") {
    once(meta, attrs.negated)?;
    attrs.negated = true;
  } else if path.is_ident("apply") {
    let lit: LitStr = meta.value()?.parse()?;
    once(meta, attrs.apply.is_some())?;
    attrs.apply = Some(pick(&lit, APPLY, "apply")?);
  } else if path.is_ident("kind") {
    let lit: LitStr = meta.value()?.parse()?;
    once(meta, attrs.kind.is_some())?;
    attrs.kind = Some(pick(&lit, KINDS, "kind")?);
  } else if path.is_ident("doc") {
    let lit: LitStr = meta.value()?.parse()?;
    once(meta, *doc_given)?;
    *doc_given = true;
    attrs.doc = lit.value();
  } else if path.is_ident("secret") {
    once(meta, attrs.secret)?;
    attrs.secret = true;
  } else if path.is_ident("path") {
    once(meta, attrs.path)?;
    attrs.path = true;
  } else if path.is_ident("user_keyed") {
    once(meta, attrs.user_keyed)?;
    attrs.user_keyed = true;
  } else {
    return Err(meta.error(
      "unknown cascade attribute, expected one of env, env_alias, cli, negated, apply, kind, doc, \
       secret, path, user_keyed",
    ));
  }
  Ok(())
}

fn serde_meta(meta: &ParseNestedMeta, attrs: &mut FieldAttrs) -> syn::Result<()> {
  if meta.path.is_ident("rename") {
    if !meta.input.peek(Token![=]) {
      return Err(meta.error("cascade needs one name for the key, use rename = \"...\""));
    }
    let lit: LitStr = meta.value()?.parse()?;
    attrs.name = lit.value();
    Ok(())
  } else if meta.path.is_ident("skip") {
    attrs.skip = true;
    Ok(())
  } else if meta.path.is_ident("flatten") {
    Err(meta.error("cascade keys need a table per section, flatten is not supported"))
  } else {
    skip_meta(meta)
  }
}

/// Consumes a serde item cascade has no use for, with its value or list if it has one.
fn skip_meta(meta: &ParseNestedMeta) -> syn::Result<()> {
  if meta.input.peek(Token![=]) {
    meta.value()?.parse::<Expr>()?;
  } else if meta.input.peek(token::Paren) {
    let content;
    syn::parenthesized!(content in meta.input);
    content.parse::<proc_macro2::TokenStream>()?;
  }
  Ok(())
}

fn once(meta: &ParseNestedMeta, seen: bool) -> syn::Result<()> {
  if seen {
    Err(meta.error("given twice"))
  } else {
    Ok(())
  }
}

fn pick(lit: &LitStr, table: &[(&str, &'static str)], what: &str) -> syn::Result<&'static str> {
  let value = lit.value();
  table
    .iter()
    .find(|(name, _)| *name == value)
    .map(|(_, variant)| *variant)
    .ok_or_else(|| {
      let names: Vec<_> = table.iter().map(|(name, _)| *name).collect();
      syn::Error::new(
        lit.span(),
        format!(
          "unknown {what} `{value}`, expected one of {}",
          names.join(", ")
        ),
      )
    })
}

fn check_env_name(lit: &LitStr) -> syn::Result<()> {
  let name = lit.value();
  let valid = name
    .chars()
    .next()
    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
  if valid {
    Ok(())
  } else {
    Err(syn::Error::new(
      lit.span(),
      format!("`{name}` is not an environment variable name (letters, digits and `_`)"),
    ))
  }
}

/// `--long` or `-s|--long`
fn parse_cli(lit: &LitStr) -> syn::Result<(Option<char>, String)> {
  let value = lit.value();
  let err = || {
    syn::Error::new(
      lit.span(),
      format!("`{value}` is not a flag, expected `--long` or `-s|--long`"),
    )
  };
  let (short, long) = match value.split_once('|') {
    Some((short, long)) => {
      let mut chars = short.strip_prefix('-').ok_or_else(err)?.chars();
      match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => (Some(c), long),
        _ => return Err(err()),
      }
    }
    None => (None, value.as_str()),
  };
  let name = long.strip_prefix("--").ok_or_else(err)?;
  let valid = name
    .chars()
    .next()
    .is_some_and(|c| c.is_ascii_alphanumeric())
    && name
      .chars()
      .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
  if !valid {
    return Err(err());
  }
  Ok((short, long.to_string()))
}

fn ident_name(ident: &Ident) -> String {
  let name = ident.to_string();
  name.strip_prefix("r#").map(str::to_string).unwrap_or(name)
}

/// The doc comment's lines, trimmed and joined; a blank line starts a new paragraph.
fn doc_comment(attrs: &[Attribute]) -> String {
  let mut paragraphs: Vec<String> = vec![String::new()];
  for attr in attrs {
    let Ok(name_value) = attr.meta.require_name_value() else {
      continue;
    };
    if !attr.path().is_ident("doc") {
      continue;
    }
    let Expr::Lit(syn::ExprLit {
      lit: syn::Lit::Str(lit),
      ..
    }) = &name_value.value
    else {
      continue;
    };
    let line = lit.value();
    let line = line.trim();
    let current = paragraphs.last_mut().expect("starts with one paragraph");
    if line.is_empty() {
      if !current.is_empty() {
        paragraphs.push(String::new());
      }
    } else {
      if !current.is_empty() {
        current.push(' ');
      }
      current.push_str(line);
    }
  }
  paragraphs.retain(|p| !p.is_empty());
  paragraphs.join("\n")
}

#[cfg(test)]
mod tests {
  use super::*;
  use syn::parse_quote;

  fn one_field(field: syn::Field) -> syn::Result<FieldAttrs> {
    let mut errors = Errors::default();
    let attrs = super::field(&field, &mut errors);
    errors.finish().map(|()| attrs)
  }

  fn named(tokens: syn::FieldsNamed) -> syn::Field {
    tokens.named.into_iter().next().unwrap()
  }

  fn error(field: syn::Field) -> String {
    match one_field(field) {
      Ok(_) => panic!("expected an error"),
      Err(e) => e.to_string(),
    }
  }

  #[test]
  fn every_attribute_is_read() {
    let attrs = one_field(named(parse_quote!({
      /// The user to log in as.
      ///
      /// Second paragraph.
      #[cascade(env = "APP_USER", env_alias = "NEW_USER", cli = "-u|--user")]
      #[cascade(apply = "reconnect", secret, kind = "string", path)]
      #[serde(default, rename = "login")]
      user: String
    })))
    .unwrap();
    assert_eq!(attrs.name, "login");
    assert_eq!(attrs.env.as_deref(), Some("APP_USER"));
    assert_eq!(attrs.env_aliases, ["NEW_USER"]);
    assert_eq!(attrs.short, Some('u'));
    assert_eq!(attrs.long.as_deref(), Some("--user"));
    assert_eq!(attrs.apply, Some("Reconnect"));
    assert!(attrs.secret && attrs.path);
    assert_eq!(attrs.kind, Some("String"));
    assert_eq!(attrs.doc, "The user to log in as.\nSecond paragraph.");
    assert!(!attrs.user_keyed && !attrs.skip && !attrs.negated);
    let off = one_field(named(parse_quote!({
      #[cascade(cli = "--no-mapping", negated)]
      mapping: bool
    })))
    .unwrap();
    assert!(off.negated);
  }

  #[test]
  fn an_explicit_doc_replaces_the_comment() {
    let attrs = one_field(named(parse_quote!({
      /// from the comment
      #[cascade(doc = "given")]
      r#type: String
    })))
    .unwrap();
    assert_eq!(attrs.doc, "given");
    assert_eq!(attrs.name, "type");
  }

  #[test]
  fn bad_attributes_say_what_is_wrong() {
    let cases: Vec<(syn::Field, &str)> = vec![
      (
        named(parse_quote!({ #[cascade(color = "red")] a: u8 })),
        "unknown cascade attribute",
      ),
      (
        named(parse_quote!({ #[cascade(apply = "sometimes")] a: u8 })),
        "unknown apply `sometimes`, expected one of live, reconnect, restart",
      ),
      (
        named(parse_quote!({ #[cascade(kind = "color")] a: u8 })),
        "unknown kind `color`",
      ),
      (
        named(parse_quote!({ #[cascade(cli = "debug")] a: bool })),
        "`debug` is not a flag",
      ),
      (
        named(parse_quote!({ #[cascade(cli = "-dx|--debug")] a: bool })),
        "`-dx|--debug` is not a flag",
      ),
      (
        named(parse_quote!({ #[cascade(cli = "--Debug")] a: bool })),
        "`--Debug` is not a flag",
      ),
      (
        named(parse_quote!({ #[cascade(env = "NOT-A-VAR")] a: u8 })),
        "`NOT-A-VAR` is not an environment variable name",
      ),
      (
        named(parse_quote!({ #[cascade(env = "A", env = "B")] a: u8 })),
        "given twice",
      ),
      (
        named(parse_quote!({ #[cascade(env_alias = "B")] a: u8 })),
        "`env_alias` needs an `env`",
      ),
      (
        named(parse_quote!({ #[cascade(negated)] a: bool })),
        "`negated` says what a `cli` flag does",
      ),
      (
        named(parse_quote!({ #[cascade(user_keyed, kind = "string")] a: Map })),
        "a user_keyed map is its own kind",
      ),
      (
        named(parse_quote!({ #[cascade(user_keyed, env = "A")] a: Map })),
        "a user_keyed map can only be set in files",
      ),
      (
        named(parse_quote!({ #[cascade(user_keyed, path)] a: Map })),
        "a user_keyed map is not a path",
      ),
      (
        named(parse_quote!({ #[serde(flatten)] a: Inner })),
        "flatten is not supported",
      ),
      (
        named(parse_quote!({ #[serde(rename(serialize = "x"))] a: u8 })),
        "use rename = \"...\"",
      ),
    ];
    for (field, expected) in cases {
      let message = error(field);
      assert!(message.contains(expected), "{message:?} lacks {expected:?}");
    }
  }

  #[test]
  fn serde_items_cascade_ignores_are_skipped_whole() {
    let attrs = one_field(named(parse_quote!({
      #[serde(default = "five", with = "humantime_serde", skip_serializing_if = "Option::is_none")]
      #[serde(alias = "old")]
      a: u8
    })))
    .unwrap();
    assert_eq!(attrs.name, "a");
    let skipped = one_field(named(parse_quote!({ #[serde(skip)] a: u8 }))).unwrap();
    assert!(skipped.skip);
  }

  #[test]
  fn container_attributes() {
    let mut errors = Errors::default();
    let container = super::container(
      &[parse_quote!(#[cascade(env_alias_prefix(from = "OLD_", to = "NEW_"))])],
      &mut errors,
    );
    errors.finish().unwrap();
    assert_eq!(
      container.alias_prefixes,
      [("OLD_".to_string(), "NEW_".to_string())]
    );

    for (attr, expected) in [
      (
        parse_quote!(#[cascade(env_alias_prefix(from = "OLD_"))]),
        "needs both",
      ),
      (
        parse_quote!(#[serde(rename_all = "camelCase")]),
        "rename fields one by one",
      ),
      (
        parse_quote!(#[cascade(secret)]),
        "unknown cascade attribute on a struct",
      ),
    ] {
      let mut errors = Errors::default();
      super::container(&[attr], &mut errors);
      let message = errors.finish().unwrap_err().to_string();
      assert!(message.contains(expected), "{message:?} lacks {expected:?}");
    }
  }
}
