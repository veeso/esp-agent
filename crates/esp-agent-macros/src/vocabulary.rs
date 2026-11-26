//! Expands `vocabulary!` into a constant `Vocabulary` expression.

use std::collections::HashSet;

use esp_agent_patterns::{Normalization, Piece, TARGET, TargetRule};
use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Field, Ident, LitStr, Path, Token, Type, braced, bracketed};

use crate::commands::{Action, Bindings, alternative, parse_pattern, parser_body};

/// One action variant with its clauses.
struct Entry {
    /// The enum that the variant belongs to, such as `Action`.
    enum_path: Path,
    variant: Ident,
    fields: Vec<(Ident, Type)>,
    explicit: Option<Vec<LitStr>>,
    previous: Option<Vec<LitStr>>,
}

/// The library path from the wrapper and the user declarations.
struct Vocabulary {
    library: TokenStream,
    entries: Vec<Entry>,
}

impl Parse for Vocabulary {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let path;
        bracketed!(path in input);
        let library = path.parse()?;
        let mut entries = Vec::new();
        while !input.is_empty() {
            entries.push(input.parse()?);
            if !input.is_empty() {
                input.parse::<Token![;]>()?;
            }
        }
        Ok(Self { library, entries })
    }
}

impl Parse for Entry {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut path: Path = input.parse()?;
        if path.segments.len() < 2 {
            return Err(syn::Error::new_spanned(
                &path,
                "name the variant as `Enum::Variant`",
            ));
        }
        let variant = path
            .segments
            .pop()
            .expect("a path with two segments has a last one");
        if !variant.arguments.is_empty() {
            return Err(syn::Error::new_spanned(
                &variant.arguments,
                "put generic arguments on the enum, as in `Enum::<Type>::Variant`",
            ));
        }
        path.segments.pop_punct();
        let mut fields = Vec::new();
        if input.peek(syn::token::Brace) {
            let content;
            braced!(content in input);
            for field in
                Punctuated::<Field, Token![,]>::parse_terminated_with(&content, Field::parse_named)?
            {
                let name = field.ident.expect("a named field has a name");
                fields.push((name, field.ty));
            }
        }
        input.parse::<Token![=>]>()?;
        let mut entry = Self {
            enum_path: path,
            variant: variant.ident,
            fields,
            explicit: None,
            previous: None,
        };
        loop {
            let keyword = input.call(Ident::parse_any)?;
            let slot = if keyword == "explicit" {
                &mut entry.explicit
            } else if keyword == "previous" {
                &mut entry.previous
            } else {
                return Err(syn::Error::new(
                    keyword.span(),
                    "expected `explicit` or `previous`",
                ));
            };
            if slot.is_some() {
                return Err(syn::Error::new(
                    keyword.span(),
                    format!("repeated `{keyword}` clause"),
                ));
            }
            let content;
            bracketed!(content in input);
            let strings: Vec<_> = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?
                .into_iter()
                .collect();
            if strings.is_empty() {
                return Err(syn::Error::new(
                    keyword.span(),
                    format!("`{keyword}` needs at least one phrase"),
                ));
            }
            *slot = Some(strings);
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }
        Ok(entry)
    }
}

