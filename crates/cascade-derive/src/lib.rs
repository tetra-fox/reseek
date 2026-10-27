//! `#[derive(Cascade)]`: describes a configuration struct's fields (their keys, kinds, env vars,
//! flags, apply classes, secrecy and docs) so `cascade` can build its schema. Use it through the
//! `cascade` crate, which re-exports it.

#![warn(missing_docs)]

mod attrs;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, parse_macro_input};

use attrs::{Errors, FieldAttrs};

/// Describes a configuration struct for `cascade`. See the `cascade` crate for the attributes.
#[proc_macro_derive(Cascade, attributes(cascade))]
pub fn derive_cascade(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  expand(&input)
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
  let mut errors = Errors::default();
  if !input.generics.params.is_empty() {
    errors.push(syn::Error::new_spanned(
      &input.generics,
      "a configuration struct can not be generic",
    ));
  }
  let fields = match &input.data {
    Data::Struct(data) => match &data.fields {
      Fields::Named(named) => named.named.iter().collect(),
      _ => {
        errors.push(syn::Error::new_spanned(
          &input.ident,
          "a configuration struct needs named fields, one per key",
        ));
        Vec::new()
      }
    },
    _ => {
      errors.push(syn::Error::new_spanned(
        &input.ident,
        "Cascade derives for structs; give a leaf enum `#[cascade(kind = \"string\")]` where it \
         is used, or implement `cascade::Cascade` for it",
      ));
      Vec::new()
    }
  };
  let container = attrs::container(&input.attrs, &mut errors);
  let pushes: Vec<TokenStream> = fields
    .iter()
    .filter_map(|field| {
      let attrs = attrs::field(field, &mut errors);
      (!attrs.skip).then(|| push(&field.ty, &attrs))
    })
    .collect();
  errors.finish()?;

  let ident = &input.ident;
  let prefixes = container
    .alias_prefixes
    .iter()
    .map(|(from, to)| quote!((#from, #to)));
  Ok(quote! {
    impl ::cascade::Cascade for #ident {
      fn shape() -> ::cascade::Shape {
        let mut nodes = ::std::vec::Vec::new();
        #(#pushes)*
        ::cascade::__private::section(nodes, &[#(#prefixes),*])
      }
    }
  })
}

/// The statement that adds one field to the struct's shape.
fn push(ty: &syn::Type, attrs: &FieldAttrs) -> TokenStream {
  let name = &attrs.name;
  let env = option(attrs.env.as_deref());
  let aliases = &attrs.env_aliases;
  let long = option(attrs.long.as_deref());
  let short = match attrs.short {
    Some(c) => quote!(::std::option::Option::Some(#c)),
    None => quote!(::std::option::Option::None),
  };
  let apply = match attrs.apply {
    Some(variant) => {
      let variant = Ident::new(variant, Span::call_site());
      quote!(::std::option::Option::Some(::cascade::Apply::#variant))
    }
    None => quote!(::std::option::Option::None),
  };
  let secret = attrs.secret;
  let negated = attrs.negated;
  let doc = &attrs.doc;
  let fields = quote! {
    ::cascade::__private::Attrs {
      name: #name,
      env: #env,
      env_aliases: &[#(#aliases),*],
      long: #long,
      short: #short,
      negated: #negated,
      apply: #apply,
      secret: #secret,
      doc: #doc,
    }
  };
  if let Some(kind) = attrs.kind {
    let kind = Ident::new(kind, Span::call_site());
    quote!(::cascade::__private::push_kind(&mut nodes, ::cascade::Kind::#kind, #fields);)
  } else if attrs.user_keyed {
    quote!(::cascade::__private::push_keyed::<#ty>(&mut nodes, #fields);)
  } else {
    quote!(::cascade::__private::push::<#ty>(&mut nodes, #fields);)
  }
}

fn option(value: Option<&str>) -> TokenStream {
  match value {
    Some(value) => quote!(::std::option::Option::Some(#value)),
    None => quote!(::std::option::Option::None),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use syn::parse_quote;

  fn error(input: DeriveInput) -> String {
    expand(&input).unwrap_err().to_string()
  }

  #[test]
  fn only_plain_named_structs() {
    assert!(
      error(parse_quote!(
        struct A<T> {
          a: T,
        }
      ))
      .contains("can not be generic")
    );
    assert!(
      error(parse_quote!(
        struct A(u8);
      ))
      .contains("needs named fields")
    );
    assert!(
      error(parse_quote!(
        enum A {
          B,
        }
      ))
      .contains("Cascade derives for structs")
    );
  }

  #[test]
  fn every_field_error_is_reported_at_once() {
    let message = error(parse_quote!(
      struct A {
        #[cascade(apply = "never")]
        a: u8,
        #[cascade(kind = "colour")]
        b: u8,
      }
    ));
    assert!(message.contains("unknown apply"), "{message}");
    let all = expand(&parse_quote!(
      struct A {
        #[cascade(apply = "never")]
        a: u8,
        #[cascade(kind = "colour")]
        b: u8,
      }
    ))
    .unwrap_err();
    assert_eq!(all.into_iter().count(), 2);
  }

  #[test]
  fn expansion_routes_each_field_by_its_attributes() {
    let tokens = expand(&parse_quote!(
      #[cascade(env_alias_prefix(from = "OLD_", to = "NEW_"))]
      struct A {
        plain: u8,
        #[cascade(kind = "size")]
        sized: Bytes,
        #[cascade(user_keyed)]
        keyed: Map,
        #[serde(skip)]
        hidden: u8,
      }
    ))
    .unwrap()
    .to_string();
    assert!(tokens.contains("push :: < u8 >"), "{tokens}");
    assert!(tokens.contains("push_kind (& mut nodes , :: cascade :: Kind :: Size"));
    assert!(tokens.contains("push_keyed :: < Map >"));
    assert!(!tokens.contains("hidden"));
    assert!(tokens.contains("(\"OLD_\" , \"NEW_\")"));
  }
}
