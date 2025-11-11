use quote::{format_ident, quote};
use syn::{DataEnum, DataStruct, Fields};

pub(super) fn encode_struct(
    data: &DataStruct,
    self_tokens: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match &data.fields {
        Fields::Named(fields) => {
            let encode = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().unwrap();
                quote! { neo_base::NeoEncode::neo_encode(&#self_tokens.#ident, writer); }
            });
            quote! { #( #encode )* }
        }
        Fields::Unnamed(fields) => {
            let encode = fields.unnamed.iter().enumerate().map(|(idx, _)| {
                let index = syn::Index::from(idx);
                quote! { neo_base::NeoEncode::neo_encode(&#self_tokens.#index, writer); }
            });
            quote! { #( #encode )* }
        }
        Fields::Unit => quote! {},
    }
}

pub(super) fn encode_enum(_ident: &syn::Ident, data: &DataEnum) -> proc_macro2::TokenStream {
    let arms = data.variants.iter().enumerate().map(|(idx, variant)| {
        let v_ident = &variant.ident;
        let tag = idx as u64;
        match &variant.fields {
            Fields::Unit => {
                quote! {
                    Self::#v_ident => {
                        neo_base::write_varint(writer, #tag);
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let bindings: Vec<_> = fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| format_ident!("field{idx}"))
                    .collect();
                let encoders = bindings.iter().map(|binding| {
                    quote! { neo_base::NeoEncode::neo_encode(#binding, writer); }
                });
                quote! {
                    Self::#v_ident( #( ref #bindings ),* ) => {
                        neo_base::write_varint(writer, #tag);
                        #( #encoders )*
                    }
                }
            }
            Fields::Named(fields) => {
                let bindings: Vec<_> = fields
                    .named
                    .iter()
                    .map(|field| field.ident.as_ref().unwrap())
                    .collect();
                let encoders = bindings.iter().map(|binding| {
                    quote! { neo_base::NeoEncode::neo_encode(#binding, writer); }
                });
                quote! {
                    Self::#v_ident { #( ref #bindings ),* } => {
                        neo_base::write_varint(writer, #tag);
                        #( #encoders )*
                    }
                }
            }
        }
    });

    quote! {
        match self {
            #( #arms )*
        }
    }
}
