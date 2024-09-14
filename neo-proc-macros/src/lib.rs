// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Expr, ExprLit, ImplItem, ItemFn, ItemImpl, Lit, Meta};

use syn::parse::Parse;

mod bin;
mod derive_contract;

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

#[proc_macro_derive(Contract)]
pub fn derive_contract(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl Contract for #name {
            fn script(&self) -> &Vec<u8> {
                &self.script
            }

            fn parameter_list(&self) -> &Vec<ContractParameterType> {
                &self.parameter_list
            }

            fn script_hash(&mut self) -> UInt160 {
                if let Some(hash) = self.script_hash {
                    hash
                } else {
                    let hash = UInt160::from_script(&self.script);
                    self.script_hash = Some(hash);
                    hash
                }
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    // event::generate(attr.into(), item.into()).into()
    TokenStream::default()
}

#[proc_macro_attribute]
pub fn contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        #input

        impl #name {
            pub fn new() -> Self {
                Self {}
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn contract_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    let ty = &input.self_ty;

    let expanded = quote! {
        #input

        impl #ty {
            pub fn contract_type() -> &'static str {
                stringify!(#ty)
            }
        }
    };

    TokenStream::from(expanded)
}

struct ContractMethodArgs {
    name: Option<String>,
    required_call_flags: Option<u32>,
    cpu_fee: Option<u64>,
    storage_fee: Option<u64>,
    active_in: Option<String>,
    deprecated_in: Option<String>,
}

impl Parse for ContractMethodArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut args = ContractMethodArgs {
            name: None,
            required_call_flags: None,
            cpu_fee: None,
            storage_fee: None,
            active_in: None,
            deprecated_in: None,
        };

        while !input.is_empty() {
            let meta = input.parse::<Meta>()?;
            match meta {
                Meta::NameValue(nv) => {
                    let ident = nv.path.get_ident().unwrap().to_string();
                    match ident.as_str() {
                        "name" => {
                            if let Expr::Lit(ExprLit {
                                lit: Lit::Str(lit), ..
                            }) = nv.value
                            {
                                args.name = Some(lit.value());
                            }
                        }
                        "required_call_flags" => {
                            if let Lit::Int(lit) = nv.value {
                                args.required_call_flags = Some(lit.base10_parse()?);
                            }
                        }
                        "cpu_fee" => {
                            if let Lit::Int(lit) = nv.value {
                                args.cpu_fee = Some(lit.base10_parse()?);
                            }
                        }
                        "storage_fee" => {
                            if let Lit::Int(lit) = nv.value {
                                args.storage_fee = Some(lit.base10_parse()?);
                            }
                        }
                        "active_in" => {
                            if let Lit::Str(lit) = nv.value {
                                args.active_in = Some(lit.value());
                            }
                        }
                        "deprecated_in" => {
                            if let Lit::Str(lit) = nv.value {
                                args.deprecated_in = Some(lit.value());
                            }
                        }
                        _ => {}
                    }
                }
                _ => return Err(syn::Error::new_spanned(meta, "Unsupported attribute")),
            }
            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }

        Ok(args)
    }
}

#[proc_macro_attribute]
pub fn contract_method(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ContractMethodArgs);
    let input = parse_macro_input!(input as ItemFn);

    let fn_name = &input.sig.ident;
    let fn_args = &input.sig.inputs;
    let fn_output = &input.sig.output;
    let fn_body = &input.block;

    let name = args.name.unwrap_or_else(|| fn_name.to_string());
    let required_call_flags = args.required_call_flags.unwrap_or(0);
    let cpu_fee = args.cpu_fee.unwrap_or(0);
    let storage_fee = args.storage_fee.unwrap_or(0);
    let active_in = args
        .active_in
        .map(|s| quote! { Some(#s.parse().unwrap()) })
        .unwrap_or(quote! { None });
    let deprecated_in = args
        .deprecated_in
        .map(|s| quote! { Some(#s.parse().unwrap()) })
        .unwrap_or(quote! { None });

    let expanded = quote! {
        #[allow(non_upper_case_globals)]
        const #fn_name: ContractMethodMetadata = ContractMethodMetadata {
            name: #name,
            required_call_flags: #required_call_flags,
            cpu_fee: #cpu_fee,
            storage_fee: #storage_fee,
            active_in: #active_in,
            deprecated_in: #deprecated_in,
        };

        fn #fn_name(#fn_args) #fn_output {
            #fn_body
        }
    };

    TokenStream::from(expanded)
}

use syn::{Ident, LitInt, LitStr, Token};

#[proc_macro_attribute]
pub fn contract_events(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemImpl);

    let mut events = Vec::new();

    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if attr.path().is_ident("contract_event") {
                    if let Ok(event) = attr.parse_args::<EventAttribute>() {
                        events.push(event);
                    }
                }
            }
        }
    }

    let event_descriptors = events.iter().map(|event| {
        let name = &event.name;
        let order = &event.order;
        let params = event.params.iter().map(|(name, ty)| {
            quote! {
                ContractParameterDefinition {
                    name: #name.to_string(),
                    ty: #ty.to_string(),
                }
            }
        });

        quote! {
            ContractEventDescriptor {
                name: #name.to_string(),
                order: #order,
                parameters: vec![#(#params),*],
            }
        }
    });

    let expanded = quote! {
        #input

        impl ContractEvents for #input.self_ty {
            fn get_event_descriptors() -> Vec<ContractEventDescriptor> {
                vec![
                    #(#event_descriptors),*
                ]
            }
        }
    };

    TokenStream::from(expanded)
}

struct EventAttribute {
    name: LitStr,
    order: LitInt,
    params: Vec<(Ident, LitStr)>,
}

impl Parse for EventAttribute {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut order = None;
        let mut params = Vec::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "name" => {
                    name = Some(input.parse()?);
                }
                "order" => {
                    order = Some(input.parse()?);
                }
                _ => {
                    let value: LitStr = input.parse()?;
                    params.push((key, value));
                }
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(EventAttribute {
            name: name.ok_or_else(|| syn::Error::new(input.span(), "Missing 'name' parameter"))?,
            order: order
                .ok_or_else(|| syn::Error::new(input.span(), "Missing 'order' parameter"))?,
            params,
        })
    }
}
