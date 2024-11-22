use syn::parse::Parse;
use syn::{Lit, Meta};

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
                            if let Lit::Str(lit) = nv.value {
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
