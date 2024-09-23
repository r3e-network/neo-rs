use std::fmt;
use std::sync::Arc;
use std::str::FromStr;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use serde::{Serialize, Deserialize};
use num_bigint::BigInt;
use num_traits::Zero;
use neo_core::blockchain::Blockchain;
use neo_core::block::Block;
use neo_core::transaction::{Transaction, Signer, Witness};
use neo_core::state::{AppExecResult, NotificationEvent};
use neo_core::util::{Uint160, Uint256};
use neo_core::vm::{VM, VMState};
use neo_core::smartcontract::{Contract, CallFlags};
use neo_core::trigger::TriggerType;
use neo_core::wallet::Wallet;
use neo_core::io::Io;
use neo_core::fee::Fee;
use neo_core::native_contracts::NativeContract;
use neo_core::config::NetMode;
use neo_core::vm::stackitem::StackItem;
use neo_core::vm::vmstate::VMState;
use neo_core::wallet::Wallet;
use neo_core::require::Require;

pub struct Executor {
    chain: Arc<Blockchain>,
    validator: Signer,
    committee: Signer,
    committee_hash: Uint160,
    collect_coverage: bool,
}

impl Executor {
    pub fn new(t: &mut dyn Require, bc: Arc<Blockchain>, validator: Signer, committee: Signer) -> Self {
        check_multi_signer(t, &validator);
        check_multi_signer(t, &committee);

        Executor {
            chain: bc,
            validator,
            committee,
            committee_hash: committee.script_hash(),
            collect_coverage: is_coverage_enabled(),
        }
    }

    pub fn top_block(&self, t: &mut dyn Require) -> Block {
        let block = self.chain.get_block(self.chain.get_header_hash(self.chain.block_height())).unwrap();
        t.require_no_error(&block);
        block
    }

    pub fn native_hash(&self, t: &mut dyn Require, name: &str) -> Uint160 {
        let hash = self.chain.get_native_contract_script_hash(name).unwrap();
        t.require_no_error(&hash);
        hash
    }

    pub fn contract_hash(&self, t: &mut dyn Require, id: i32) -> Uint160 {
        let hash = self.chain.get_contract_script_hash(id).unwrap();
        t.require_no_error(&hash);
        hash
    }

    pub fn native_id(&self, t: &mut dyn Require, name: &str) -> i32 {
        let hash = self.native_hash(t, name);
        let contract_state = self.chain.get_contract_state(&hash).unwrap();
        t.require_not_nil(&contract_state);
        contract_state.id
    }

    pub fn new_unsigned_tx(&self, t: &mut dyn Require, hash: Uint160, method: &str, args: Vec<StackItem>) -> Transaction {
        let script = smartcontract::create_call_script(hash, method, args).unwrap();
        t.require_no_error(&script);

        let mut tx = Transaction::new(script, 0);
        tx.nonce = nonce();
        tx.valid_until_block = self.chain.block_height() + 1;
        tx
    }

    pub fn new_tx(&self, t: &mut dyn Require, signers: Vec<Signer>, hash: Uint160, method: &str, args: Vec<StackItem>) -> Transaction {
        let mut tx = self.new_unsigned_tx(t, hash, method, args);
        self.sign_tx(t, &mut tx, -1, signers);
        tx
    }

    pub fn sign_tx(&self, t: &mut dyn Require, tx: &mut Transaction, sys_fee: i64, signers: Vec<Signer>) {
        for acc in &signers {
            tx.signers.push(Signer {
                account: acc.script_hash(),
                scopes: transaction::Global,
            });
        }
        add_network_fee(t, &self.chain, tx, &signers);
        self.add_system_fee(tx, sys_fee);

        for acc in &signers {
            t.require_no_error(&acc.sign_tx(self.chain.get_config().magic, tx));
        }
    }

    pub fn new_account(&self, t: &mut dyn Require, expected_gas_balance: Option<i64>) -> Signer {
        let acc = Wallet::new_account().unwrap();
        t.require_no_error(&acc);

        let amount = expected_gas_balance.unwrap_or(100_0000_0000);
        let tx = self.new_tx(t, vec![self.validator.clone()],
            self.native_hash(t, nativenames::GAS), "transfer",
            vec![self.validator.script_hash(), acc.contract.script_hash(), amount.into(), StackItem::Null]);
        self.add_new_block(t, vec![tx]);
        self.check_halt(t, tx.hash());
        Signer::new_single(acc)
    }

    pub fn deploy_contract(&self, t: &mut dyn Require, c: &Contract, data: Option<StackItem>) -> Uint256 {
        self.deploy_contract_by(t, self.validator.clone(), c, data)
    }

