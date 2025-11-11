use quote::quote;
use syn::{DataEnum, DataStruct, Fields};

pub(super) fn decode_struct(_ident: &syn::Ident, data: &DataStruct) -> proc_macro2::TokenStream {
    match &data.fields {
        Fields::Named(fields) => {
            let names = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().unwrap());
            let decode = names.clone().map(|ident| {
                quote! { #ident: neo_base::NeoDecode::neo_decode(reader)? }
            });
            quote! {
                Ok(Self {
                    #( #decode, )*
                })
            }
        }
        Fields::Unnamed(fields) => {
            let decode = fields.unnamed.iter().map(|_| {
                quote! { neo_base::NeoDecode::neo_decode(reader)? }
            });
            quote! { Ok(Self( #( #decode, )* )) }
        }
        Fields::Unit => quote! { Ok(Self) },
    }
}

pub(super) fn decode_enum(ident: &syn::Ident, data: &DataEnum) -> proc_macro2::TokenStream {
    let variants = data.variants.iter().enumerate().map(|(idx, variant)| {
        let tag = idx as u64;
        let v_ident = &variant.ident;
        match &variant.fields {
            Fields::Unit => quote! {
                #tag => Ok(#ident::#v_ident),
            },
            Fields::Unnamed(fields) => {
                let values = fields.unnamed.iter().map(|_| {
                    quote! { neo_base::NeoDecode::neo_decode(reader)? }
                });
                quote! {
                    #tag => Ok(#ident::#v_ident( #( #values ),* )),
                }
            }
            Fields::Named(fields) => {
                let values = fields.named.iter().map(|field| {
                    let name = field.ident.as_ref().unwrap();
                    quote! { #name: neo_base::NeoDecode::neo_decode(reader)? }
                });
                quote! {
                    #tag => Ok(#ident::#v_ident { #( #values ),* }),
                }
            }
        }
    });

    quote! {
        let tag = neo_base::read_varint(reader)?;
        match tag {
            #( #variants )*
            _ => Err(neo_base::DecodeError::InvalidValue("enum variant")),
        }
    }
}
