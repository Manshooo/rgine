#![forbid(unsafe_code)]

//! Derive macros re-exported by `engine-ecs`.

use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as Tokens};
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Error, LitStr, Path, Token, Type, parenthesized,
    parse_macro_input, parse_quote,
};

/// Declares storage, requirements, and hooks for an `engine_ecs::Component`.
#[proc_macro_derive(Component, attributes(component))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(mut input: DeriveInput) -> syn::Result<Tokens> {
    match &input.data {
        Data::Struct(data) => {
            for field in &data.fields {
                reject_component_attribute(&field.attrs)?;
            }
        }
        Data::Enum(data) => {
            for variant in &data.variants {
                reject_component_attribute(&variant.attrs)?;
                for field in &variant.fields {
                    reject_component_attribute(&field.attrs)?;
                }
            }
        }
        Data::Union(data) => {
            for field in &data.fields.named {
                reject_component_attribute(&field.attrs)?;
            }
        }
    }

    let mut storage = None;
    let mut required = Vec::new();
    let mut on_add = None;
    let mut on_remove = None;
    for attribute in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("component"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("requires") {
                let content;
                parenthesized!(content in meta.input);
                required
                    .extend(content.parse_terminated(|input| input.parse::<Type>(), Token![,])?);
                return Ok(());
            }
            let hook = if meta.path.is_ident("on_add") {
                Some(&mut on_add)
            } else if meta.path.is_ident("on_remove") {
                Some(&mut on_remove)
            } else {
                None
            };
            if let Some(hook) = hook {
                if hook.is_some() {
                    return Err(Error::new_spanned(meta.path, "duplicate component hook"));
                }
                *hook = Some(meta.value()?.parse::<Path>()?);
                return Ok(());
            }
            if !meta.path.is_ident("storage") {
                return Err(Error::new_spanned(
                    meta.path,
                    "expected `storage`, `requires`, `on_add`, or `on_remove`",
                ));
            }
            if storage.is_some() {
                return Err(Error::new_spanned(meta.path, "duplicate storage kind"));
            }
            let value: LitStr = meta.value()?.parse()?;
            let name = match value.value().as_str() {
                "table" => "Table",
                "sparse" => "Sparse",
                _ => return Err(Error::new_spanned(value, "expected `table` or `sparse`")),
            };
            storage = Some(Ident::new(name, value.span()));
            Ok(())
        })?;
    }

    let name = input.ident;
    let storage = storage.map(|storage| {
        quote! {
            const STORAGE: ::engine_ecs::StorageKind = ::engine_ecs::StorageKind::#storage;
        }
    });
    let required_components = (!required.is_empty()).then(|| {
        quote! {
            const REQUIRED: &'static [::engine_ecs::RequiredComponent] = &[
                #(::engine_ecs::RequiredComponent::new::<#required>()),*
            ];
        }
    });
    let hooks = [("ON_ADD", on_add), ("ON_REMOVE", on_remove)]
        .into_iter()
        .filter_map(|(name, hook)| {
            let hook = hook?;
            let name = Ident::new(name, proc_macro2::Span::call_site());
            Some(quote! {
                const #name: ::core::option::Option<::engine_ecs::ComponentHook> =
                    ::core::option::Option::Some(#hook);
            })
        });
    input
        .generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: ::core::marker::Send + ::core::marker::Sync + 'static));
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::engine_ecs::Component for #name #type_generics #where_clause {
            #storage
            #required_components
            #(#hooks)*
        }
    })
}

fn reject_component_attribute(attributes: &[Attribute]) -> syn::Result<()> {
    if let Some(attribute) = attributes
        .iter()
        .find(|attr| attr.path().is_ident("component"))
    {
        return Err(Error::new_spanned(
            attribute,
            "declare component options on the component type",
        ));
    }
    Ok(())
}
