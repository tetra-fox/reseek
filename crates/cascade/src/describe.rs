//! How a type appears in a configuration: the [`Cascade`] trait, implemented by
//! `#[derive(Cascade)]` for sections and here for the std types that are leaves.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

/// What kind of value a leaf key holds; env vars and command-line values are read as this kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
  /// `true` or `false`, in any case from an env var or flag
  Bool,
  /// a whole number
  Integer,
  /// a number with a fraction
  Float,
  /// text, kept as written
  String,
  /// written as a human-readable string such as `"15m"`; the application's type parses it
  Duration,
  /// a byte count, written as an integer or a string such as `"10 MiB"`; the application's type
  /// parses it
  Size,
  /// a list, replaced whole by the layer that sets it
  List(Box<Kind>),
  /// a table whose keys the user picks, replaced whole by the layer that sets it
  Map,
  /// a table that is an element of a list
  Table,
}

/// When a changed value takes effect in a running application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum Apply {
  /// at once
  #[default]
  Live,
  /// when the application next reconnects to whatever the value configures
  Reconnect,
  /// when the application restarts; until then it runs with the value it started with
  Restart,
}

/// A type that can be a field of a configuration struct: a section (a struct deriving
/// `Cascade`) or a leaf.
///
/// Implemented for `bool`, the integer and float types, `String`, `PathBuf`, the `std::net`
/// address types, `std::time::Duration`, and `Vec` and `Option` of those. A leaf type of the
/// application's own, such as an enum written as a string, either implements it with
/// [`Shape::leaf`] or is given `#[cascade(kind = "...")]` where it is used.
#[diagnostic::on_unimplemented(
  message = "`{Self}` is neither a section nor a known leaf of a configuration",
  note = "derive Cascade on a section, mark a map `#[cascade(user_keyed)]`, or name a leaf's kind \
          with `#[cascade(kind = \"...\")]`"
)]
pub trait Cascade {
  /// How the type appears in a configuration.
  fn shape() -> Shape;
}

/// How a type appears in a configuration: a leaf of one [`Kind`], or a section of fields.
#[derive(Clone, Debug)]
pub struct Shape(pub(crate) Inner);

#[derive(Clone, Debug)]
pub(crate) enum Inner {
  /// `entries` are the fields of each element when the leaf is a list of tables or a
  /// user-keyed map of tables
  Leaf {
    kind: Kind,
    entries: Vec<Node>,
  },
  Section(Vec<Node>),
}

impl Shape {
  /// A leaf of `kind`.
  pub fn leaf(kind: Kind) -> Self {
    Self(Inner::Leaf {
      kind,
      entries: Vec::new(),
    })
  }
}

/// A field as the derive describes it, before the schema adds its full key and default.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct Node {
  pub(crate) key: String,
  pub(crate) kind: Kind,
  pub(crate) env: Option<String>,
  pub(crate) env_aliases: Vec<String>,
  pub(crate) long: Option<String>,
  pub(crate) short: Option<char>,
  pub(crate) apply: Option<Apply>,
  pub(crate) secret: bool,
  pub(crate) doc: String,
  pub(crate) entries: Vec<Node>,
}

impl Node {
  fn leaf(attrs: &private::Attrs, kind: Kind, entries: Vec<Node>) -> Self {
    Self {
      key: attrs.name.to_string(),
      kind,
      env: attrs.env.map(str::to_string),
      env_aliases: attrs.env_aliases.iter().map(|a| a.to_string()).collect(),
      long: attrs.long.map(str::to_string),
      short: attrs.short,
      apply: attrs.apply,
      secret: attrs.secret,
      doc: attrs.doc.to_string(),
      entries,
    }
  }

  /// This node as a field of the section `section`, which passes its apply class and secrecy
  /// down to fields that do not set their own.
  fn under(mut self, section: &private::Attrs) -> Self {
    self.key = format!("{}.{}", section.name, self.key);
    self.apply = self.apply.or(section.apply);
    self.secret |= section.secret;
    self
  }
}

