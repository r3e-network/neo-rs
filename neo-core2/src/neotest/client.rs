use std::sync::Arc;
use crate::core::transaction::{Transaction, Signer};
use crate::smartcontract::{callflag, trigger};
use crate::util::Uint160;
use crate::vm::{self, stackitem, vmstate};
use crate::testing::TB;
use crate::require;

pub struct ContractInvoker {
    executor: Arc<Executor>,
    hash: Uint160,
    signers: Vec<Signer>,
}

impl ContractInvoker {
    pub fn new_invoker(executor: Arc<Executor>, hash: Uint160, signers: Vec<Signer>) -> Self {
        Self {
            executor,
            hash,
            signers,
        }
    }

    pub fn committee_invoker(executor: Arc<Executor>, hash: Uint160) -> Self {
        Self {
            executor: executor.clone(),
            hash,
            signers: vec![executor.committee.clone()],
        }
    }

    pub fn validator_invoker(executor: Arc<Executor>, hash: Uint160) -> Self {
        Self {
            executor: executor.clone(),
            hash,
            signers: vec![executor.validator.clone()],
        }
    }

    pub fn test_invoke_script(&self, t: &dyn TB, script: &[u8], signers: Vec<Signer>, valid_until_block: Option<u32>) -> Result<vm::Stack, Box<dyn std::error::Error>> {
        let mut tx = self.prepare_invocation_no_sign(t, script, valid_until_block);
        for acc in signers {
            tx.signers.push(Signer {
                account: acc.script_hash(),
                scopes: transaction::Global,
            });
        }
        let b = self.new_unsigned_block(t, &tx);
        let (mut ic, err) = self.chain.get_test_vm(trigger::Application, &tx, &b)?;
        t.cleanup(ic.finalize);

        if self.collect_coverage {
            ic.vm.set_on_exec_hook(coverage_hook);
        }

        ic.vm.load_with_flags(&tx.script, callflag::All);
        ic.vm.run()?;
        Ok(ic.vm.estack())
    }

    pub fn test_invoke(&self, t: &dyn TB, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Result<vm::Stack, Box<dyn std::error::Error>> {
        let tx = self.prepare_invoke_no_sign(t, method, args);
        let b = self.new_unsigned_block(t, &tx);
        let (mut ic, err) = self.chain.get_test_vm(trigger::Application, &tx, &b)?;
        t.cleanup(ic.finalize);

        if self.collect_coverage {
            ic.vm.set_on_exec_hook(coverage_hook);
        }

        ic.vm.load_with_flags(&tx.script, callflag::All);
        ic.vm.run()?;
        Ok(ic.vm.estack())
    }

    pub fn with_signers(&self, signers: Vec<Signer>) -> Self {
        let mut new_c = self.clone();
        new_c.signers = signers;
        new_c
    }

    pub fn prepare_invoke(&self, t: &dyn TB, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Transaction {
        self.executor.new_tx(t, &self.signers, &self.hash, method, args)
    }

    pub fn prepare_invoke_no_sign(&self, t: &dyn TB, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Transaction {
        self.executor.new_unsigned_tx(t, &self.hash, method, args)
    }

    pub fn invoke(&self, t: &dyn TB, result: Box<dyn std::any::Any>, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Uint256 {
        let tx = self.prepare_invoke(t, method, args);
        self.add_new_block(t, &tx);
        self.check_halt(t, tx.hash(), stackitem::make(result));
        tx.hash()
    }

    pub fn invoke_and_check(&self, t: &dyn TB, check_result: Option<fn(&dyn TB, Vec<stackitem::Item>)>, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Uint256 {
        let tx = self.prepare_invoke(t, method, args);
        self.add_new_block(t, &tx);
        let aer = self.chain.get_app_exec_results(tx.hash(), trigger::Application).unwrap();
        require::no_error(t, &aer);
        require::equal(t, vmstate::Halt, aer[0].vm_state, &aer[0].fault_exception);
        if let Some(check_result) = check_result {
            check_result(t, aer[0].stack);
        }
        tx.hash()
    }

    pub fn invoke_with_fee_fail(&self, t: &dyn TB, message: &str, sys_fee: i64, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Uint256 {
        let tx = self.prepare_invoke_no_sign(t, method, args);
        self.executor.sign_tx(t, &tx, sys_fee, &self.signers);
        self.add_new_block(t, &tx);
        self.check_fault(t, tx.hash(), message);
        tx.hash()
    }

    pub fn invoke_fail(&self, t: &dyn TB, message: &str, method: &str, args: Vec<Box<dyn std::any::Any>>) -> Uint256 {
        let tx = self.prepare_invoke(t, method, args);
        self.add_new_block(t, &tx);
        self.check_fault(t, tx.hash(), message);
        tx.hash()
    }
}
