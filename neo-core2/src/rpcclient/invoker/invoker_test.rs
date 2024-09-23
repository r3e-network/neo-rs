use std::error::Error;
use std::collections::HashMap;
use uuid::Uuid;
use neo_core2::rpcclient::invoker::{Invoker, New, NewHistoricAtHeight, NewHistoricWithState};
use neo_core2::core::transaction::{Signer, Witness};
use neo_core2::neorpc::result::{Invoke, Iterator};
use neo_core2::smartcontract::Parameter;
use neo_core2::util::{Uint160, Uint256};
use neo_core2::vm::stackitem::Item;
use neo_core2::require;

struct RpcInv {
    res_inv: Option<Invoke>,
    res_trm: bool,
    res_itm: Vec<Item>,
    err: Option<Box<dyn Error>>,
}

impl RpcInv {
    fn invoke_contract_verify(&self, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_function(&self, contract: Uint160, operation: String, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_script(&self, script: Vec<u8>, signers: Vec<Signer>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_contract_verify_at_height(&self, height: u32, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_contract_verify_with_state(&self, stateroot: Uint256, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_function_at_height(&self, height: u32, contract: Uint160, operation: String, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_function_with_state(&self, stateroot: Uint256, contract: Uint160, operation: String, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_script_at_height(&self, height: u32, script: Vec<u8>, signers: Vec<Signer>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn invoke_script_with_state(&self, stateroot: Uint256, script: Vec<u8>, signers: Vec<Signer>) -> Result<Option<Invoke>, Box<dyn Error>> {
        Ok(self.res_inv.clone())
    }

    fn terminate_session(&self, session_id: Uuid) -> Result<bool, Box<dyn Error>> {
        if let Some(err) = &self.err {
            return Err(err.clone());
        }
        Ok(self.res_trm)
    }

    fn traverse_iterator(&self, session_id: Uuid, iterator_id: Uuid, max_items_count: i32) -> Result<Vec<Item>, Box<dyn Error>> {
        if let Some(err) = &self.err {
            return Err(err.clone());
        }
        Ok(self.res_itm.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core2::require;

    #[test]
    fn test_invoker() {
        let res_exp = Invoke { state: "HALT".to_string() };
        let ri = RpcInv { res_inv: Some(res_exp.clone()), res_trm: true, res_itm: vec![], err: None };

        let test_inv = |inv: &Invoker| {
            let res = inv.call(Uint160::default(), "method".to_string()).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.verify(Uint160::default(), vec![]).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.run(vec![1]).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.call(Uint160::default(), "method".to_string()).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.verify(Uint160::default(), vec![], "param".to_string()).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.call(Uint160::default(), "method".to_string(), 42).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.verify(Uint160::default(), vec![], HashMap::new());
            require::error(&res);

            let res = inv.call(Uint160::default(), "method".to_string(), HashMap::new());
            require::error(&res);

            let res = inv.call_and_expand_iterator(Uint160::default(), "method".to_string(), 10, 42).unwrap();
            require::no_error(&res);
            require::equal(&res_exp, &res);

            let res = inv.call_and_expand_iterator(Uint160::default(), "method".to_string(), 10, HashMap::new());
            require::error(&res);
        };

        test_inv(&New(ri, None));
        test_inv(&NewHistoricAtHeight(100500, ri, None));
        test_inv(&NewHistoricWithState(Uint256::default(), ri, None));

        let inv = New(&historic_converter { client: ri }, None);
        require::panics(|| inv.call(Uint160::default(), "method".to_string()));
        require::panics(|| inv.verify(Uint160::default(), vec![], "param".to_string()));
        require::panics(|| inv.run(vec![1]));
    }

    #[test]
    fn test_terminate_session() {
        let res_exp = Invoke { state: "HALT".to_string() };
        let mut ri = RpcInv { res_inv: Some(res_exp.clone()), res_trm: true, res_itm: vec![], err: None };

        for inv in vec![New(ri, None), NewHistoricWithState(Uint256::default(), ri, None)] {
            ri.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            require::error(&inv.terminate_session(Uuid::new_v4()));
            ri.err = None;
            ri.res_trm = false;
            require::error(&inv.terminate_session(Uuid::new_v4()));
            ri.res_trm = true;
            require::no_error(&inv.terminate_session(Uuid::new_v4()));
        }
    }

    #[test]
    fn test_traverse_iterator() {
        let res_exp = Invoke { state: "HALT".to_string() };
        let mut ri = RpcInv { res_inv: Some(res_exp.clone()), res_trm: true, res_itm: vec![], err: None };

        for inv in vec![New(ri, None), NewHistoricWithState(Uint256::default(), ri, None)] {
            let res = inv.traverse_iterator(Uuid::new_v4(), &Iterator { values: vec![Item::make(42)] }, 0).unwrap();
            require::no_error(&res);
            require::equal(&vec![Item::make(42)], &res);

            let res = inv.traverse_iterator(Uuid::new_v4(), &Iterator { values: vec![Item::make(42)] }, 1).unwrap();
            require::no_error(&res);
            require::equal(&vec![Item::make(42)], &res);

            let res = inv.traverse_iterator(Uuid::new_v4(), &Iterator { values: vec![Item::make(42)] }, 2).unwrap();
            require::no_error(&res);
            require::equal(&vec![Item::make(42)], &res);

            ri.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let res = inv.traverse_iterator(Uuid::new_v4(), &Iterator { id: Some(Uuid::new_v4()) }, 2);
            require::error(&res);

            ri.err = None;
            ri.res_itm = vec![Item::make(42)];
            let res = inv.traverse_iterator(Uuid::new_v4(), &Iterator { id: Some(Uuid::new_v4()) }, 2).unwrap();
            require::no_error(&res);
            require::equal(&vec![Item::make(42)], &res);
        }
    }

    #[test]
    fn test_invoker_signers() {
        let res_exp = Invoke { state: "HALT".to_string() };
        let ri = RpcInv { res_inv: Some(res_exp.clone()), res_trm: true, res_itm: vec![], err: None };
        let mut inv = New(ri, None);

        require::is_none(&inv.signers());

        let s = vec![];
        inv = New(ri, Some(s.clone()));
        require::equal(&s, &inv.signers());

        let s = vec![Signer { account: Uint160::from([1, 2, 3]), scopes: transaction::CalledByEntry }];
        inv = New(ri, Some(s.clone()));
        require::equal(&s, &inv.signers());
    }
}
