use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::parse::ParseStream;
use syn::{Attribute, Error, Lit, LitInt, LitStr, Token};

#[derive(PartialEq, Eq)]
pub(crate) enum Site {
    Type,
    Variant,
    Field,
}

#[derive(Default)]
pub(crate) struct Attributes {
    pub(crate) version: Option<u32>,
    display_name: Option<LitStr>,
    tooltip: Option<LitStr>,
    range: Option<(f64, f64)>,
}

impl Attributes {
    pub(crate) fn parse(attributes: &[Attribute], site: Site) -> syn::Result<Self> {
        let mut options = Self::default();
        for attribute in attributes
            .iter()
            .filter(|attr| attr.path().is_ident("reflect"))
        {
            attribute.parse_nested_meta(|meta| {
                let path = &meta.path;
                let existed = if path.is_ident("version") && site == Site::Type {
                    let version = meta.value()?.parse::<LitInt>()?.base10_parse()?;
                    options.version.replace(version).is_some()
                } else if path.is_ident("display_name") {
                    options
                        .display_name
                        .replace(meta.value()?.parse()?)
                        .is_some()
                } else if path.is_ident("tooltip") {
                    options.tooltip.replace(meta.value()?.parse()?).is_some()
                } else if path.is_ident("range") && site == Site::Field {
                    let value = meta.value()?;
                    let start = number(value)?;
                    value.parse::<Token![..=]>()?;
                    let end = number(value)?;
                    if start > end {
                        return Err(meta.error("range starts above its end"));
                    }
                    options.range.replace((start, end)).is_some()
                } else {
                    return Err(Error::new_spanned(path, "unsupported reflect option here"));
                };
                if existed {
                    return Err(Error::new_spanned(path, "duplicate reflect option"));
                }
                Ok(())
            })?;
        }
        Ok(options)
    }

    pub(crate) fn editor(&self) -> TokenStream {
        let display_name = option(self.display_name.as_ref());
        let tooltip = option(self.tooltip.as_ref());
        let range = option(self.range.map(|(start, end)| quote!((#start, #end))));
        quote! {
            ::engine_core::reflection::EditorMetadata {
                display_name: #display_name,
                tooltip: #tooltip,
                range: #range,
            }
        }
    }
}

fn number(input: ParseStream<'_>) -> syn::Result<f64> {
    let sign = if input.peek(Token![-]) {
        input.parse::<Token![-]>()?;
        -1.0
    } else {
        1.0
    };
    let literal = input.parse::<Lit>()?;
    let value: f64 = match &literal {
        Lit::Float(value) => value.base10_parse()?,
        Lit::Int(value) => value.base10_parse()?,
        _ => return Err(Error::new_spanned(literal, "expected a numeric bound")),
    };
    if !value.is_finite() {
        return Err(Error::new_spanned(literal, "range bounds must be finite"));
    }
    Ok(sign * value)
}

fn option(value: Option<impl ToTokens>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}
