//! Expands `#[derive(TurnCommands)]` into a whole-input command parser.

use std::collections::HashSet;

use esp_agent_patterns::{
    MAX_CAPTURES, Normalization, Pattern, PatternError, Piece, TARGET, TargetRule, parse_pieces,
    validate,
};
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Token, Type, Variant};

/// One action variant with the patterns that spell it.
pub(crate) struct Action<'a> {
    /// The expression that names the variant, such as `Self::TurnOn`.
    pub(crate) constructor: TokenStream,
    pub(crate) fields: Vec<(&'a Ident, &'a Type)>,
    pub(crate) patterns: Vec<Pattern>,
}

/// The generated locals shared by a parser and its alternatives.
pub(crate) struct Bindings {
    pub(crate) input: Ident,
    pub(crate) operations: Ident,
    found: Ident,
    pub(crate) target: Ident,
    index: Ident,
    part: Ident,
    fields: [Ident; MAX_CAPTURES],
}

impl Bindings {
    pub(crate) fn new(prefix: &str) -> Self {
        let ident = |name: &str| format_ident!("{prefix}{name}", span = Span::mixed_site());
        Self {
            input: ident("input"),
            operations: ident("operations"),
            found: ident("found"),
            target: ident("target"),
            index: ident("index"),
            part: ident("part"),
            fields: core::array::from_fn(|i| ident(&format!("field_{i}"))),
        }
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &Ident> {
        [
            &self.input,
            &self.operations,
            &self.found,
            &self.target,
            &self.index,
            &self.part,
        ]
        .into_iter()
        .chain(&self.fields)
    }
}

/// Expands the derive input into the `TurnCommands` implementation.
///
/// Every problem is an error that points at the offending attribute or field.
pub(crate) fn expand(input: proc_macro2::TokenStream) -> syn::Result<TokenStream> {
    let source = input.to_string();
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
    let mut names = Vec::new();
    for variant in &data.variants {
        if let Some(action) = parse_action(variant, &mut seen)? {
            actions.push(action);
            names.push(variant.ident.unraw().to_string());
        }
    }

    let library = library_path();
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let previous = previous.map_or_else(
        || quote!(::core::option::Option::None),
        |word| quote!(::core::option::Option::Some(#word)),
    );
    let bindings = Bindings::new("__");
    let Bindings { input, target, .. } = &bindings;
    let target = quote!(#library::_private::selector(#target, #previous));
    let alternatives = actions
        .iter()
        .flat_map(|action| {
            action
                .patterns
                .iter()
                .map(|pattern| alternative(&library, action, pattern, &target, &bindings, None))
        })
        .collect::<Vec<_>>();
    let body = parser_body(
        &library,
        &quote!(Self),
        &alternatives,
        &bindings,
        Normalization::Derive,
    );
    // These function aliases keep argument patterns from resolving to caller constants.
    // Keep them out of the names that the enum and its generic bounds refer to.
    let mut prefix = "__".to_owned();
    let parameters = loop {
        let parameters = ["action", "fields", "field", "text", "matcher"]
            .map(|name| format_ident!("{prefix}{name}", span = Span::mixed_site()));
        if parameters
            .iter()
            .all(|name| !source.contains(&name.to_string()))
        {
            break parameters;
        }
        prefix.push('_');
    };
    let named_actions = named_actions(&library, &names, &actions, &parameters);

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #library::TurnCommands for #ident #type_generics #where_clause {
            fn parse_command(
                #input: &::core::primitive::str,
            ) -> ::core::option::Option<#library::Command<'_, Self>> {
                #body
            }
        }

        const _: () = {
            #(use ::core::mem::drop as #parameters;)*
            #[automatically_derived]
            impl #impl_generics #library::NamedActions for #ident #type_generics #where_clause {
                #named_actions
            }
        };
    })
}

