use neo_vm::stack_item::StackItem;
use crate::neo_contract::contract_parameter::ContractParameterType;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::manifest::contract_parameter_definition::ContractParameterDefinition;

/// Represents a method in a smart contract ABI.
#[derive(Clone, Debug)]
pub struct ContractMethodDescriptor {
    /// The name of the method.
    pub name: String,
    /// The parameters of the method.
    pub parameters: Vec<ContractParameterDefinition>,
    /// Indicates the return type of the method.
    pub return_type: ContractParameterType,
    /// The position of the method in the contract script.
    pub offset: i32,
    /// Indicates whether the method is a safe method.
    /// If a method is marked as safe, the user interface will not give any warnings when it is called by other contracts.
    pub safe: bool,
}

impl IInteroperable for ContractMethodDescriptor {
    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Error> {
        if let StackItem::Struct(s) = stack_item {
            let name = s.get(0).ok_or(Error::InvalidFormat)?.as_string()?;
            let parameters = s.get(1).ok_or(Error::InvalidFormat)?
                .as_array()?
                .iter()
                .map(|p| ContractParameterDefinition::from_stack_item(p))
                .collect::<Result<Vec<_>, _>>()?;
            let return_type = ContractParameterType::from_u8(s.get(2).ok_or(Error::InvalidFormat)?.as_integer()? as u8)?;
            let offset = s.get(3).ok_or(Error::InvalidFormat)?.as_integer()? as i32;
            let safe = s.get(4).ok_or(Error::InvalidFormat)?.as_bool()?;
            Ok(Self { name, parameters, return_type, offset, safe })
        } else {
            Err(Error::InvalidFormat)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Struct(Struct::new(vec![
            StackItem::String(self.name.clone()),
            StackItem::Array(self.parameters.iter().map(|p| p.to_stack_item()).collect()),
            StackItem::Integer(self.return_type as u8 as i32),
            StackItem::Integer(self.offset as i32),
            StackItem::Boolean(self.safe),
        ])))
    }
    
    type Error = std::io::Error;
}

impl ContractMethodDescriptor {
    /// Converts the method from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The method represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted method.
    pub fn from_json(json: &Json) -> Result<Self, Error> {
        let name = json.get("name")
            .ok_or(Error::InvalidFormat)?
            .as_string()
            .ok_or(Error::InvalidFormat)?;
        
        if name.is_empty() {
            return Err(Error::InvalidFormat);
        }

        let parameters = json.get("parameters")
            .ok_or(Error::InvalidFormat)?
            .as_array()
            .ok_or(Error::InvalidFormat)?
            .iter()
            .map(|u| ContractParameterDefinition::from_json(u))
            .collect::<Result<Vec<_>, _>>()?;

        // Validate that parameter names are unique
        let mut param_names = std::collections::HashSet::new();
        for param in &parameters {
            if !param_names.insert(param.name.clone()) {
                return Err(Error::InvalidFormat);
            }
        }

        let return_type = ContractParameterType::from_str(
            json.get("returntype")
                .ok_or(Error::InvalidFormat)?
                .as_string()
                .ok_or(Error::InvalidFormat)?
        )?;

        let offset = json.get("offset")
            .ok_or(Error::InvalidFormat)?
            .as_i64()
            .ok_or(Error::InvalidFormat)? as i32;

        if offset < 0 {
            return Err(Error::InvalidFormat);
        }

        let safe = json.get("safe")
            .ok_or(Error::InvalidFormat)?
            .as_bool()
            .ok_or(Error::InvalidFormat)?;

        Ok(Self {
            name: name.to_string(),
            parameters,
            return_type,
            offset,
            safe,
        })
    }

    /// Converts the method to a JSON object.
    ///
    /// # Returns
    ///
    /// The method represented by a JSON object.
    pub fn to_json(&self) -> Json {
        let mut json = Json::new_object();
        json.insert("name", Json::from(self.name.clone()));
        json.insert("parameters", Json::from(self.parameters.iter().map(|u| u.to_json()).collect::<Vec<_>>()));
        json.insert("returntype", Json::from(self.return_type.to_string()));
        json.insert("offset", Json::from(self.offset));
        json.insert("safe", Json::from(self.safe));
        json
    }
}
