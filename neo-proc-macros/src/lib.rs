// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

mod bin;


#[proc_macro_derive(BinEncode, attributes(bin))]
pub fn derive_bin_encode(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    bin::encode_bin(syn::parse_macro_input!(input as syn::DeriveInput))
}

#[proc_macro_derive(BinDecode, attributes(bin))]
pub fn derive_bin_decode(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    bin::decode_bin(syn::parse_macro_input!(input as syn::DeriveInput))
}

#[proc_macro_derive(InnerBinDecode, attributes(bin))]
pub fn derive_bin_decode_inner(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    bin::decode_bin_inner(syn::parse_macro_input!(input as syn::DeriveInput))
}