/// Generates the items of `NamedActions`: the table of the actions and their constructor.
fn named_actions(
    library: &TokenStream,
    names: &[String],
    actions: &[Action<'_>],
    parameters: &[Ident; 5],
) -> TokenStream {
    let [action, values, field, text, matcher] = parameters;
    let specs = names.iter().zip(actions).map(|(name, action)| {
        let fields = action.fields.iter().map(|(field, _)| field.to_string());
        quote!(#library::ActionSpec { name: #name, fields: &[#(#fields),*] })
    });
    let arms = actions.iter().enumerate().map(|(index, variant)| {
        let constructor = &variant.constructor;
        let count = variant.fields.len();
        let fields = variant.fields.iter().enumerate().map(|(i, (name, ty))| {
            quote!(#name: <#ty as ::core::str::FromStr>::from_str(#values[#i]).ok()?)
        });
        quote! {
            #index if #values.len() == #count => {
                ::core::option::Option::Some(#constructor { #(#fields),* })
            }
        }
    });
    let validators = actions.iter().enumerate().flat_map(|(index, variant)| {
        variant.fields.iter().enumerate().map(move |(position, (_, ty))| {
            quote!((#index, #position) => <#ty as ::core::str::FromStr>::from_str(#text).is_ok(),)
        })
    });
    let matchers = actions.iter().enumerate().map(|(index, variant)| {
        let constructor = &variant.constructor;
        let slots = variant
            .fields
            .iter()
            .map(|(_, ty)| quote!(::core::option::Option::<#ty>::None));
        let parsers = variant.fields.iter().enumerate().map(|(i, (_, ty))| {
            let slot = syn::Index::from(i);
            quote!(#i => {
                #values.#slot = <#ty as ::core::str::FromStr>::from_str(#text).ok();
                #values.#slot.is_some()
            })
        });
        let fields = variant.fields.iter().enumerate().map(|(i, (name, _))| {
            let slot = syn::Index::from(i);
            quote!(#name: #values.#slot.take().expect("matched field"))
        });
        quote!(#index => #matcher.typed(
            (#(#slots,)*),
            |#values, #field, #text| match #field { #(#parsers,)* _ => false },
            |#values| #constructor { #(#fields),* },
        ),)
    });
    quote! {
        const ACTIONS: &'static [#library::ActionSpec] = &[#(#specs),*];

        fn accepts_field(
            #action: ::core::primitive::usize,
            #field: ::core::primitive::usize,
            #text: &::core::primitive::str,
        ) -> ::core::primitive::bool {
            match (#action, #field) {
                #(#validators)*
                _ => false,
            }
        }

        fn match_phrase(
            #action: ::core::primitive::usize,
            #matcher: #library::_private::RuntimeMatch<'_, '_, Self>,
        ) -> ::core::primitive::bool {
            match #action {
                #(#matchers)*
                _ => false,
            }
        }

        fn build(
            #action: ::core::primitive::usize,
            #values: &[&::core::primitive::str],
        ) -> ::core::option::Option<Self> {
            match #action {
                #(#arms)*
                _ => ::core::option::Option::None,
            }
        }
    }
}

/// Generates the body of a parser that tries every alternative against the whole input.
///
/// Each alternative returns `None` when the input is ambiguous and otherwise records its
/// command in the parser's result binding.
pub(crate) fn parser_body(
    library: &TokenStream,
    action_ty: &TokenStream,
    alternatives: &[TokenStream],
    bindings: &Bindings,
    normalization: Normalization,
) -> TokenStream {
    let Bindings { input, found, .. } = bindings;
    let trim = match normalization {
        Normalization::Derive => quote!(trim_sentence),
        Normalization::Vocabulary => quote!(trim_vocabulary),
    };
    quote! {
        let #input = #library::_private::#trim(#input);
        let mut #found: ::core::option::Option<#library::Command<'_, #action_ty>> =
            ::core::option::Option::None;
        #(#alternatives)*
        #found
    }
}

/// Generates the matching of one pattern against the whole input.
///
/// `selector` is the expression of the command's selector. It can read the text that the
/// `{target}` capture matched through `locals.target`.
/// `operations`, when present, is a tuple of the constructor and field converters resolved
/// outside the parser body.
pub(crate) fn alternative(
    library: &TokenStream,
    action: &Action<'_>,
    pattern: &Pattern,
    selector: &TokenStream,
    locals: &Bindings,
    operations: Option<&TokenStream>,
) -> TokenStream {
    let Bindings {
        input,
        found,
        target,
        index,
        part,
        ..
    } = locals;
    let constructor = &action.constructor;
    let fields = &action.fields;
    let bindings = &locals.fields[..fields.len()];
    let names = fields.iter().map(|(name, _)| name);
    let capture_match = quote!(#library::_private::CaptureMatch);
    let conversions = pattern
        .captures
        .iter()
        .enumerate()
        .map(|(position, capture)| {
            if capture == TARGET {
                return quote!(#position => #target = #part);
            }
            let i = fields
                .iter()
                .position(|(name, _)| *name == capture.as_str())
                .expect("a validated capture names a field");
            let ty = fields[i].1;
            let binding = &bindings[i];
            let parse = operations.map_or_else(
                || quote!(<#ty as ::core::str::FromStr>::from_str(#part)),
                |operations| {
                    let i = syn::Index::from(i + 1);
                    quote!((#operations.#i)(#part))
                },
            );
            quote! {
                #position => {
                    #binding = #parse.ok();
                    if #binding.is_none() {
                        return #capture_match::Reject;
                    }
                }
            }
        });
    let values = bindings
        .iter()
        .map(|binding| quote!(#binding.take().expect("matched field")));
    let action = if let Some(operations) = operations {
        quote!((#operations.0)((#(#values,)*)))
    } else {
        quote!(#constructor { #(#names: #values),* })
    };
    let literals = &pattern.literals;
    if pattern.captures.is_empty() {
        // A phrase without captures matches the whole trimmed input.
        let [literal] = literals.as_slice() else {
            unreachable!("a pattern without captures has one literal")
        };
        return quote! {
            if #input.eq_ignore_ascii_case(#literal) {
                if #found.is_some() {
                    return ::core::option::Option::None;
                }
                #found = ::core::option::Option::Some(#library::Command {
                    action: #action,
                    target: #selector,
                });
            }
        };
    }
    let last = pattern.captures.len() - 1;
    let target_binding = pattern
        .captures
        .iter()
        .any(|capture| capture == TARGET)
        .then(|| quote!(let mut #target = "";));
    quote! {
        {
            #target_binding
            #(let mut #bindings = ::core::option::Option::None;)*
            if #library::_private::split(#input, &[#(#literals),*], |#index, #part| {
                match #index {
                    #(#conversions,)*
                    _ => ::core::unreachable!(),
                }
                if #index == #last {
                    if #found.is_some() {
                        return #capture_match::Ambiguous;
                    }
                    // Every field was accepted along this path. Only the first match needs
                    // to move the values, so shared prefixes never require Clone.
                    #found = ::core::option::Option::Some(#library::Command {
                        action: #action,
                        target: #selector,
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
        patterns.push(parse_pattern(
            string,
            &fields,
            &variant.ident,
            TargetRule::Required,
            Normalization::Derive,
            seen,
        )?);
    }
    Ok(Some(Action {
        constructor: {
            let variant = &variant.ident;
            quote!(Self::#variant)
        },
        fields,
        patterns,
    }))
}

/// Parses and validates one pattern string, and rejects a pattern that `seen` already holds.
///
/// Two patterns are the same when they differ only in the ASCII case of their literals.
pub(crate) fn parse_pattern(
    string: &LitStr,
    fields: &[(&Ident, &Type)],
    variant: &Ident,
    target: TargetRule,
    normalization: Normalization,
    seen: &mut HashSet<Vec<Piece>>,
) -> syn::Result<Pattern> {
    let value = string.value();
    // The build host reads the whole pattern, so that the first broken rule is reported.
    let pieces = match parse_pieces(&value, normalization, usize::MAX) {
        Ok(pieces) => pieces,
        Err(error) => {
            return Err(syn::Error::new(
                string.span(),
                message(&error, &value, variant),
            ));
        }
    };
    let names = fields
        .iter()
        .map(|(name, _)| name.to_string())
        .collect::<Vec<_>>();
    let names = names.iter().map(String::as_str).collect::<Vec<_>>();
    let pattern = validate(&pieces, &names, target).map_err(|error| {
        // A field that no capture names is reported where it is declared.
        let span = match error {
            PatternError::MissingField(missing) => fields
                .iter()
                .find(|(name, _)| *name == missing)
                .map_or_else(|| string.span(), |(name, _)| name.span()),
            _ => string.span(),
        };
        syn::Error::new(span, message(&error, &value, variant))
    })?;
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
            format!("duplicate pattern \"{value}\""),
        ));
    }
    Ok(pattern)
}

/// Words a refused pattern as a compile error.
///
/// # Panics
///
/// Panics when the build host runs out of memory while checking a pattern.
fn message(error: &PatternError<'_>, pattern: &str, variant: &Ident) -> String {
    match error {
        PatternError::UnclosedCapture => "unclosed capture `{`".to_owned(),
        PatternError::EmptyCapture => "empty capture `{}`".to_owned(),
        PatternError::UnmatchedBrace => "unmatched `}`, write `}}` for a literal brace".to_owned(),
        PatternError::OutOfMemory => {
            panic!("the build host ran out of memory while checking a command pattern")
        }
        PatternError::AdjacentCaptures => {
            format!("adjacent captures in \"{pattern}\" need a literal between them")
        }
        PatternError::UnknownField(name) => {
            format!("unknown field `{name}` in \"{pattern}\"")
        }
        PatternError::TargetInPrevious => {
            "a previous phrase refers to the previous target and cannot hold {target}".to_owned()
        }
        PatternError::RepeatedCapture(name) => format!("repeated capture `{{{name}}}`"),
        PatternError::MissingTarget => format!("\"{pattern}\" needs a {{target}} capture"),
        PatternError::MissingField(field) => {
            format!("missing field `{field}` in \"{pattern}\" of `{variant}`")
        }
        PatternError::EmptyPhrase => "empty phrase".to_owned(),
        PatternError::TooManyCaptures => {
            format!("a pattern holds at most {MAX_CAPTURES} captures")
        }
    }
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

    /// Pins every pattern message byte for byte, so that the shared rules cannot reword them.
    #[test]
    fn commands_keep_the_exact_pattern_messages() {
        for (pattern, expected) in [
            ("dim {target", "unclosed capture `{`"),
            ("dim {}", "empty capture `{}`"),
            (
                "dim } {target}",
                "unmatched `}`, write `}}` for a literal brace",
            ),
            (
                "{target}{level}",
                "adjacent captures in \"{target}{level}\" need a literal between them",
            ),
            (
                " {nope} {target}!",
                "unknown field `nope` in \" {nope} {target}!\"",
            ),
            ("{target} {level} {level}", "repeated capture `{level}`"),
            ("dim {level}", "\"dim {level}\" needs a {target} capture"),
            (
                "dim {target}",
                "missing field `level` in \"dim {target}\" of `Dim`",
            ),
        ] {
            let source = format!("enum Op {{ #[command({pattern:?})] Dim {{ level: u8 }} }}");
            let error = super::expand(source.parse().unwrap()).unwrap_err();
            assert_eq!(error.to_string(), expected, "{pattern}");
        }
        let error = super::expand(
            "enum Op { #[command(\"{target} {a} {b} {c} {d} {e} {f} {g} {h}\")] \
             Pick { a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8 } }"
                .parse()
                .unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "a pattern holds at most 8 captures");
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
