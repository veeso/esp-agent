//! Expands `#[derive(TurnCommands)]` into a whole-input command parser.

use std::collections::HashSet;

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Token, Type, Variant};

/// The most captures one pattern can hold, bounding the depth of the runtime search.
const MAX_CAPTURES: usize = 8;

/// The name that a pattern uses for the target of the command.
const TARGET: &str = "target";

/// One piece of a command pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Piece {
    /// Text that the input must repeat, ignoring ASCII case.
    Literal(String),
    /// A named capture such as `{target}`.
    Capture(String),
}

/// A validated pattern: the literals around its captures and which name each capture has.
struct Pattern {
    /// One more literal than there are captures. Only the first and last can be empty.
    literals: Vec<String>,
    /// The name of each capture, in order of appearance.
    captures: Vec<String>,
}

/// One action variant with the patterns that spell it.
struct Action<'a> {
    variant: &'a Ident,
    fields: Vec<(&'a Ident, &'a Type)>,
    patterns: Vec<Pattern>,
}

/// Expands the derive input into the `TurnCommands` implementation.
///
/// Every problem is an error that points at the offending attribute or field.
pub(crate) fn expand(input: proc_macro2::TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "TurnCommands can only be derived for an enum",
        ));
    };
    let previous = previous_word(&input)?;

    let mut seen = HashSet::new();
    let mut actions = Vec::new();
    for variant in &data.variants {
        if let Some(action) = parse_action(variant, &mut seen)? {
            actions.push(action);
        }
    }

    let library = library_path();
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let previous = previous.map_or_else(
        || quote!(::core::option::Option::None),
        |word| quote!(::core::option::Option::Some(#word)),
    );
    let alternatives = actions
        .iter()
        .flat_map(|action| {
            action
                .patterns
                .iter()
                .map(|pattern| alternative(&library, &previous, action, pattern))
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #library::TurnCommands for #ident #type_generics #where_clause {
            fn parse_command(
                __input: &::core::primitive::str,
            ) -> ::core::option::Option<#library::Command<'_, Self>> {
                let __input = #library::_private::trim_sentence(__input);
                let mut __found: ::core::option::Option<#library::Command<'_, Self>> =
                    ::core::option::Option::None;
                #(#alternatives)*
                __found
            }
        }
    })
}

/// Generates the matching of one pattern against the whole input.
fn alternative(
    library: &TokenStream,
    previous: &TokenStream,
    action: &Action<'_>,
    pattern: &Pattern,
) -> TokenStream {
    let variant = action.variant;
    let fields = &action.fields;
    let bindings: Vec<_> = (0..fields.len())
        .map(|i| format_ident!("__field_{i}", span = Span::mixed_site()))
        .collect();
    let names = fields.iter().map(|(name, _)| name);
    let capture_match = quote!(#library::_private::CaptureMatch);
    let conversions = pattern
        .captures
        .iter()
        .enumerate()
        .map(|(position, capture)| {
            if capture == TARGET {
                return quote!(#position => __target = __part);
            }
            let i = fields
                .iter()
                .position(|(name, _)| *name == capture.as_str())
                .expect("a validated capture names a field");
            let ty = fields[i].1;
            let binding = &bindings[i];
            quote! {
                #position => {
                    #binding = <#ty as ::core::str::FromStr>::from_str(__part).ok();
                    if #binding.is_none() {
                        return #capture_match::Reject;
                    }
                }
            }
        });
    let literals = &pattern.literals;
    let last = pattern.captures.len() - 1;
    quote! {
        {
            let mut __target = "";
            #(let mut #bindings = ::core::option::Option::None;)*
            if #library::_private::split(__input, &[#(#literals),*], |__index, __part| {
                match __index {
                    #(#conversions,)*
                    _ => ::core::unreachable!(),
                }
                if __index == #last {
                    if __found.is_some() {
                        return #capture_match::Ambiguous;
                    }
                    // Every field was accepted along this path. Only the first match needs
                    // to move the values, so shared prefixes never require Clone.
                    __found = ::core::option::Option::Some(#library::Command {
                        action: Self::#variant { #(#names: #bindings.take().expect("matched field")),* },
                        target: #library::_private::selector(__target, #previous),
                    });
                }
                #capture_match::Accept
            }) {
                return ::core::option::Option::None;
            }
        }
    }
}

/// Reads the optional `#[commands(previous = "word")]` attribute of the enum.
fn previous_word(input: &DeriveInput) -> syn::Result<Option<LitStr>> {
    let mut previous = None;
    for attribute in input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("commands"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("previous") {
                previous = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            } else {
                Err(meta.error("expected `previous = \"word\"`"))
            }
        })?;
    }
    Ok(previous)
}

/// Reads the patterns of a variant, or `None` for a variant that has no `#[command]`.
fn parse_action<'a>(
    variant: &'a Variant,
    seen: &mut HashSet<Vec<Piece>>,
) -> syn::Result<Option<Action<'a>>> {
    let mut strings = Vec::new();
    for attribute in variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("command"))
    {
        let patterns =
            attribute.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)?;
        if patterns.is_empty() {
            return Err(syn::Error::new_spanned(
                attribute,
                "a command needs at least one pattern",
            ));
        }
        strings.extend(patterns);
    }
    if strings.is_empty() {
        return Ok(None);
    }
    let fields = match &variant.fields {
        Fields::Unit => Vec::new(),
        Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let name = field.ident.as_ref().expect("a named field has a name");
                if name == TARGET {
                    return Err(syn::Error::new(
                        name.span(),
                        "the field name `target` is reserved for the {target} capture",
                    ));
                }
                Ok((name, &field.ty))
            })
            .collect::<syn::Result<Vec<_>>>()?,
        Fields::Unnamed(_) => {
            return Err(syn::Error::new(
                variant.ident.span(),
                "a variant with commands must be a unit or have named fields",
            ));
        }
    };
    let mut patterns = Vec::new();
    for string in &strings {
        let pieces = parse_pieces(string)?;
        let pattern = validate(string, &pieces, &fields, variant)?;
        let key = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Literal(text) => Piece::Literal(text.to_ascii_lowercase()),
                capture @ Piece::Capture(_) => capture.clone(),
            })
            .collect::<Vec<_>>();
        if !seen.insert(key) {
            return Err(syn::Error::new(
                string.span(),
                format!("duplicate pattern \"{pattern}\"", pattern = string.value()),
            ));
        }
        patterns.push(pattern);
    }
    Ok(Some(Action {
        variant: &variant.ident,
        fields,
        patterns,
    }))
}

