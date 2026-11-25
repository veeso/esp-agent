//! Expands the arguments of `target!` into a `DynamicTarget` expression.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Expr, Ident, Token, parenthesized};

/// One comma-separated argument: positional, or `key = value`.
enum Argument {
    Positional(Expr),
    Named(Ident, Expr),
}

/// The hygienic type path and the arguments forwarded by `target!`.
struct Arguments {
    target_type: TokenStream,
    arguments: Vec<Argument>,
}

impl Parse for Arguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path;
        parenthesized!(path in input);
        let target_type = path.parse()?;
        let mut arguments = Vec::new();
        while !input.is_empty() {
            // `flag == 1` starts with an identifier and a `=` that belongs to `==`.
            if input.peek(Ident) && input.peek2(Token![=]) && !input.peek2(Token![==]) {
                let key: Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                arguments.push(Argument::Named(key, input.parse()?));
            } else {
                arguments.push(Argument::Positional(input.parse()?));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            target_type,
            arguments,
        })
    }
}

/// Compiles the arguments of `target!` into a `DynamicTarget` expression.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    const FIELDS: [&str; 3] = ["id", "name", "replies"];
    let Arguments {
        target_type,
        arguments,
    } = syn::parse2(input)?;
    let types = [
        quote!(::core::primitive::u32),
        quote!(impl ::core::convert::Into<alloc::string::String>),
        quote!(
            impl ::core::iter::IntoIterator<Item = impl ::core::convert::Into<alloc::string::String>>
        ),
    ];
    let mut field_bindings = [None, None, None];
    let mut bindings = Vec::new();
    let mut argument_types = Vec::new();
    let mut expressions = Vec::new();
    let mut named = false;
    let mut positional = 0;
    for argument in arguments {
        let (index, key, value) = match argument {
            Argument::Positional(value) => {
                if named {
                    return Err(syn::Error::new(
                        value.span(),
                        "positional arguments must come before named arguments",
                    ));
                }
                if positional == FIELDS.len() {
                    return Err(syn::Error::new(
                        value.span(),
                        "expected at most three positional arguments: id, name, replies",
                    ));
                }
                positional += 1;
                (positional - 1, FIELDS[positional - 1], value)
            }
            Argument::Named(key, value) => {
                named = true;
                let field_name = key.unraw();
                let Some(index) = FIELDS.iter().position(|field| field_name == field) else {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown field `{key}`, expected `id`, `name`, or `replies`"),
                    ));
                };
                (index, FIELDS[index], value)
            }
        };
        let binding = format_ident!("{key}");
        if field_bindings[index].replace(binding.clone()).is_some() {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("`{key}` is given twice"),
            ));
        }
        bindings.push(binding);
        argument_types.push(&types[index]);
        expressions.push(value);
    }
    let [id, name, replies] = field_bindings;
    let Some(id) = id else {
        return Err(syn::Error::new(
            Span::call_site(),
            "missing the device ID: pass it first or as `id = ..`",
        ));
    };
    let Some(name) = name else {
        return Err(syn::Error::new(
            Span::call_site(),
            "missing the device name: pass it second or as `name = ..`",
        ));
    };
    let replies = replies.map(|replies| quote!(.with_replies(#replies)));
    // A module keeps caller constants out of the pattern's scope. The function argument
    // evaluates in caller order and keeps borrowed temporaries alive through construction.
    Ok(quote!(({
        mod __esp_agent_target {
            extern crate alloc;

            pub(super) fn build((#(#bindings,)*): (#(#argument_types,)*)) -> #target_type {
                #target_type::new(#id, #name) #replies
            }
        }
        __esp_agent_target::build
    })((#(#expressions,)*))))
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;

    fn expand(source: &str) -> syn::Result<TokenStream> {
        let arguments: TokenStream = source.parse().unwrap();
        super::expand(quote!((DynamicTarget) #arguments))
    }

    fn error(source: &str) -> String {
        expand(source).unwrap_err().to_string()
    }

    #[test]
    fn target_reports_invalid_arguments() {
        for (source, reason) in [
            ("", "missing the device ID"),
            ("name = \"lamp\"", "missing the device ID"),
            ("1", "missing the device name"),
            ("id = 1, replies = []", "missing the device name"),
            (
                "1, \"lamp\", [], extra",
                "at most three positional arguments",
            ),
            ("1, \"lamp\", id = 2", "`id` is given twice"),
            (
                "id = 1, name = \"a\", name = \"b\"",
                "`name` is given twice",
            ),
            ("1, \"a\", [], replies = []", "`replies` is given twice"),
            (
                "1, name = \"a\", []",
                "positional arguments must come before named arguments",
            ),
            ("1, \"a\", aliases = []", "unknown field `aliases`"),
        ] {
            let diagnostic = error(source);
            assert!(diagnostic.contains(reason), "{source}: {diagnostic}");
        }
    }

    #[test]
    fn target_treats_comparisons_as_positional() {
        let expanded = expand("flag == 1, \"a\"").unwrap().to_string();
        assert!(expanded.contains("flag == 1"), "{expanded}");
        assert!(!expanded.contains("with_replies"), "{expanded}");
    }

    #[test]
    fn target_rejects_duplicate_raw_and_plain_fields() {
        for source in [
            "id = 1, r#name = \"a\", name = \"b\"",
            "id = 1, name = \"a\", r#name = \"b\"",
        ] {
            assert_eq!(error(source), "`name` is given twice", "{source}");
        }
    }
}