/// What the derive's generated code calls; not part of the public interface.
#[doc(hidden)]
pub mod private {
  use super::{Apply, Cascade, Inner, Kind, Node, Shape};

  /// The attributes of one field, as the derive read them.
  pub struct Attrs {
    pub name: &'static str,
    pub env: Option<&'static str>,
    pub env_aliases: &'static [&'static str],
    pub long: Option<&'static str>,
    pub short: Option<char>,
    pub apply: Option<Apply>,
    pub secret: bool,
    pub doc: &'static str,
  }

  /// A map whose keys the user picks.
  pub trait UserKeyed {
    /// the type of each entry
    type Value: Cascade;
  }

  impl<V: Cascade> UserKeyed for std::collections::BTreeMap<String, V> {
    type Value = V;
  }

  impl<V: Cascade, S> UserKeyed for std::collections::HashMap<String, V, S> {
    type Value = V;
  }

  /// Adds a field whose type says whether it is a leaf or a section.
  ///
  /// # Panics
  ///
  /// When a section is given an env var or a flag; those go on its leaves.
  pub fn push<F: Cascade>(nodes: &mut Vec<Node>, attrs: Attrs) {
    match F::shape().0 {
      Inner::Leaf { kind, entries } => nodes.push(Node::leaf(&attrs, kind, entries)),
      Inner::Section(children) => {
        assert!(
          attrs.env.is_none() && attrs.long.is_none() && attrs.env_aliases.is_empty(),
          "`{}` is a section: env vars and flags go on its fields",
          attrs.name
        );
        nodes.extend(children.into_iter().map(|child| child.under(&attrs)));
      }
    }
  }

  /// Adds a leaf whose kind the attributes name.
  pub fn push_kind(nodes: &mut Vec<Node>, kind: Kind, attrs: Attrs) {
    nodes.push(Node::leaf(&attrs, kind, Vec::new()));
  }

  /// Adds a map whose keys the user picks, tracked as one value.
  pub fn push_keyed<F: UserKeyed>(nodes: &mut Vec<Node>, attrs: Attrs) {
    let entries = match <F::Value as Cascade>::shape().0 {
      Inner::Section(children) => children,
      Inner::Leaf { .. } => Vec::new(),
    };
    nodes.push(Node::leaf(&attrs, Kind::Map, entries));
  }

  /// The shape of a struct with these fields. `prefixes` are `(from, to)` pairs: every env var
  /// named `from...` is also read as `to...`, after the aliases the field names itself.
  pub fn section(mut nodes: Vec<Node>, prefixes: &[(&str, &str)]) -> Shape {
    for node in &mut nodes {
      let Some(env) = &node.env else {
        continue;
      };
      for (from, to) in prefixes {
        if let Some(rest) = env.strip_prefix(from) {
          let alias = format!("{to}{rest}");
          if !node.env_aliases.contains(&alias) {
            node.env_aliases.push(alias);
          }
        }
      }
    }
    Shape(Inner::Section(nodes))
  }
}

macro_rules! leaves {
  ($kind:ident: $($ty:ty),*) => {
    $(impl Cascade for $ty {
      fn shape() -> Shape {
        Shape::leaf(Kind::$kind)
      }
    })*
  };
}

leaves!(Bool: bool);
leaves!(Integer: i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
leaves!(Float: f32, f64);
leaves!(String: String, PathBuf, IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr);
leaves!(Duration: Duration);

impl<T: Cascade> Cascade for Vec<T> {
  fn shape() -> Shape {
    let (item, entries) = match T::shape().0 {
      Inner::Leaf { kind, entries } => (kind, entries),
      Inner::Section(fields) => (Kind::Table, fields),
    };
    Shape(Inner::Leaf {
      kind: Kind::List(Box::new(item)),
      entries,
    })
  }
}

/// An optional value has the shape of the value; `None` is written as null.
impl<T: Cascade> Cascade for Option<T> {
  fn shape() -> Shape {
    T::shape()
  }
}
