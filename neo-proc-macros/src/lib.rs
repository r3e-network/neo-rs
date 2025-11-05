use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DataEnum, DataStruct, DeriveInput, Fields};

#[proc_macro_derive(NeoEncode, attributes(neo))]
pub fn derive_neo_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;

    let body = match input.data {
        Data::Struct(data) => encode_struct(&data, quote! { self }),
        Data::Enum(data) => encode_enum(&ident, &data),
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
        Data::Struct(data) => decode_struct(&ident, &data),
        Data::Enum(data) => decode_enum(&ident, &data),
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

fn encode_struct(
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

fn decode_struct(_ident: &syn::Ident, data: &DataStruct) -> proc_macro2::TokenStream {
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

fn encode_enum(_ident: &syn::Ident, data: &DataEnum) -> proc_macro2::TokenStream {
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

fn decode_enum(ident: &syn::Ident, data: &DataEnum) -> proc_macro2::TokenStream {
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
