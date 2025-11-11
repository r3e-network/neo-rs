mod decode;
mod encode;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(NeoEncode, attributes(neo))]
pub fn derive_neo_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;

    let body = match input.data {
        Data::Struct(data) => encode::encode_struct(&data, quote! { self }),
        Data::Enum(data) => encode::encode_enum(&ident, &data),
        Data::Union(_) => {
            return syn::Error::new_spanned(ident, "NeoEncode not supported for unions")
                .to_compile_error()
                .into();
        }
    };

    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics neo_base::NeoEncode for #ident #ty_generics #where_clause {
            fn neo_encode<W: neo_base::NeoWrite>(&self, writer: &mut W) {
                #body
            }
        }
    }
    .into()
}

#[proc_macro_derive(NeoDecode, attributes(neo))]
pub fn derive_neo_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;

    let body = match input.data {
        Data::Struct(data) => decode::decode_struct(&ident, &data),
        Data::Enum(data) => decode::decode_enum(&ident, &data),
        Data::Union(_) => {
            return syn::Error::new_spanned(ident, "NeoDecode not supported for unions")
                .to_compile_error()
                .into();
        }
    };

    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics neo_base::NeoDecode for #ident #ty_generics #where_clause {
            fn neo_decode<R: neo_base::NeoRead>(reader: &mut R) -> Result<Self, neo_base::DecodeError> {
                #body
            }
        }
    }
    .into()
}
