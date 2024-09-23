use std::error::Error;
use std::fmt;

use crate::core::transaction::{self, Transaction};
use crate::neorpc::result::Invoke;
use crate::util::Uint160;
use crate::vm::vmstate::VmState;

// TransactionCheckerModifier is a callback that receives the result of
// test-invocation and the transaction that can perform the same invocation
// on chain. This callback is accepted by methods that create transactions, it
// can examine both arguments and return an error if there is anything wrong
// there which will abort the creation process. Notice that when used this
// callback is completely responsible for invocation result checking, including
// checking for HALT execution state (so if you don't check for it in a callback
// you can send a transaction that is known to end up in FAULT state). It can
// also modify the transaction (see TransactionModifier).
type TransactionCheckerModifier = fn(&Invoke, &mut Transaction) -> Result<(), Box<dyn Error>>;

// TransactionModifier is a callback that receives the transaction before
// it's signed from a method that creates signed transactions. It can check
// fees and other fields of the transaction and return an error if there is
// anything wrong there which will abort the creation process. It also can modify
// Nonce, SystemFee, NetworkFee and ValidUntilBlock values taking full
// responsibility on the effects of these modifications (smaller fee values, too
// low or too high ValidUntilBlock or bad Nonce can render transaction invalid).
// Modifying other fields is not supported. Mostly it's useful for increasing
// fee values since by default they're just enough for transaction to be
// successfully accepted and executed.
type TransactionModifier = fn(&mut Transaction) -> Result<(), Box<dyn Error>>;

// DefaultModifier is the default modifier, it does nothing.
fn default_modifier(tx: &mut Transaction) -> Result<(), Box<dyn Error>> {
    Ok(())
}

// DefaultCheckerModifier is the default TransactionCheckerModifier, it checks
// for HALT state in the invocation result given to it and does nothing else.
fn default_checker_modifier(r: &Invoke, tx: &mut Transaction) -> Result<(), Box<dyn Error>> {
    if r.state != VmState::Halt.to_string() {
        return Err(Box::new(fmt::Error::new(
            fmt::ErrorKind::Other,
            format!("script failed ({} state) due to an error: {}", r.state, r.fault_exception),
        )));
    }
    Ok(())
}

impl Actor {
    // MakeCall creates a transaction that calls the given method of the given
    // contract with the given parameters. Test call is performed and filtered through
    // Actor-configured TransactionCheckerModifier. The resulting transaction has
    // Actor-configured attributes added as well. If you need to override attributes
    // and/or TransactionCheckerModifier use MakeTunedCall.
    fn make_call(&self, contract: Uint160, method: &str, params: Vec<impl Any>) -> Result<Transaction, Box<dyn Error>> {
        self.make_tuned_call(contract, method, None, None, params)
    }

    // MakeTunedCall creates a transaction with the given attributes (or Actor default
    // ones if nil) that calls the given method of the given contract with the given
    // parameters. It's filtered through the provided callback (or Actor default
    // one's if nil, see TransactionCheckerModifier documentation also), so the
    // process can be aborted and transaction can be modified before signing.
    fn make_tuned_call(
        &self,
        contract: Uint160,
        method: &str,
        attrs: Option<Vec<transaction::Attribute>>,
        tx_hook: Option<TransactionCheckerModifier>,
        params: Vec<impl Any>,
    ) -> Result<Transaction, Box<dyn Error>> {
        let (r, err) = self.call(contract, method, params)?;
        self.make_unchecked_wrapper(r, err, attrs, tx_hook)
    }

    // MakeRun creates a transaction with the given executable script. Test
    // invocation of this script is performed and filtered through Actor's
    // TransactionCheckerModifier. The resulting transaction has attributes that are
    // configured for current Actor. If you need to override them or use a different
    // TransactionCheckerModifier use MakeTunedRun.
    fn make_run(&self, script: &[u8]) -> Result<Transaction, Box<dyn Error>> {
        self.make_tuned_run(script, None, None)
    }

    // MakeTunedRun creates a transaction with the given attributes (or Actor default
    // ones if nil) that executes the given script. It's filtered through the
    // provided callback (if not nil, otherwise Actor default one is used, see
    // TransactionCheckerModifier documentation also), so the process can be aborted
    // and transaction can be modified before signing.
    fn make_tuned_run(
        &self,
        script: &[u8],
        attrs: Option<Vec<transaction::Attribute>>,
        tx_hook: Option<TransactionCheckerModifier>,
    ) -> Result<Transaction, Box<dyn Error>> {
        let (r, err) = self.run(script)?;
        self.make_unchecked_wrapper(r, err, attrs, tx_hook)
    }

    fn make_unchecked_wrapper(
        &self,
        r: Invoke,
        err: Box<dyn Error>,
        attrs: Option<Vec<transaction::Attribute>>,
        tx_hook: Option<TransactionCheckerModifier>,
    ) -> Result<Transaction, Box<dyn Error>> {
        if let Some(err) = err {
            return Err(Box::new(fmt::Error::new(
                fmt::ErrorKind::Other,
                format!("test invocation failed: {}", err),
            )));
        }
        self.make_unchecked_run(r.script, r.gas_consumed, attrs, |tx| {
            if let Some(tx_hook) = tx_hook {
                tx_hook(&r, tx)
            } else {
                self.opts.checker_modifier(&r, tx)
            }
        })
    }