    pub fn deploy_contract_by(&self, t: &mut dyn Require, signer: Signer, c: &Contract, data: Option<StackItem>) -> Uint256 {
        self.track_coverage(t, c);
        let tx = self.new_deploy_tx_by(t, signer, c, data);
        self.add_new_block(t, vec![tx]);
        self.check_halt(t, tx.hash());

        self.check_tx_notification_event(t, tx.hash(), -1, NotificationEvent {
            script_hash: self.native_hash(t, nativenames::MANAGEMENT),
            name: "Deploy".to_string(),
            item: StackItem::Array(vec![StackItem::ByteArray(c.hash().to_bytes_be())]),
        });

        tx.hash()
    }

    pub fn deploy_contract_check_fault(&self, t: &mut dyn Require, c: &Contract, data: Option<StackItem>, err_message: &str) {
        self.track_coverage(t, c);
        let tx = self.new_deploy_tx(t, c, data);
        self.add_new_block(t, vec![tx]);
        self.check_fault(t, tx.hash(), err_message);
    }

    pub fn track_coverage(&self, t: &mut dyn Require, c: &Contract) {
        if self.collect_coverage {
            add_script_to_coverage(c);
            t.cleanup(|| {
                report_coverage(t);
            });
        }
    }

    pub fn invoke_script(&self, t: &mut dyn Require, script: Vec<u8>, signers: Vec<Signer>) -> Uint256 {
        let tx = self.prepare_invocation(t, script, signers);
        self.add_new_block(t, vec![tx]);
        tx.hash()
    }

    pub fn prepare_invocation(&self, t: &mut dyn Require, script: Vec<u8>, signers: Vec<Signer>, valid_until_block: Option<u32>) -> Transaction {
        let mut tx = self.prepare_invocation_no_sign(t, script, valid_until_block);
        self.sign_tx(t, &mut tx, -1, signers);
        tx
    }

    pub fn prepare_invocation_no_sign(&self, t: &mut dyn Require, script: Vec<u8>, valid_until_block: Option<u32>) -> Transaction {
        let mut tx = Transaction::new(script, 0);
        tx.nonce = nonce();
        tx.valid_until_block = self.chain.block_height() + 1;
        if let Some(vub) = valid_until_block {
            tx.valid_until_block = vub;
        }
        tx
    }

    pub fn invoke_script_check_halt(&self, t: &mut dyn Require, script: Vec<u8>, signers: Vec<Signer>, stack: Vec<StackItem>) {
        let hash = self.invoke_script(t, script, signers);
        self.check_halt(t, hash, stack);
    }

    pub fn invoke_script_check_fault(&self, t: &mut dyn Require, script: Vec<u8>, signers: Vec<Signer>, err_message: &str) -> Uint256 {
        let hash = self.invoke_script(t, script, signers);
        self.check_fault(t, hash, err_message);
        hash
    }

    pub fn check_halt(&self, t: &mut dyn Require, h: Uint256, stack: Vec<StackItem>) -> AppExecResult {
        let aer = self.chain.get_app_exec_results(h, TriggerType::Application).unwrap();
        t.require_no_error(&aer);
        t.require_equal(&VMState::Halt, &aer[0].vm_state, &aer[0].fault_exception);
        if !stack.is_empty() {
            t.require_equal(&stack, &aer[0].stack);
        }
        aer[0].clone()
    }

    pub fn check_fault(&self, t: &mut dyn Require, h: Uint256, s: &str) {
        let aer = self.chain.get_app_exec_results(h, TriggerType::Application).unwrap();
        t.require_no_error(&aer);
        t.require_equal(&VMState::Fault, &aer[0].vm_state);
        t.require_true(&aer[0].fault_exception.contains(s), &format!("expected: {}, got: {}", s, aer[0].fault_exception));
    }

    pub fn check_tx_notification_event(&self, t: &mut dyn Require, h: Uint256, index: i32, expected: NotificationEvent) {
        let aer = self.chain.get_app_exec_results(h, TriggerType::Application).unwrap();
        t.require_no_error(&aer);
        let l = aer[0].events.len();
        let idx = if index < 0 { l + index as usize } else { index as usize };
        t.require_true(&(0 <= idx && idx < l), &format!("notification index is out of range: want {}, len is {}", index, l));
        t.require_equal(&expected, &aer[0].events[idx]);
    }

    pub fn check_gas_balance(&self, t: &mut dyn Require, acc: Uint160, expected: &BigInt) {
        let actual = self.chain.get_utility_token_balance(acc);
        t.require_equal(expected, &actual, &format!("invalid GAS balance: expected {}, got {}", expected, actual));
    }

