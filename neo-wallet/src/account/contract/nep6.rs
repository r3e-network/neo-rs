use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::nep6::{Nep6Contract, Nep6Parameter};

use super::{Contract, ContractParameter, ContractParameterType};

pub(crate) fn contract_to_nep6(contract: &Contract) -> Nep6Contract {
    let script = BASE64.encode(contract.script());
    let parameters = contract
        .parameters()
        .iter()
        .map(|param| Nep6Parameter {
            name: param.name().to_string(),
            type_id: param.parameter_type() as u8,
        })
        .collect();
    Nep6Contract {
        script,
        parameters,
        deployed: contract.deployed(),
    }
}

pub(crate) fn contract_from_nep6(
    contract: &Nep6Contract,
) -> Result<Contract, crate::error::WalletError> {
    let script = BASE64
        .decode(contract.script.as_bytes())
        .map_err(|_| crate::error::WalletError::InvalidNep6("invalid contract script encoding"))?;
    let parameters = contract
        .parameters
        .iter()
        .map(|param| {
            Ok(ContractParameter::new(
                param.name.clone(),
                ContractParameterType::try_from(param.type_id)?,
            ))
        })
        .collect::<Result<Vec<_>, crate::error::WalletError>>()?;
    Ok(Contract::new(script, parameters, contract.deployed))
}
