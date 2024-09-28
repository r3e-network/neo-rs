// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use proc_macro2::TokenStream;
use quote::{ToTokens, quote, quote_spanned};
use syn::{Meta, Token, punctuated::Punctuated, spanned::Spanned};

pub(crate) fn encode_bin(input: syn::DeriveInput) -> proc_macro::TokenStream {
    let name = &input.ident;
    let (impls, types, wheres) = &input.generics.split_for_impl();
    encode_bin_recursive(&input)
        .map(|(encoded, size)| {
            quote! {
                #[allow(non_snake_case)]
                impl #impls BinEncoder for #name #types #wheres {
                    fn encode_bin(&self, w: &mut impl BinWriter) {
                        #encoded
                    }

                    fn bin_size(&self) -> usize {
                        #size
                    }
                }
            }
        })
        .unwrap_or_else(|s| quote! { compile_error!(#s); })
        .into()
}

pub(crate) fn decode_bin(input: syn::DeriveInput) -> proc_macro::TokenStream {
    let name = &input.ident;
    let (impls, types, wheres) = &input.generics.split_for_impl();
    decode_bin_recursive(&input)
        .map(|decoded| {
            quote! {
                impl #impls BinDecoder for #name #types #wheres {
                    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
                        #decoded
                    }
                }
            }
        })
        .unwrap_or_else(|s| quote! { compile_error!(#s); })
        .into()
}

pub(crate) fn decode_bin_inner(input: syn::DeriveInput) -> proc_macro::TokenStream {
    let name = &input.ident;
    let (impls, types, wheres) = &input.generics.split_for_impl();
    decode_bin_recursive(&input)
        .map(|decoded| {
            quote! {
                impl #impls #name #types #wheres {
                    fn decode_bin_inner(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
                        #decoded
                    }
                }
            }
        })
        .unwrap_or_else(|s| quote! { compile_error!(#s); })
        .into()
}

fn decode_bin_recursive(input: &syn::DeriveInput) -> Result<TokenStream, String> {
    let name = &input.ident;
    match &input.data {
        syn::Data::Struct(ref struct_) => {
            let actions = match struct_.fields {
                syn::Fields::Named(ref fields) => decode_struct_named(name, fields),
                syn::Fields::Unnamed(ref fields) => decode_struct_unnamed(name, fields),
                syn::Fields::Unit => Ok(quote! { #name }), // just empty
            }?;

            Ok(quote! { Ok( #actions )})
        }
        syn::Data::Enum(ref enum_) => decode_enum(name, &input.attrs, enum_),
        syn::Data::Union(_) => Err("`union` must implements BinDecoder manually".into()),
    }
}

fn decode_struct_named(
    name: &syn::Ident,
    fields: &syn::FieldsNamed,
) -> Result<TokenStream, String> {
    let mut items = Vec::with_capacity(fields.named.len());
    for f in fields.named.iter() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        let name = &f.ident;
        let item = if !ignore {
            quote_spanned! { f.span() => #name: BinDecoder::decode_bin(r)?, }
        } else {
            quote_spanned! { f.span() => #name: core::default::Default::default(), }
        };

        items.push(item);
    }

    Ok(quote! { #name { #( #items)* } })
}

fn decode_struct_unnamed(
    name: &syn::Ident,
    fields: &syn::FieldsUnnamed,
) -> Result<TokenStream, String> {
    let mut items = Vec::with_capacity(fields.unnamed.len());
    for f in fields.unnamed.iter() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        let item = if !ignore {
            quote_spanned! { f.span() => BinDecoder::decode_bin(r)?, }
        } else {
            quote_spanned! { f.span() => core::default::Default::default(), }
        };

        items.push(item);
    }

    Ok(quote! { #name ( #( #items)* ) })
}

fn decode_enum(
    name: &syn::Ident,
    attrs: &[syn::Attribute],
    enum_: &syn::DataEnum,
) -> Result<TokenStream, String> {
    let repr = enum_repr_attr(attrs)?;

    let mut items = Vec::with_capacity(enum_.variants.len());
    for v in enum_.variants.iter() {
        let item = &v.ident;
        let typ = enum_variant_type(v)?;

        let item = match v.fields {
            syn::Fields::Named(ref fields) => decode_enum_named(name, item, fields)?,
            syn::Fields::Unnamed(ref fields) => decode_enum_unnamed(name, item, fields)?,
            syn::Fields::Unit => quote_spanned! { v.span() => #name::#item },
        };

        items.push(quote_spanned! { v.span() => #typ => Ok( #item ), });
    }

    let name_lit = name.to_string();
    Ok(quote! {
        let offset = r.consumed();
        let typ: #repr = BinDecoder::decode_bin(r)?;
        match typ {
            #( #items)*
            _ => { Err(BinDecodeError::InvalidType(#name_lit, offset, typ as u64)) } ,
        }
    })
}

fn encode_bin_recursive(input: &syn::DeriveInput) -> Result<(TokenStream, TokenStream), String> {
    let name = &input.ident;
    match &input.data {
        syn::Data::Struct(ref struct_) => {
            match struct_.fields {
                syn::Fields::Named(ref fields) => encode_struct_named(name, fields),
                syn::Fields::Unnamed(ref fields) => encode_struct_unnamed(name, fields),
                syn::Fields::Unit => Ok((quote! {}, quote! { 0 })), // just empty
            }
        }
        syn::Data::Enum(enum_) => encode_enum(name, &input.attrs, enum_),
        syn::Data::Union(_) => Err("`union` must implement BinEncoder manually".into()),
    }
}

fn encode_struct_named(
    _name: &syn::Ident,
    fields: &syn::FieldsNamed,
) -> Result<(TokenStream, TokenStream), String> {
    let mut items = Vec::with_capacity(fields.named.len());
    let mut sizes = Vec::with_capacity(fields.named.len());
    for f in fields.named.iter() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        if !ignore {
            let name = &f.ident;
            items.push(quote_spanned! { f.span() => self.#name.encode_bin(w); });
            sizes.push(quote_spanned! { f.span() => self.#name.bin_size() });
        }
    }

    Ok((quote! {  #( #items)* }, quote! {  0 #(+ #sizes)* }))
}

fn encode_struct_unnamed(
    _name: &syn::Ident,
    fields: &syn::FieldsUnnamed,
) -> Result<(TokenStream, TokenStream), String> {
    let mut items = Vec::with_capacity(fields.unnamed.len());
    let mut sizes = Vec::with_capacity(fields.unnamed.len());
    for (i, f) in fields.unnamed.iter().enumerate() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        if !ignore {
            let index = syn::Index::from(i);
            items.push(quote_spanned! { f.span() => self.#index.encode_bin(w); });
            sizes.push(quote_spanned! { f.span() => self.#index.bin_size() });
        }
    }

    Ok((quote! {  #( #items)* }, quote! {  0 #(+ #sizes)*  }))
}

fn encode_enum(
    name: &syn::Ident,
    attrs: &[syn::Attribute],
    enum_: &syn::DataEnum,
) -> Result<(TokenStream, TokenStream), String> {
    let repr = enum_repr_attr(attrs)?;

    let mut items = Vec::with_capacity(enum_.variants.len());
    let mut sizes = Vec::with_capacity(enum_.variants.len());
    for v in enum_.variants.iter() {
        let item = &v.ident;
        let typ = enum_variant_type(&v)?;
        let typ = quote! { ((#typ) as #repr).encode_bin(w); };

        let (item, size) = match v.fields {
            syn::Fields::Named(ref fields) => encode_enum_named(name, item, typ, fields)?,
            syn::Fields::Unnamed(ref fields) => encode_enum_unnamed(name, item, typ, fields)?,
            syn::Fields::Unit => (
                quote_spanned! { v.span() => #name::#item => { #typ } },
                quote_spanned! { v.span() => #name::#item => { 0 } },
            ),
        };

        items.push(item);
        sizes.push(size);
    }

    Ok((
        quote! {
            match self {
                #( #items)*
            }
        },
        quote! {
            core::mem::size_of::<#repr>() + match self {
                #( #sizes)*
            }
        },
    ))
}

fn encode_enum_named(
    name: &syn::Ident,
    variant: &syn::Ident,
    typ: TokenStream,
    fields: &syn::FieldsNamed,
) -> Result<(TokenStream, TokenStream), String> {
    let mut refs = Vec::with_capacity(fields.named.len());
    let mut items = Vec::with_capacity(fields.named.len());
    let mut sizes = Vec::with_capacity(fields.named.len());
    for f in fields.named.iter() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        let ident = &f.ident;
        refs.push(quote_spanned! {f.span() => ref #ident, });

        if !ignore {
            sizes.push(quote_spanned! { f.span() => #ident.bin_size() });
            items.push(quote_spanned! { f.span() => #ident.encode_bin(w); });
        } else {
            items.push(quote_spanned! { f.span() => let _ = #ident; })
        }
    }

    Ok((
        quote! {
            #name::#variant { #( #refs)* } => {
                #typ
                #( #items)*
            }
        },
        quote! {
            #name::#variant { #( #refs)* } => {
                0 #(+ #sizes)*
            }
        },
    ))
}

fn encode_enum_unnamed(
    name: &syn::Ident,
    variant: &syn::Ident,
    typ: TokenStream,
    fields: &syn::FieldsUnnamed,
) -> Result<(TokenStream, TokenStream), String> {
    let mut refs = Vec::with_capacity(fields.unnamed.len());
    let mut items = Vec::with_capacity(fields.unnamed.len());
    let mut sizes = Vec::with_capacity(fields.unnamed.len());
    for (idx, f) in fields.unnamed.iter().enumerate() {
        let ident = syn::Ident::new(&format!("_{}_{}_field{}", name, variant, idx), f.span());
        refs.push(quote_spanned! {f.span() => ref #ident, });

        let ignore = bin_ignore_attr(&f.attrs)?;
        if !ignore {
            sizes.push(quote_spanned! { f.span() => #ident.bin_size() });
            items.push(quote_spanned! { f.span() => #ident.encode_bin(w); });
        }
    }

    Ok((
        quote! {
            #name::#variant ( #( #refs)* ) => {
                #typ
                #( #items)*
            }
        },
        quote! {
            #name::#variant ( #( #refs)* ) => {
                0 #(+ #sizes)*
            }
        },
    ))
}

fn decode_enum_named(
    name: &syn::Ident,
    variant: &syn::Ident,
    fields: &syn::FieldsNamed,
) -> Result<TokenStream, String> {
    let mut items = Vec::with_capacity(fields.named.len());
    for f in fields.named.iter() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        let name = &f.ident;
        let item = if !ignore {
            quote_spanned! { f.span() => #name: BinDecoder::decode_bin(r)?, }
        } else {
            quote_spanned! { f.span() => #name: core::default::Default::default(), }
        };

        items.push(item);
    }

    Ok(quote! { #name::#variant { #( #items)* } })
}

fn decode_enum_unnamed(
    name: &syn::Ident,
    variant: &syn::Ident,
    fields: &syn::FieldsUnnamed,
) -> Result<TokenStream, String> {
    let mut items = Vec::with_capacity(fields.unnamed.len());
    for f in fields.unnamed.iter() {
        let ignore = bin_ignore_attr(&f.attrs)?;
        let item = if !ignore {
            quote_spanned! { f.span() => BinDecoder::decode_bin(r)?, }
        } else {
            quote_spanned! { f.span() => core::default::Default::default(), }
        };

        items.push(item);
    }

    Ok(quote! { #name::#variant ( #( #items)* ) })
}

fn enum_variant_type(v: &syn::Variant) -> Result<TokenStream, String> {
    let tag = token_of_attr(&v.attrs, "tag")?;
    if !tag.is_empty() {
        return Ok(tag);
    }

    let (ref _eq, ref expr) = v
        .discriminant
        .as_ref()
        .ok_or("enum variant must be set one of discriminant or type attribute".to_string())?;

    Ok(quote! { #expr })
}

fn enum_repr_attr(attrs: &[syn::Attribute]) -> Result<TokenStream, String> {
    let repr = token_of_attr(attrs, "repr")?;
    if !repr.is_empty() {
        let lit = repr.to_string();
        match lit.as_str() {
            "u8" | "u16" | "u32" | "u64" => Ok(repr),
            _ => Err(format!("invalid bin(repr) attribute: `{}`", lit)),
        }
    } else {
        Ok(quote! { u8 })
    }
}

fn bin_ignore_attr(attrs: &[syn::Attribute]) -> Result<bool, String> {
    for attr in attrs {
        if !attr.path().is_ident("bin") {
            continue;
        }

        let metas = attr
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map_err(|err| format!("invalid bin(ignore) attribute: {}", err))?;

        for meta in metas.iter() {
            if let Meta::Path(nv) = meta {
                if nv.is_ident("ignore") {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn token_of_attr(attrs: &[syn::Attribute], name: &str) -> Result<TokenStream, String> {
    for attr in attrs {
        if !attr.path().is_ident("bin") {
            continue;
        }

        let metas = attr
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map_err(|err| format!("invalid bin({}) attribute: {}", name, err))?;

        for meta in metas.iter() {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident(name) {
                    return Ok(nv.value.to_token_stream());
                }
            }
        }
    }

    Ok(TokenStream::new())
}