/// Expands the body of `vocabulary!` into a `Vocabulary` expression.
///
/// Every problem is an error that points at the offending tokens.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let source = input.to_string();
    let vocabulary: Vocabulary = syn::parse2(input)?;
    let Some(first) = vocabulary.entries.first() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "a vocabulary needs at least one entry",
        ));
    };
    let enum_path = &first.enum_path;
    let enum_name = quote!(#enum_path).to_string();

    // Only the input alias is visible to caller tokens. Don't mask a name that the
    // declaration spells directly, such as a const generic argument.
    let bindings = (0..)
        .map(|i| {
            if i == 0 {
                Bindings::new("__")
            } else {
                Bindings::new(&format!("__esp_agent_{i}_"))
            }
        })
        .find(|bindings| !source.contains(&bindings.input.to_string()))
        .expect("the declaration mentions finitely many names");
    let input = &bindings.input;
    let operations = &bindings.operations;
    let target = &bindings.target;

    let library = &vocabulary.library;
    let mut variants = HashSet::new();
    let mut seen: HashSet<Vec<Piece>> = HashSet::new();
    let mut alternatives = Vec::new();
    let mut callbacks = Vec::new();
    let mut callback_types = Vec::new();
    for (i, entry) in vocabulary.entries.iter().enumerate() {
        let entry_enum = &entry.enum_path;
        if quote!(#entry_enum).to_string() != enum_name {
            return Err(syn::Error::new_spanned(
                &entry.enum_path,
                "every entry of a vocabulary must spell the enum path identically",
            ));
        }
        if !variants.insert(entry.variant.unraw()) {
            return Err(syn::Error::new(
                entry.variant.span(),
                format!("duplicate entry for `{variant}`", variant = entry.variant),
            ));
        }
        let fields: Vec<_> = entry.fields.iter().map(|(name, ty)| (name, ty)).collect();
        let mut field_names = HashSet::new();
        for (name, _) in &fields {
            let field_name = name.unraw();
            if field_name == TARGET {
                return Err(syn::Error::new(
                    name.span(),
                    "the field name `target` is reserved for the {target} capture",
                ));
            }
            if !field_names.insert(field_name.clone()) {
                return Err(syn::Error::new(
                    name.span(),
                    format!("duplicate field `{field_name}`"),
                ));
            }
        }
        let variant = &entry.variant;
        let constructor = quote!(#enum_path::#variant);
        let i = syn::Index::from(i);
        let entry_operations = quote!(#operations.#i);
        let clauses = [
            (
                &entry.explicit,
                TargetRule::Required,
                quote!(#library::TargetSelector::Explicit(#target)),
            ),
            (
                &entry.previous,
                TargetRule::Forbidden,
                quote!(#library::TargetSelector::Previous),
            ),
        ];
        for (phrases, rule, target) in clauses {
            let Some(phrases) = phrases else {
                continue;
            };
            let mut patterns = Vec::new();
            for string in phrases {
                patterns.push(parse_pattern(
                    string,
                    &fields,
                    variant,
                    rule,
                    Normalization::Vocabulary,
                    &mut seen,
                )?);
            }
            let action = Action {
                constructor: constructor.clone(),
                fields: fields.clone(),
                patterns,
            };
            alternatives.extend(action.patterns.iter().map(|pattern| {
                alternative(
                    library,
                    &action,
                    pattern,
                    &target,
                    &bindings,
                    Some(&entry_operations),
                )
            }));
        }
        let names = fields.iter().map(|(name, _)| name);
        let indices = (0..fields.len()).map(syn::Index::from);
        let types = fields.iter().map(|(_, ty)| ty);
        let parsed_types = types.clone();
        let converter_types = fields
            .iter()
            .map(|_| quote!(fn(&::core::primitive::str) -> ::core::result::Result<_, _>));
        let constructor_parameters = fields.iter().map(|_| quote!(_));
        // Reuse the one alias already in scope rather than introducing a name per field.
        let parameter = if fields.is_empty() {
            quote!(())
        } else {
            quote!(#input)
        };
        // Parse the declared types first, then let the enum initializer coerce the values.
        callbacks.push(quote!((
            |#parameter: (#(#parsed_types,)*)| #constructor { #(#names: #input.#indices),* },
            #(<#types as ::core::str::FromStr>::from_str,)*
        )));
        callback_types
            .push(quote!((fn((#(#constructor_parameters,)*)) -> _, #(#converter_types,)*)));
    }
    let body = parser_body(
        library,
        &quote!(_),
        &alternatives,
        &bindings,
        Normalization::Vocabulary,
    );
    let locals = bindings.names().filter(|name| *name != input);

    Ok(quote! {
        #library::Vocabulary::<#enum_path>::new({
            use ::core::mem::drop as #input;
            |#input| {
                // A block-local function alias takes precedence over caller constants in
                // patterns. Resolve caller paths and types in sibling callback expressions,
                // so those aliases are only visible to the parser (not the callbacks).
                ({
                    #(use ::core::mem::drop as #locals;)*
                    |#operations: (#(#callback_types,)*)| { #body }
                })((#(#callbacks,)*))
            }
        })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn vocabulary_keeps_the_exact_phrase_messages() {
        for (body, expected) in [
            (
                r#"Op::On => previous ["on {target}"];"#,
                "a previous phrase refers to the previous target and cannot hold {target}",
            ),
            (r#"Op::On => previous ["  !"];"#, "empty phrase"),
            (
                r#"Op::On => previous ["on it", "ON IT!"];"#,
                "duplicate pattern \"ON IT!\"",
            ),
        ] {
            let error =
                super::expand(format!("[::esp_agent] {body}").parse().unwrap()).unwrap_err();
            assert_eq!(error.to_string(), expected, "{body}");
        }
    }

    #[test]
    fn vocabulary_reports_invalid_declarations() {
        for (body, reason) in [
            ("", "at least one entry"),
            (r#"TurnOn => explicit ["on {target}"];"#, "`Enum::Variant`"),
            (
                r#"Op::On::<u8> => previous ["on it"];"#,
                "`Enum::<Type>::Variant`",
            ),
            (
                r#"Op::On => explicit ["on {target}"]; Other::Off => explicit ["off {target}"];"#,
                "spell the enum path identically",
            ),
            (
                r#"Op::On => previous ["on it"]; crate::Op::Off => previous ["off it"];"#,
                "spell the enum path identically",
            ),
            (
                r#"Op::On => explicit ["on {target}"]; Op::On => previous ["on it"];"#,
                "duplicate entry",
            ),
            (
                r#"Op::On => explicit ["on {target}"]; Op::r#On => previous ["on it"];"#,
                "duplicate entry",
            ),
            (r#"Op::On => explicit [];"#, "at least one phrase"),
            (
                r#"Op::On => explicit ["on {target}"], explicit ["{target} on"];"#,
                "repeated `explicit`",
            ),
            (
                r#"Op::On => later ["on {target}"];"#,
                "expected `explicit` or `previous`",
            ),
            (
                r#"Op::On => for ["on {target}"];"#,
                "expected `explicit` or `previous`",
            ),
            (r#"Op::On => explicit ["on"];"#, "needs a {target} capture"),
            (
                r#"Op::On => previous ["on {target}"];"#,
                "cannot hold {target}",
            ),
            (r#"Op::On => previous ["  !"];"#, "empty phrase"),
            (r#"Op::On => previous ["on it", " ! ? "];"#, "empty phrase"),
            (
                r#"Op::On => previous ["basta così!\u{2003}?", "basta così"];"#,
                "duplicate pattern",
            ),
            (
                r#"Op::On => previous ["on it"]; Op::Off => previous ["ON IT"];"#,
                "duplicate pattern",
            ),
            (
                r#"Op::Dim { level: u8 } => previous ["dim it"];"#,
                "missing field",
            ),
            (
                r#"Op::Dim { r#target: u8 } => previous ["dim {r#target}"];"#,
                "the field name `target` is reserved",
            ),
            (
                r#"Op::Dim { level: u8, level: u8, level: u8, level: u8, level: u8,
                    level: u8, level: u8, level: u8, level: u8 } => previous ["impostala al {level}%"];"#,
                "duplicate field `level`",
            ),
            (
                r#"Op::Dim { level: u8, r#level: u8 } => previous ["impostala al {level}%"];"#,
                "duplicate field `level`",
            ),
            (
                r#"Op::Set { a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8, i: u8 } =>
                    previous ["{a} {b} {c} {d} {e} {f} {g} {h} {i}"];"#,
                "a pattern holds at most 8 captures",
            ),
        ] {
            let error =
                super::expand(format!("[::esp_agent] {body}").parse().unwrap()).unwrap_err();
            assert!(error.to_string().contains(reason), "{body}: {error}");
        }
    }
}
