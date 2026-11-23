#![forbid(unsafe_code)]

//! Derive macros re-exported by `engine-core`.

mod attributes;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields, GenericParam, parse_macro_input, parse_quote};

use attributes::{Attributes, Site};

/// Describes a struct or enum through the `engine_core::Reflect` trait.
#[proc_macro_derive(Reflect, attributes(reflect))]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(mut input: DeriveInput) -> syn::Result<Tokens> {
    let attributes = Attributes::parse(&input.attrs, Site::Type)?;
    let version = attributes.version.unwrap_or(1);
    let editor = attributes.editor();
    let shape = match input.data {
        Data::Struct(data) => {
            let fields = fields(&data.fields)?;
            quote!(::engine_core::reflection::TypeShape::Struct(#fields))
        }
        Data::Enum(data) => {
            let variants = data
                .variants
                .iter()
                .map(|variant| {
                    let editor = Attributes::parse(&variant.attrs, Site::Variant)?.editor();
                    let name = variant.ident.unraw().to_string();
                    let fields = fields(&variant.fields)?;
                    Ok(quote!(::engine_core::__private::reflect_editor!(
                        ::engine_core::reflection::Variant::new(#name, #fields),
                        #editor
                    )))
                })
                .collect::<syn::Result<Vec<_>>>()?;
            quote!(::engine_core::reflection::TypeShape::Enum(::std::boxed::Box::new([#(#variants),*])))
        }
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "Reflect supports structs and enums",
            ));
        }
    };

    for parameter in &mut input.generics.params {
        match parameter {
            GenericParam::Type(parameter) => parameter.bounds.push(parse_quote!('static)),
            GenericParam::Lifetime(parameter) => parameter.bounds.push(parse_quote!('static)),
            GenericParam::Const(_) => {}
        }
    }
    let name = input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::engine_core::Reflect for #name #type_generics #where_clause {
            fn schema() -> ::engine_core::reflection::TypeSchema {
                ::engine_core::__private::reflect_editor!(
                    ::engine_core::reflection::TypeSchema::new(#version, #shape),
                    #editor
                )
            }
        }
    })
}

fn fields(fields: &Fields) -> syn::Result<Tokens> {
    let fields = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = field
                .ident
                .as_ref()
                .map_or_else(|| index.to_string(), |name| name.unraw().to_string());
            let editor = Attributes::parse(&field.attrs, Site::Field)?.editor();
            let ty = &field.ty;
            Ok(quote!(::engine_core::__private::reflect_editor!(
                ::engine_core::reflection::Field::of::<#ty>(#name),
                #editor
            )))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(quote!(::std::boxed::Box::new([#(#fields),*])))
}