    // MakeUncheckedRun creates a transaction with the given attributes (or Actor
    // default ones if nil) that executes the given script and is expected to use
    // up to sysfee GAS for its execution. The transaction is filtered through the
    // provided callback (or Actor default one, see TransactionModifier documentation
    // also), so the process can be aborted and transaction can be modified before
    // signing. This method is mostly useful when test invocation is already
    // performed and the script and required system fee values are already known.
    fn make_unchecked_run(
        &self,
        script: &[u8],
        sysfee: i64,
        attrs: Option<Vec<transaction::Attribute>>,
        tx_hook: TransactionModifier,
    ) -> Result<Transaction, Box<dyn Error>> {
        let mut tx = self.make_unsigned_unchecked_run(script, sysfee, attrs)?;
        if let Some(tx_hook) = tx_hook {
            tx_hook(&mut tx)?;
        } else {
            self.opts.modifier(&mut tx)?;
        }
        self.sign(&mut tx)?;
        Ok(tx)
    }

    // MakeUnsignedCall creates an unsigned transaction with the given attributes
    // that calls the given method of the given contract with the given parameters.
    // Test-invocation is performed and is expected to end up in HALT state, the
    // transaction returned has correct SystemFee and NetworkFee values.
    // TransactionModifier is not applied to the result of this method, but default
    // attributes are used if attrs is nil.
    fn make_unsigned_call(
        &self,
        contract: Uint160,
        method: &str,
        attrs: Option<Vec<transaction::Attribute>>,
        params: Vec<impl Any>,
    ) -> Result<Transaction, Box<dyn Error>> {
        let (r, err) = self.call(contract, method, params)?;
        self.make_unsigned_wrapper(r, err, attrs)
    }

    // MakeUnsignedRun creates an unsigned transaction with the given attributes
    // that executes the given script. Test-invocation is performed and is expected
    // to end up in HALT state, the transaction returned has correct SystemFee and
    // NetworkFee values. TransactionModifier is not applied to the result of this
    // method, but default attributes are used if attrs is nil.
    fn make_unsigned_run(
        &self,
        script: &[u8],
        attrs: Option<Vec<transaction::Attribute>>,
    ) -> Result<Transaction, Box<dyn Error>> {
        let (r, err) = self.run(script)?;
        self.make_unsigned_wrapper(r, err, attrs)
    }

    fn make_unsigned_wrapper(
        &self,
        r: Invoke,
        err: Box<dyn Error>,
        attrs: Option<Vec<transaction::Attribute>>,
    ) -> Result<Transaction, Box<dyn Error>> {
        if let Some(err) = err {
            return Err(Box::new(fmt::Error::new(
                fmt::ErrorKind::Other,
                format!("failed to test-invoke: {}", err),
            )));
        }
        default_checker_modifier(&r, &mut Transaction::default())?;
        self.make_unsigned_unchecked_run(r.script, r.gas_consumed, attrs)
    }

    // MakeUnsignedUncheckedRun creates an unsigned transaction containing the given
    // script with the system fee value and attributes. It's expected to be used when
    // test invocation is already done and the script and system fee value are already
    // known to be good, so it doesn't do test invocation internally. But it fills
    // Signers with Actor's signers, calculates proper ValidUntilBlock and NetworkFee
    // values. The resulting transaction can be changed in its Nonce, SystemFee,
    // NetworkFee and ValidUntilBlock values and then be signed and sent or
    // exchanged via context.ParameterContext. TransactionModifier is not applied to
    // the result of this method, but default attributes are used if attrs is nil.
    fn make_unsigned_unchecked_run(
        &self,
        script: &[u8],
        sys_fee: i64,
        attrs: Option<Vec<transaction::Attribute>>,
    ) -> Result<Transaction, Box<dyn Error>> {
        if script.is_empty() {
            return Err(Box::new(fmt::Error::new(
                fmt::ErrorKind::Other,
                "empty script".to_string(),
            )));
        }
        if sys_fee < 0 {
            return Err(Box::new(fmt::Error::new(
                fmt::ErrorKind::Other,
                "negative system fee".to_string(),
            )));
        }

        let attrs = attrs.unwrap_or_else(|| self.opts.attributes.clone());
        let mut tx = Transaction::new(script.to_vec(), sys_fee);
        tx.signers = self.tx_signers.clone();
        tx.attributes = attrs;

        tx.valid_until_block = self.calculate_valid_until_block()?;
        tx.scripts = vec![transaction::Witness::default(); self.signers.len()];
        for (i, signer) in self.signers.iter().enumerate() {
            if !signer.account.contract.deployed {
                tx.scripts[i].verification_script = signer.account.contract.script.clone();
                continue;
            }
            if let Some(build) = &signer.account.contract.invocation_builder {
                let invoc = build(&tx)?;
                tx.scripts[i].invocation_script = invoc;
            }
        }
        tx.network_fee = self.client.calculate_network_fee(&tx)?;
        Ok(tx)
    }

    // CalculateValidUntilBlock returns correct ValidUntilBlock value for a new
    // transaction relative to the current blockchain height. It uses "height +
    // number of validators + 1" formula suggesting shorter transaction lifetime
    // than the usual "height + MaxValidUntilBlockIncrement" approach. Shorter
    // lifetime can be useful to control transaction acceptance wait time because
    // it can't be added into a block after ValidUntilBlock.
    fn calculate_valid_until_block(&self) -> Result<u32, Box<dyn Error>> {
        let block_count = self.client.get_block_count()?;
        let mut vc = self.version.protocol.validators_count as u32;
        let mut best_h = 0;
        for (h, n) in &self.version.protocol.validators_history {
            if *h >= best_h && *h <= block_count {
                vc = *n;
                best_h = *h;
            }
        }
        Ok(block_count + vc + 1)
    }
}
