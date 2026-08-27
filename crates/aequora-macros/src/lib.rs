//! Compile-time helpers for Aequora domain operation registration.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitInt, LitStr, parse_macro_input};

const PROFILE_KINDS: &[&str] = &[
    "ImmutableAppendOnly",
    "OptimisticVersioned",
    "Commutative",
    "LastWriterWins",
    "ManualConflict",
    "StrongAggregate",
    "ServerOnly",
    "DeviceLocal",
    "DerivedProjection",
];

const SEMANTIC_CLASSES: &[&str] = &[
    "SetValue",
    "PatchFields",
    "AppendEvent",
    "Increment",
    "SetMembership",
    "Transition",
    "Command",
    "DerivedOnly",
];

/// Implements `DomainOperation` from stable wire metadata.
///
/// ```ignore
/// #[derive(serde::Deserialize, AequoraOperation)]
/// #[aequora(kind = 0x1002, schema = 1, entity = "student")]
/// struct UpdateStudentPhone { /* domain fields */ }
/// ```
///
/// `kind` and `schema` are required `u16` values; schema zero is rejected. `entity` is accepted as
/// documentation metadata for forward compatibility but does not couple the operation to a
/// database table or storage schema.
#[proc_macro_derive(AequoraOperation, attributes(aequora))]
pub fn derive_aequora_operation(input: TokenStream) -> TokenStream {
    derive_operation(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Implements a stable aggregate identity and built-in consistency profile.
#[proc_macro_derive(AequoraAggregate, attributes(aequora))]
pub fn derive_aequora_aggregate(input: TokenStream) -> TokenStream {
    derive_aggregate(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn derive_operation(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let mut kind = None;
    let mut schema = None;
    let mut entity = None;
    let mut aggregate = None;
    let mut semantic = None;

    for attribute in input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("aequora"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("kind") {
                if kind.is_some() {
                    return Err(meta.error("duplicate `kind`"));
                }
                let literal: LitInt = meta.value()?.parse()?;
                kind = Some(literal.base10_parse::<u16>()?);
                return Ok(());
            }
            if meta.path.is_ident("schema") {
                if schema.is_some() {
                    return Err(meta.error("duplicate `schema`"));
                }
                let literal: LitInt = meta.value()?.parse()?;
                let value = literal.base10_parse::<u16>()?;
                if value == 0 {
                    return Err(syn::Error::new(literal.span(), "`schema` must be non-zero"));
                }
                schema = Some(value);
                return Ok(());
            }
            if meta.path.is_ident("entity") {
                if entity.is_some() {
                    return Err(meta.error("duplicate `entity`"));
                }
                entity = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            if meta.path.is_ident("aggregate") {
                if aggregate.is_some() {
                    return Err(meta.error("duplicate `aggregate`"));
                }
                let literal: LitInt = meta.value()?.parse()?;
                let value = literal.base10_parse::<u16>()?;
                if value == 0 {
                    return Err(syn::Error::new(
                        literal.span(),
                        "`aggregate` must be non-zero",
                    ));
                }
                aggregate = Some(value);
                return Ok(());
            }
            if meta.path.is_ident("semantic") {
                if semantic.is_some() {
                    return Err(meta.error("duplicate `semantic`"));
                }
                semantic = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            Err(meta.error(
                "unsupported `aequora` option; expected `kind`, `schema`, `entity`, `aggregate`, or `semantic`",
            ))
        })?;
    }

    let kind = kind.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "missing `#[aequora(kind = ...)]` value")
    })?;
    let schema = schema.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "missing `#[aequora(schema = ...)]` value")
    })?;
    let name = input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let profile_impl = match (aggregate, semantic) {
        (Some(aggregate), Some(semantic)) => {
            let semantic = checked_variant(&semantic, SEMANTIC_CLASSES, "semantic")?;
            quote! {
                impl #impl_generics ::aequora::profile::OperationProfileDefinition for #name #type_generics #where_clause {
                    const AGGREGATE_ID: ::aequora::profile::AggregateProfileId =
                        ::aequora::profile::AggregateProfileId::from_static(#aggregate);
                    const SEMANTIC_CLASS: ::aequora::profile::OperationSemanticClass =
                        ::aequora::profile::OperationSemanticClass::#semantic;
                }
            }
        }
        (None, None) => quote! {},
        _ => {
            return Err(syn::Error::new_spanned(
                &name,
                "`aggregate` and `semantic` must be specified together",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::aequora::executor::DomainOperation for #name #type_generics #where_clause {
            const KIND: u16 = #kind;
            const CURRENT_SCHEMA: u16 = #schema;
        }
        #profile_impl
    })
}

fn derive_aggregate(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let mut aggregate = None;
    let mut profile = None;
    for attribute in input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("aequora"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("aggregate") {
                if aggregate.is_some() {
                    return Err(meta.error("duplicate `aggregate`"));
                }
                let literal: LitInt = meta.value()?.parse()?;
                let value = literal.base10_parse::<u16>()?;
                if value == 0 {
                    return Err(syn::Error::new(
                        literal.span(),
                        "`aggregate` must be non-zero",
                    ));
                }
                aggregate = Some(value);
                return Ok(());
            }
            if meta.path.is_ident("profile") {
                if profile.is_some() {
                    return Err(meta.error("duplicate `profile`"));
                }
                profile = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            Err(meta.error("unsupported `aequora` option; expected `aggregate` or `profile`"))
        })?;
    }
    let aggregate = aggregate.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "missing `#[aequora(aggregate = ...)]` value")
    })?;
    let profile = profile.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "missing `#[aequora(profile = ...)]` value")
    })?;
    let profile = checked_variant(&profile, PROFILE_KINDS, "profile")?;
    let name = input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::aequora::profile::AggregateDefinition for #name #type_generics #where_clause {
            const AGGREGATE_ID: ::aequora::profile::AggregateProfileId =
                ::aequora::profile::AggregateProfileId::from_static(#aggregate);
            const PROFILE: ::aequora::profile::ConsistencyProfileKind =
                ::aequora::profile::ConsistencyProfileKind::#profile;
        }
    })
}

fn checked_variant(literal: &LitStr, allowed: &[&str], option: &str) -> syn::Result<syn::Ident> {
    let value = literal.value();
    if allowed.contains(&value.as_str()) {
        Ok(syn::Ident::new(&value, literal.span()))
    } else {
        Err(syn::Error::new(
            literal.span(),
            format!("unsupported `{option}` value {value:?}"),
        ))
    }
}
