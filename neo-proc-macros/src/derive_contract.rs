use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

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