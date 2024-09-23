use super::*;
use crate::smartcontract::{self, Parameter};
use anyhow::Result;

#[test]
fn test_params_from_any() -> Result<()> {
    let str = "jajaja";

    let ps = from_any(vec![
        str.into(),
        Parameter {
            param_type: smartcontract::ParameterType::String,
            value: str.into(),
        },
    ])?;
    assert_eq!(ps.len(), 2);

    let res_str = ps[0].get_string()?;
    assert_eq!(res_str, str);

    let res_fp = ps[1].get_func_param()?;
    assert_eq!(res_fp.param_type, smartcontract::ParameterType::String);
    let res_str = res_fp.value.get_string()?;
    assert_eq!(res_str, str);

    // Invalid item.
    let result = from_any(vec![Parameter {
        param_type: smartcontract::ParameterType::Integer,
        value: str.into(),
    }]);
    assert!(result.is_err());

    Ok(())
}