    pub fn ensure_gas_balance(&self, t: &mut dyn Require, acc: Uint160, is_ok: impl Fn(&BigInt) -> bool) {
        let actual = self.chain.get_utility_token_balance(acc);
        t.require_true(&is_ok(&actual), &format!("invalid GAS balance: got {}, condition is not satisfied", actual));
    }

    pub fn new_deploy_tx(&self, t: &mut dyn Require, c: &Contract, data: Option<StackItem>) -> Transaction {
        self.new_deploy_tx_by(t, self.validator.clone(), c, data)
    }

    pub fn new_deploy_tx_by(&self, t: &mut dyn Require, signer: Signer, c: &Contract, data: Option<StackItem>) -> Transaction {
        let raw_manifest = serde_json::to_vec(&c.manifest).unwrap();
        t.require_no_error(&raw_manifest);

        let neb = c.nef.bytes().unwrap();
        t.require_no_error(&neb);

        let script = smartcontract::create_call_script(self.chain.management_contract_hash(), "deploy", vec![neb, raw_manifest, data.unwrap_or(StackItem::Null)]).unwrap();
        t.require_no_error(&script);

        let mut tx = Transaction::new(script, 0);
        tx.nonce = nonce();
        tx.valid_until_block = self.chain.block_height() + 1;
        tx.signers.push(Signer {
            account: signer.script_hash(),
            scopes: transaction::Global,
        });
        add_network_fee(t, &self.chain, &mut tx, &[signer.clone()]);
        self.add_system_fee(&mut tx, -1);
        t.require_no_error(&signer.sign_tx(self.chain.get_config().magic, &mut tx));
        tx
    }

    pub fn add_system_fee(&self, tx: &mut Transaction, sys_fee: i64) {
        if sys_fee >= 0 {
            tx.system_fee = sys_fee;
        } else {
            let v = self.test_invoke(tx).unwrap();
            tx.system_fee = v.gas_consumed();
        }
    }

    pub fn add_new_block(&self, t: &mut dyn Require, txs: Vec<Transaction>) -> Block {
        let mut b = self.new_unsigned_block(t, txs);
        self.sign_block(&mut b);
        t.require_no_error(&self.chain.add_block(b.clone()));
        b
    }

    pub fn generate_new_blocks(&self, t: &mut dyn Require, count: usize) -> Vec<Block> {
        (0..count).map(|_| self.add_new_block(t, vec![])).collect()
    }

    pub fn sign_block(&self, b: &mut Block) {
        let invoc = self.validator.sign_hashable(self.chain.get_config().magic, b);
        b.script.invocation_script = invoc;
    }

    pub fn add_block_check_halt(&self, t: &mut dyn Require, txs: Vec<Transaction>) -> Block {
        let b = self.add_new_block(t, txs.clone());
        for tx in txs {
            self.check_halt(t, tx.hash());
        }
        b
    }

    pub fn test_invoke(&self, tx: &Transaction) -> Result<VM, String> {
        let last_block = self.chain.get_block(self.chain.get_header_hash(self.chain.block_height())).unwrap();
        let mut b = Block::new();
        b.header.index = self.chain.block_height() + 1;
        b.header.timestamp = last_block.timestamp + 1;

        let mut ttx = tx.clone();
        let mut ic = self.chain.get_test_vm(TriggerType::Application, &mut ttx, &b).unwrap();

        if self.collect_coverage {
            ic.vm.set_on_exec_hook(coverage_hook);
        }

        ic.vm.load_with_flags(tx.script.clone(), CallFlags::All);
        ic.vm.run()?;
        Ok(ic.vm)
    }

    pub fn get_transaction(&self, t: &mut dyn Require, h: Uint256) -> (Transaction, u32) {
        let (tx, height) = self.chain.get_transaction(h).unwrap();
        t.require_no_error(&tx);
        (tx, height)
    }

    pub fn get_block_by_index(&self, t: &mut dyn Require, idx: u32) -> Block {
        let h = self.chain.get_header_hash(idx);
        t.require_not_empty(&h);
        let b = self.chain.get_block(h).unwrap();
        t.require_no_error(&b);
        b
    }

    pub fn get_tx_exec_result(&self, t: &mut dyn Require, h: Uint256) -> AppExecResult {
        let aer = self.chain.get_app_exec_results(h, TriggerType::Application).unwrap();
        t.require_no_error(&aer);
        t.require_equal(&1, &aer.len());
        aer[0].clone()
    }

    pub fn enable_coverage(&mut self) {
        self.collect_coverage = is_coverage_enabled();
    }

    pub fn disable_coverage(&mut self) {
        self.collect_coverage = false;
    }
}
