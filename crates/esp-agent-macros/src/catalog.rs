//! Expands the declarations of `targets!` into a constant catalog expression.

use std::collections::HashMap;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitInt, LitStr, Token, braced, bracketed};

/// One `device` declaration.
struct Device {
    ident: Ident,
    id: LitInt,
    name: LitStr,
    replies: Vec<LitStr>,
}

/// One `choose` or `group` declaration.
struct Selector {
    phrases: Vec<LitStr>,
    members: Vec<Ident>,
}

/// The parsed body of `targets!`.
struct Declarations {
    devices: Vec<Device>,
    choices: Vec<Selector>,
    groups: Vec<Selector>,
}

impl Parse for Declarations {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut declarations = Self {
            devices: Vec::new(),
            choices: Vec::new(),
            groups: Vec::new(),
        };
        while !input.is_empty() {
            let keyword: Ident = input.parse()?;
            match keyword.to_string().as_str() {
                "device" => declarations.devices.push(parse_device(input)?),
                "choose" => {
                    let phrase: LitStr = input.parse()?;
                    input.parse::<kw::from>()?;
                    let members = parse_members(input)?;
                    input.parse::<Token![;]>()?;
                    declarations.choices.push(Selector {
                        phrases: vec![phrase],
                        members,
                    });
                }
                "group" => {
                    let phrases = parse_strings(input)?;
                    if phrases.is_empty() {
                        return Err(syn::Error::new(
                            keyword.span(),
                            "a group needs at least one phrase",
                        ));
                    }
                    input.parse::<Token![=]>()?;
                    let members = parse_members(input)?;
                    input.parse::<Token![;]>()?;
                    declarations.groups.push(Selector { phrases, members });
                }
                other => {
                    return Err(syn::Error::new(
                        keyword.span(),
                        format!("expected `device`, `choose` or `group`, found `{other}`"),
                    ));
                }
            }
        }
        Ok(declarations)
    }
}

mod kw {
    syn::custom_keyword!(from);
    syn::custom_keyword!(replies);
}

fn parse_device(input: ParseStream) -> syn::Result<Device> {
    let ident = input.parse()?;
    input.parse::<Token![=]>()?;
    let id = input.parse()?;
    input.parse::<Token![,]>()?;
    let name = input.parse()?;
    let body;
    braced!(body in input);
    let mut replies = Vec::new();
    if !body.is_empty() {
        body.parse::<kw::replies>()?;
        replies = parse_strings(&body)?;
        body.parse::<Token![;]>()?;
    }
    if !body.is_empty() {
        return Err(body.error("expected `}` after the replies"));
    }
    Ok(Device {
        ident,
        id,
        name,
        replies,
    })
}

