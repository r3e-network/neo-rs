use neo_core2::core::transaction::{self, Signer};
use neo_core2::crypto::{hash, keys};
use neo_core2::rpcclient::notary::{FakeSimpleAccount, FakeContractAccount, FakeMultisigAccount};
use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_fake_accounts() -> Result<()> {
        let k = keys::PrivateKey::new()?;
        
        let fac = FakeSimpleAccount::new(k.public_key());
        assert!(!fac.can_sign());

        let sh = k.public_key().get_script_hash();
        let mut tx = transaction::Transaction::new(vec![1, 2, 3], 1);
        tx.signers.push(Signer { account: sh });
        fac.sign_tx(0, &mut tx)?;

        let fac = FakeContractAccount::new(sh);
        assert!(!fac.can_sign());
        fac.sign_tx(0, &mut tx)?;

        let result = FakeMultisigAccount::new(0, vec![k.public_key()]);
        assert!(result.is_err());

        let fac = FakeMultisigAccount::new(1, vec![k.public_key()])?;
        assert!(!fac.can_sign());
        tx.signers[0].account = hash::hash160(&fac.contract().script);
        fac.sign_tx(0, &mut tx)?;

        Ok(())
    }
}