/// Splits a pattern string into literals and `{name}` captures.
fn parse_pieces(string: &LitStr) -> syn::Result<Vec<Piece>> {
    // The input loses its outer whitespace and trailing sentence punctuation before it is
    // matched, so a pattern that kept them could never match.
    let value = string.value();
    let value = value.trim().trim_end_matches(['.', '!', '?']).trim_end();
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => {
                            return Err(syn::Error::new(string.span(), "unclosed capture `{`"));
                        }
                    }
                }
                if name.is_empty() {
                    return Err(syn::Error::new(string.span(), "empty capture `{}`"));
                }
                if !literal.is_empty() {
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                }
                pieces.push(Piece::Capture(name));
            }
            '}' => {
                return Err(syn::Error::new(
                    string.span(),
                    "unmatched `}`, write `}}` for a literal brace",
                ));
            }
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    Ok(pieces)
}

/// Checks the captures of one pattern against the fields of its variant.
fn validate(
    string: &LitStr,
    pieces: &[Piece],
    fields: &[(&Ident, &Type)],
    variant: &Variant,
) -> syn::Result<Pattern> {
    let error = |message: String| syn::Error::new(string.span(), message);
    let mut literals = vec![String::new()];
    let mut captures: Vec<String> = Vec::new();
    let mut after_capture = false;
    for piece in pieces {
        match piece {
            Piece::Literal(text) => {
                literals
                    .last_mut()
                    .expect("there is a literal")
                    .push_str(text);
                after_capture = false;
            }
            Piece::Capture(name) => {
                if after_capture {
                    return Err(error(format!(
                        "adjacent captures in \"{pattern}\" need a literal between them",
                        pattern = string.value()
                    )));
                }
                if name != TARGET && !fields.iter().any(|(field, _)| *field == name.as_str()) {
                    return Err(error(format!(
                        "unknown field `{name}` in \"{pattern}\"",
                        pattern = string.value()
                    )));
                }
                if captures.contains(name) {
                    return Err(error(format!("repeated capture `{{{name}}}`")));
                }
                captures.push(name.clone());
                literals.push(String::new());
                after_capture = true;
            }
        }
    }
    if !captures.iter().any(|name| name == TARGET) {
        return Err(error(format!(
            "\"{pattern}\" needs a {{target}} capture",
            pattern = string.value()
        )));
    }
    if let Some((field, _)) = fields
        .iter()
        .find(|(field, _)| !captures.iter().any(|name| *field == name.as_str()))
    {
        return Err(syn::Error::new(
            field.span(),
            format!(
                "missing field `{field}` in \"{pattern}\" of `{variant}`",
                pattern = string.value(),
                variant = variant.ident
            ),
        ));
    }
    if captures.len() > MAX_CAPTURES {
        return Err(error(format!(
            "a pattern holds at most {MAX_CAPTURES} captures"
        )));
    }
    Ok(Pattern { literals, captures })
}

/// Returns the path of the library from the point of view of the crate being compiled.
fn library_path() -> TokenStream {
    match crate_name("esp-agent") {
        Ok(FoundCrate::Itself)
            if std::env::var("CARGO_CRATE_NAME").is_ok_and(|name| name == "esp_agent") =>
        {
            quote!(crate)
        }
        Ok(FoundCrate::Name(name)) => {
            let name = format_ident!("{name}", span = Span::call_site());
            quote!(::#name)
        }
        _ => quote!(::esp_agent),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commands_report_invalid_patterns() {
        for (patterns, reason) in [
            ("", "at least one pattern"),
            ("\"dim {target} {missing}\"", "unknown field"),
            ("\"dim {target}\"", "missing field"),
            ("\"dim {target}{level}\"", "adjacent"),
            ("\"dim {level}\"", "target"),
            ("\"dim {target} {level} {level}\"", "repeated"),
            (
                "\"dim {target} {level}\", \"DIM {target} {level}\"",
                "duplicate",
            ),
        ] {
            let source = format!("enum Op {{ #[command({patterns})] Dim {{ level: u8 }} }}");
            assert!(
                super::expand(source.parse().unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains(reason)
            );
        }
    }

    #[test]
    fn commands_report_invalid_variant_declarations() {
        for (variant, reason) in [
            (
                r#"#[command("pick {target}")] Pick { target: u8 }"#,
                "the field name `target` is reserved",
            ),
            (
                r#"#[command("pick {target}")] Pick(u8)"#,
                "a variant with commands must be a unit or have named fields",
            ),
            (
                r#"#[command("{target} {a} {b} {c} {d} {e} {f} {g} {h}")]
                   Pick { a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8 }"#,
                "a pattern holds at most 8 captures",
            ),
        ] {
            let source = format!("enum Op {{ {variant} }}");
            let error = super::expand(source.parse().unwrap()).unwrap_err();
            assert!(error.to_string().contains(reason), "{variant}: {error}");
        }
    }
}
