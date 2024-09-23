use crate::smartcontract::{self, ParamType};
use crate::vm::stackitem::{self, Item, StackItem};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    pub offset: i32,
    pub parameters: Vec<Parameter>,
    pub return_type: ParamType,
    pub safe: bool,
}

#[derive(Error, Debug)]
pub enum MethodError {
    #[error("empty or absent name")]
    EmptyName,
    #[error("negative offset")]
    NegativeOffset,
    #[error("invalid return type")]
    InvalidReturnType(#[from] smartcontract::Error),
    #[error("invalid parameters")]
    InvalidParameters(#[from] ParametersError),
}

impl Method {
    pub fn is_valid(&self) -> Result<(), MethodError> {
        if self.name.is_empty() {
            return Err(MethodError::EmptyName);
        }
        if self.offset < 0 {
            return Err(MethodError::NegativeOffset);
        }
        smartcontract::convert_to_param_type(self.return_type as i32)?;
        Parameters::are_valid(&self.parameters)?;
        Ok(())
    }

    pub fn to_stack_item(&self) -> StackItem {
        let params: Vec<StackItem> = self.parameters.iter()
            .map(|p| p.to_stack_item())
            .collect();
        
        StackItem::Struct(vec![
            StackItem::ByteString(self.name.clone().into()),
            StackItem::Array(params),
            StackItem::Integer((self.return_type as i32).into()),
            StackItem::Integer(self.offset.into()),
            StackItem::Boolean(self.safe),
        ])
    }

    pub fn from_stack_item(item: &StackItem) -> Result<Self, MethodError> {
        match item {
            StackItem::Struct(method) if method.len() == 5 => {
                let name = method[0].try_string()?;
                let params = match &method[1] {
                    StackItem::Array(params) => params.iter()
                        .map(|p| Parameter::from_stack_item(p))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(MethodError::InvalidParameters(ParametersError::InvalidType)),
                };
                let return_type = ParamType::try_from(method[2].try_integer()? as i32)?;
                let offset = method[3].try_integer()? as i32;
                let safe = method[4].try_bool()?;

                Ok(Method {
                    name,
                    offset,
                    parameters: params,
                    return_type,
                    safe,
                })
            },
            _ => Err(MethodError::InvalidParameters(ParametersError::InvalidType)),
        }
    }
}

// Note: Parameter and Parameters structs/implementations are not shown here
// They should be defined in a separate file or module
