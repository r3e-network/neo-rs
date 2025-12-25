//
// native_impl.rs - NativeContract trait implementation for ContractManagement
//

use super::*;

impl NativeContract for ContractManagement {
    fn id(&self) -> i32 {
        self.id
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        self.hydrate_from_engine(engine)
    }

    fn name(&self) -> &str {
        "ContractManagement"
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "getContract" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getContract requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt160::from_bytes(&args[0])
                    .map_err(|e| Error::invalid_argument(format!("Invalid hash: {}", e)))?;
                match self.get_contract(&hash)? {
                    Some(contract) => {
                        // Serialize contract state
                        let mut writer = BinaryWriter::new();
                        contract.serialize(&mut writer).map_err(|e| {
                            Error::serialization(format!("Failed to serialize contract: {}", e))
                        })?;
                        Ok(writer.to_bytes())
                    }
                    None => Ok(vec![]),
                }
            }
            "deploy" => {
                if args.len() != 3 {
                    return Err(Error::invalid_argument(
                        "deploy requires 3 arguments".to_string(),
                    ));
                }
                let nef_bytes = args[0].clone();
                let manifest_bytes = args[1].clone();
                let data = args[2].clone();

                let contract = self.deploy(engine, nef_bytes, manifest_bytes, data)?;

                // Serialize contract state
                let mut writer = BinaryWriter::new();
                contract.serialize(&mut writer).map_err(|e| {
                    Error::serialization(format!("Failed to serialize contract: {}", e))
                })?;
                Ok(writer.to_bytes())
            }
            "update" => {
                if args.len() != 3 {
                    return Err(Error::invalid_argument(
                        "update requires 3 arguments".to_string(),
                    ));
                }

                let nef_bytes = if args[0].is_empty() {
                    None
                } else {
                    Some(args[0].clone())
                };

                let manifest_bytes = if args[1].is_empty() {
                    None
                } else {
                    Some(args[1].clone())
                };

                let data = args[2].clone();

                self.update(engine, nef_bytes, manifest_bytes, data)?;
                Ok(vec![])
            }
            "destroy" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "destroy requires no arguments".to_string(),
                    ));
                }
                self.destroy(engine)?;
                Ok(vec![])
            }
            "getMinimumDeploymentFee" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "getMinimumDeploymentFee requires no arguments".to_string(),
                    ));
                }
                let fee = self.get_minimum_deployment_fee()?;
                Ok(fee.to_le_bytes().to_vec())
            }
            "setMinimumDeploymentFee" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "setMinimumDeploymentFee requires 1 argument".to_string(),
                    ));
                }
                if args[0].len() != 8 {
                    return Err(Error::invalid_argument("Invalid fee value".to_string()));
                }
                let value = i64::from_le_bytes(
                    args[0]
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::invalid_argument("Invalid fee value".to_string()))?,
                );
                self.set_minimum_deployment_fee(engine, value)?;
                Ok(vec![])
            }
            "hasMethod" => {
                if args.len() != 3 {
                    return Err(Error::invalid_argument(
                        "hasMethod requires 3 arguments".to_string(),
                    ));
                }
                let hash = UInt160::from_bytes(&args[0])
                    .map_err(|e| Error::invalid_argument(format!("Invalid hash: {}", e)))?;
                let method = String::from_utf8(args[1].clone()).map_err(|e| {
                    Error::invalid_argument(format!("Invalid method string: {}", e))
                })?;
                if args[2].len() != 4 {
                    return Err(Error::invalid_argument(
                        "Invalid parameter count".to_string(),
                    ));
                }
                let pcount =
                    i32::from_le_bytes(args[2].as_slice().try_into().map_err(|_| {
                        Error::invalid_argument("Invalid parameter count".to_string())
                    })?);
                let result = self.has_method(&hash, &method, pcount)?;
                Ok(vec![if result { 1 } else { 0 }])
            }
            "isContract" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "isContract requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt160::from_bytes(&args[0])
                    .map_err(|e| Error::invalid_argument(format!("Invalid hash: {}", e)))?;
                let result = Self::is_contract(engine.snapshot_cache().as_ref(), &hash)?;
                Ok(vec![if result { 1 } else { 0 }])
            }
            "getContractById" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getContractById requires 1 argument".to_string(),
                    ));
                }
                if args[0].len() != 4 {
                    return Err(Error::invalid_argument("Invalid contract ID".to_string()));
                }
                let id = i32::from_le_bytes(
                    args[0]
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::invalid_argument("Invalid contract ID".to_string()))?,
                );
                match self.get_contract_by_id(id)? {
                    Some(contract) => {
                        // Serialize contract state
                        let mut writer = BinaryWriter::new();
                        contract.serialize(&mut writer).map_err(|e| {
                            Error::serialization(format!("Failed to serialize contract: {}", e))
                        })?;
                        Ok(writer.to_bytes())
                    }
                    None => Ok(vec![]),
                }
            }
            "getContractHashes" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "getContractHashes requires no arguments".to_string(),
                    ));
                }
                let iterator_id = self.get_contract_hashes_iterator(engine)?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            _ => Err(Error::native_contract(format!(
                "Method {} not found",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn on_persist(&self, _engine: &mut ApplicationEngine) -> Result<()> {
        // No special persistence logic needed
        Ok(())
    }
}

impl Default for ContractManagement {
    fn default() -> Self {
        Self::new()
    }
}