fn parse_strings(input: ParseStream) -> syn::Result<Vec<LitStr>> {
    let content;
    bracketed!(content in input);
    Ok(Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

fn parse_members(input: ParseStream) -> syn::Result<Vec<Ident>> {
    let content;
    bracketed!(content in input);
    Ok(Punctuated::<Ident, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

/// Reduces a phrase to the form the library compares: no outer whitespace, no trailing
/// sentence punctuation, no leading article, ASCII lowercase.
///
/// Keep this in step with `normalize` in the library's `catalog` module.
fn normalize(text: &str) -> String {
    let text = text
        .trim_start()
        .trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '.' | '!' | '?'));
    let lower = text.to_ascii_lowercase();
    lower
        .strip_prefix("the ")
        .map_or(lower.as_str(), str::trim_start)
        .to_owned()
}

/// Expands the body of `targets!` into a `RawCatalog` expression.
///
/// Every problem is an error that points at the offending declaration.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let declarations: Declarations = syn::parse2(input)?;

    let mut ids: HashMap<u32, &Ident> = HashMap::new();
    let mut indexes: HashMap<String, usize> = HashMap::new();
    let mut ident_ids: Vec<u32> = Vec::new();
    for (position, device) in declarations.devices.iter().enumerate() {
        let id: u32 = device.id.base10_parse()?;
        if let Some(first) = ids.insert(id, &device.ident) {
            return Err(syn::Error::new(
                device.id.span(),
                format!("duplicate ID {id}: `{first}` already uses it",),
            ));
        }
        if indexes.insert(device.ident.to_string(), position).is_some() {
            return Err(syn::Error::new(
                device.ident.span(),
                format!("duplicate device `{ident}`", ident = device.ident),
            ));
        }
        ident_ids.push(id);
    }

    let mut selectors: HashMap<String, String> = HashMap::new();
    let mut claim = |phrase: &LitStr, owner: String| -> syn::Result<()> {
        let key = normalize(&phrase.value());
        if key.is_empty() {
            return Err(syn::Error::new(phrase.span(), "the selector is empty"));
        }
        if let Some(first) = selectors.insert(key.clone(), owner) {
            return Err(syn::Error::new(
                phrase.span(),
                format!("the selector \"{key}\" is already declared by {first}"),
            ));
        }
        Ok(())
    };
    for device in &declarations.devices {
        for alias in &device.replies {
            if normalize(&alias.value()).is_empty() {
                return Err(syn::Error::new(alias.span(), "empty reply alias"));
            }
        }
        claim(
            &device.name,
            format!("device `{ident}`", ident = device.ident),
        )?;
    }
    for selector in declarations.choices.iter().chain(&declarations.groups) {
        for phrase in &selector.phrases {
            claim(phrase, String::from("another declaration"))?;
        }
    }

    let members = |selector: &Selector| -> syn::Result<Vec<u32>> {
        if selector.members.is_empty() {
            return Err(syn::Error::new(
                selector
                    .phrases
                    .first()
                    .map_or_else(Span::call_site, LitStr::span),
                "a selector needs at least one device",
            ));
        }
        let mut found = Vec::new();
        for member in &selector.members {
            let Some(&position) = indexes.get(&member.to_string()) else {
                return Err(syn::Error::new(
                    member.span(),
                    format!("unknown device `{member}`"),
                ));
            };
            let id = ident_ids[position];
            if found.contains(&id) {
                return Err(syn::Error::new(
                    member.span(),
                    format!("device `{member}` is listed twice"),
                ));
            }
            found.push(id);
        }
        Ok(found)
    };

    let devices = declarations
        .devices
        .iter()
        .zip(&ident_ids)
        .map(|(device, id)| {
            let name = &device.name;
            let replies = &device.replies;
            quote! {
                RawDevice { id: #id, name: #name, replies: &[#(#replies),*] }
            }
        });
    let selector = |selector: &Selector| -> syn::Result<TokenStream> {
        let phrases = &selector.phrases;
        let members = members(selector)?;
        Ok(quote! {
            RawSelector { phrases: &[#(#phrases),*], members: &[#(#members),*] }
        })
    };
    let choices = declarations
        .choices
        .iter()
        .map(&selector)
        .collect::<syn::Result<Vec<_>>>()?;
    let groups = declarations
        .groups
        .iter()
        .map(&selector)
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        RawCatalog {
            devices: &[#(#devices),*],
            choices: &[#(#choices),*],
            groups: &[#(#groups),*],
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_reports_invalid_declarations() {
        for (suffix, reason) in [
            ("device B = 1, \"b\" { replies []; }", "duplicate ID"),
            (
                r#"device B = 2, "b" { replies ["?"]; }"#,
                "empty reply alias",
            ),
            (
                r#"device B = 2, "b" { replies [""]; }"#,
                "empty reply alias",
            ),
            (r#"device A = 2, "b" {}"#, "duplicate device `A`"),
            (r#"choose "lamp" from [];"#, "at least one device"),
            (r#"group ["both"] = [A, A];"#, "device `A` is listed twice"),
            ("group [] = [A];", "at least one phrase"),
            ("choose \" THE A \" from [A];", "selector"),
            (
                r#"device B = 2, "A ! ?" {}"#,
                r#"the selector "a" is already declared"#,
            ),
            ("group [\"both\"] = [A, B];", "unknown device"),
        ] {
            let source = format!("device A = 1, \"a\" {{ replies []; }} {suffix}");
            assert!(
                super::expand(source.parse().unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains(reason)
            );
        }
    }
}
