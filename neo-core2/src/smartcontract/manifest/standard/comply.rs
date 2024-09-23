use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::smartcontract::manifest::{Manifest, Method, Standard};

// Define custom error types
#[derive(Debug)]
pub enum ComplianceError {
    MethodMissing(String),
    EventMissing(String),
    InvalidReturnType(String, String, String),
    InvalidParameterCount(String, usize, usize),
    InvalidParameterName(String, usize, String, String),
    InvalidParameterType(String, String, String),
    SafeMethodMismatch(String, bool),
}

impl fmt::Display for ComplianceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ComplianceError::MethodMissing(name) => write!(f, "method missing: '{}'", name),
            ComplianceError::EventMissing(name) => write!(f, "event missing: '{}'", name),
            ComplianceError::InvalidReturnType(name, expected, got) => 
                write!(f, "invalid return type: '{}' (expected {}, got {})", name, expected, got),
            ComplianceError::InvalidParameterCount(name, expected, got) => 
                write!(f, "invalid parameter count: '{}' (expected {}, got {})", name, expected, got),
            ComplianceError::InvalidParameterName(name, index, expected, got) => 
                write!(f, "invalid parameter name: '{}'[{}] (expected {}, got {})", name, index, expected, got),
            ComplianceError::InvalidParameterType(name, expected, got) => 
                write!(f, "invalid parameter type: '{}' (expected {}, got {})", name, expected, got),
            ComplianceError::SafeMethodMismatch(name, expected) => 
                write!(f, "'{}' has wrong safe flag: expected {}", name, expected),
        }
    }
}

impl Error for ComplianceError {}

lazy_static! {
    static ref CHECKS: HashMap<String, Vec<&'static Standard>> = {
        let mut m = HashMap::new();
        m.insert("NEP11".to_string(), vec![&NEP11_NON_DIVISIBLE, &NEP11_DIVISIBLE]);
        m.insert("NEP17".to_string(), vec![&NEP17]);
        m.insert("NEP11Payable".to_string(), vec![&NEP11_PAYABLE]);
        m.insert("NEP17Payable".to_string(), vec![&NEP17_PAYABLE]);
        m
    };
}

pub fn check(m: &Manifest, standards: &[String]) -> Result<(), Box<dyn Error>> {
    check_internal(m, true, standards)
}

pub fn check_abi(m: &Manifest, standards: &[String]) -> Result<(), Box<dyn Error>> {
    check_internal(m, false, standards)
}

fn check_internal(m: &Manifest, check_names: bool, standards: &[String]) -> Result<(), Box<dyn Error>> {
    for standard in standards {
        if let Some(ss) = CHECKS.get(standard) {
            let mut last_err = None;
            for s in ss {
                if let Err(e) = comply_internal(m, check_names, s) {
                    last_err = Some(e);
                } else {
                    last_err = None;
                    break;
                }
            }
            if let Some(err) = last_err {
                return Err(format!("manifest is not compliant with '{}': {}", standard, err).into());
            }
        }
    }
    Ok(())
}

pub fn comply(m: &Manifest, st: &Standard) -> Result<(), ComplianceError> {
    comply_internal(m, true, st)
}

pub fn comply_abi(m: &Manifest, st: &Standard) -> Result<(), ComplianceError> {
    comply_internal(m, false, st)
}

fn comply_internal(m: &Manifest, check_names: bool, st: &Standard) -> Result<(), ComplianceError> {
    if let Some(base) = &st.base {
        comply_internal(m, check_names, base)?;
    }

    for stm in &st.abi.methods {
        check_method(m, stm, false, check_names)?;
    }

    for ste in &st.abi.events {
        let name = &ste.name;
        let ed = m.abi.get_event(name);
        if ed.is_none() {
            return Err(ComplianceError::EventMissing(name.clone()));
        }
        let ed = ed.unwrap();
        if ste.parameters.len() != ed.parameters.len() {
            return Err(ComplianceError::InvalidParameterCount(
                name.clone(),
                ste.parameters.len(),
                ed.parameters.len(),
            ));
        }
        for (i, (stp, edp)) in ste.parameters.iter().zip(ed.parameters.iter()).enumerate() {
            if check_names && stp.name != edp.name {
                return Err(ComplianceError::InvalidParameterName(
                    name.clone(),
                    i,
                    stp.name.clone(),
                    edp.name.clone(),
                ));
            }
            if stp.type_ != edp.type_ {
                return Err(ComplianceError::InvalidParameterType(
                    name.clone(),
                    stp.type_.to_string(),
                    edp.type_.to_string(),
                ));
            }
        }
    }

    for stm in &st.optional {
        check_method(m, stm, true, check_names)?;
    }

    Ok(())
}

fn check_method(
    m: &Manifest,
    expected: &Method,
    allow_missing: bool,
    check_names: bool,
) -> Result<(), ComplianceError> {
    let actual = m.abi.get_method(&expected.name, expected.parameters.len());
    if actual.is_none() {
        if allow_missing {
            return Ok(());
        }
        return Err(ComplianceError::MethodMissing(expected.name.clone()));
    }
    let actual = actual.unwrap();

    if expected.return_type != actual.return_type {
        return Err(ComplianceError::InvalidReturnType(
            expected.name.clone(),
            expected.return_type.to_string(),
            actual.return_type.to_string(),
        ));
    }

    for (i, (exp_param, act_param)) in expected
        .parameters
        .iter()
        .zip(actual.parameters.iter())
        .enumerate()
    {
        if check_names && exp_param.name != act_param.name {
            return Err(ComplianceError::InvalidParameterName(
                expected.name.clone(),
                i,
                exp_param.name.clone(),
                act_param.name.clone(),
            ));
        }
        if exp_param.type_ != act_param.type_ {
            return Err(ComplianceError::InvalidParameterType(
                expected.name.clone(),
                exp_param.type_.to_string(),
                act_param.type_.to_string(),
            ));
        }
    }

    if expected.safe != actual.safe {
        return Err(ComplianceError::SafeMethodMismatch(
            expected.name.clone(),
            expected.safe,
        ));
    }

    Ok(())
}
