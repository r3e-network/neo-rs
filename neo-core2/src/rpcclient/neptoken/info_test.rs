use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;
use neo_core2::core::state;
use neo_core2::core::transaction;
use neo_core2::neorpc::result;
use neo_core2::smartcontract;
use neo_core2::smartcontract::manifest;
use neo_core2::util;
use neo_core2::vm::stackitem;
use neo_core2::wallet;
use anyhow::Result;

struct RpcClient {
    cnt: usize,
    cserr: Option<Box<dyn Error>>,
    cs: Option<Arc<state::Contract>>,
    inverrs: Vec<Option<Box<dyn Error>>>,
    invs: Vec<Option<Arc<result::Invoke>>>,
}

impl RpcClient {
    fn invoke_contract_verify(&self, _contract: util::Uint160, _params: Vec<smartcontract::Parameter>, _signers: Vec<transaction::Signer>, _witnesses: Vec<transaction::Witness>) -> Result<Arc<result::Invoke>> {
        panic!("not implemented")
    }

    fn invoke_function(&mut self, _contract: util::Uint160, _operation: &str, _params: Vec<smartcontract::Parameter>, _signers: Vec<transaction::Signer>) -> Result<Arc<result::Invoke>> {
        let e = self.inverrs[self.cnt].clone();
        let i = self.invs[self.cnt].clone();
        self.cnt = (self.cnt + 1) % self.invs.len();
        match e {
            Some(err) => Err(anyhow::Error::new(err)),
            None => Ok(i.unwrap()),
        }
    }

    fn invoke_script(&self, _script: Vec<u8>, _signers: Vec<transaction::Signer>) -> Result<Arc<result::Invoke>> {
        panic!("not implemented")
    }

    fn terminate_session(&self, _session_id: Uuid) -> Result<bool> {
        panic!("not implemented")
    }

    fn traverse_iterator(&self, _session_id: Uuid, _iterator_id: Uuid, _max_items_count: usize) -> Result<Vec<stackitem::Item>> {
        panic!("not implemented")
    }

    fn get_contract_state_by_hash(&self, _hash: util::Uint160) -> Result<Arc<state::Contract>> {
        match &self.cs {
            Some(cs) => Ok(cs.clone()),
            None => Err(anyhow::Error::new(self.cserr.clone().unwrap())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_info() -> Result<()> {
        let mut c = RpcClient {
            cnt: 0,
            cserr: None,
            cs: None,
            inverrs: vec![],
            invs: vec![],
        };
        let hash = util::Uint160::from_slice(&[1, 2, 3]);

        // Error on contract state.
        c.cserr = Some(Box::new(anyhow!("")));
        let err = info(&c, hash.clone()).unwrap_err();
        assert!(err.is_error());

        // Error on missing standard.
        c.cserr = None;
        c.cs = Some(Arc::new(state::Contract {
            contract_base: state::ContractBase {
                manifest: manifest::Manifest {
                    name: "Vasiliy".to_string(),
                    supported_standards: vec!["RFC 1149".to_string()],
                },
            },
        }));
        let err = info(&c, hash.clone()).unwrap_err();
        assert!(err.is_error());

        // Error on Symbol()
        c.cs = Some(Arc::new(state::Contract {
            contract_base: state::ContractBase {
                manifest: manifest::Manifest {
                    name: "Übertoken".to_string(),
                    supported_standards: vec!["NEP-17".to_string()],
                },
            },
        }));
        c.inverrs = vec![Some(Box::new(anyhow!(""))), None];
        c.invs = vec![None, None];
        let err = info(&c, hash.clone()).unwrap_err();
        assert!(err.is_error());

        // Error on Decimals()
        c.cnt = 0;
        c.inverrs.swap(0, 1);
        c.invs[0] = Some(Arc::new(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from("UBT")],
        }));
        let err = info(&c, hash.clone()).unwrap_err();
        assert!(err.is_error());

        // OK
        c.cnt = 0;
        c.inverrs[1] = None;
        c.invs[1] = Some(Arc::new(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(8)],
        }));
        let ti = info(&c, hash.clone())?;
        assert_eq!(
            ti,
            Arc::new(wallet::Token {
                name: "Übertoken".to_string(),
                hash: hash.clone(),
                decimals: 8,
                symbol: "UBT".to_string(),
                standard: "NEP-17".to_string(),
            })
        );

        // NEP-11
        c.cs = Some(Arc::new(state::Contract {
            contract_base: state::ContractBase {
                manifest: manifest::Manifest {
                    name: "NFTizer".to_string(),
                    supported_standards: vec!["NEP-11".to_string()],
                },
            },
        }));
        c.cnt = 0;
        c.inverrs[1] = None;
        c.invs[0] = Some(Arc::new(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from("NZ")],
        }));
        c.invs[1] = Some(Arc::new(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(0)],
        }));
        let ti = info(&c, hash.clone())?;
        assert_eq!(
            ti,
            Arc::new(wallet::Token {
                name: "NFTizer".to_string(),
                hash: hash.clone(),
                decimals: 0,
                symbol: "NZ".to_string(),
                standard: "NEP-11".to_string(),
            })
        );

        Ok(())
    }
}
