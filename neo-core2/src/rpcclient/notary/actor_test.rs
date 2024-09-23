use std::sync::Arc;
use std::sync::Mutex;
use uuid::Uuid;
use neo_core2::rpcclient::notary::actor::*;
use neo_core2::rpcclient::invoker::*;
use neo_core2::rpcclient::waiter::*;
use neo_core2::config::netmode;
use neo_core2::core::state;
use neo_core2::core::transaction;
use neo_core2::crypto::keys;
use neo_core2::encoding::address;
use neo_core2::neorpc::result;
use neo_core2::network::payload;
use neo_core2::smartcontract;
use neo_core2::smartcontract::trigger;
use neo_core2::util;
use neo_core2::vm::opcode;
use neo_core2::vm::stackitem;
use neo_core2::vm::vmstate;
use neo_core2::wallet;
use neo_core2::test::require;

struct RPCClient {
    err: Option<String>,
    inv_res: Option<result::Invoke>,
    net_fee: i64,
    b_count: u32,
    version: Option<result::Version>,
    hash: util::Uint256,
    nhash: util::Uint256,
    mirror: bool,
    applog: Option<result::ApplicationLog>,
}

impl RPCClient {
    fn invoke_contract_verify(&self, contract: util::Uint160, params: Vec<smartcontract::Parameter>, signers: Vec<transaction::Signer>, witnesses: Vec<transaction::Witness>) -> Result<result::Invoke, String> {
        self.inv_res.clone().ok_or_else(|| self.err.clone().unwrap_or_default())
    }

    fn invoke_function(&self, contract: util::Uint160, operation: String, params: Vec<smartcontract::Parameter>, signers: Vec<transaction::Signer>) -> Result<result::Invoke, String> {
        self.inv_res.clone().ok_or_else(|| self.err.clone().unwrap_or_default())
    }

    fn invoke_script(&self, script: Vec<u8>, signers: Vec<transaction::Signer>) -> Result<result::Invoke, String> {
        self.inv_res.clone().ok_or_else(|| self.err.clone().unwrap_or_default())
    }

    fn calculate_network_fee(&self, tx: &transaction::Transaction) -> Result<i64, String> {
        Ok(self.net_fee)
    }

    fn get_block_count(&self) -> Result<u32, String> {
        Ok(self.b_count)
    }

    fn get_version(&self) -> Result<result::Version, String> {
        self.version.clone().ok_or_else(|| self.err.clone().unwrap_or_default())
    }

    fn send_raw_transaction(&self, tx: &transaction::Transaction) -> Result<util::Uint256, String> {
        Ok(self.hash)
    }

    fn submit_p2p_notary_request(&self, req: &payload::P2PNotaryRequest) -> Result<util::Uint256, String> {
        if self.mirror {
            Ok(req.fallback_transaction.hash())
        } else {
            Ok(self.nhash)
        }
    }

    fn terminate_session(&self, session_id: Uuid) -> Result<bool, String> {
        Ok(false)
    }

    fn traverse_iterator(&self, session_id: Uuid, iterator_id: Uuid, max_items_count: i32) -> Result<Vec<stackitem::Item>, String> {
        Ok(vec![])
    }

    fn context(&self) -> context::Context {
        context::Context::background()
    }

    fn get_application_log(&self, hash: util::Uint256, trig: Option<trigger::Type>) -> Result<result::ApplicationLog, String> {
        self.applog.clone().ok_or_else(|| self.err.clone().unwrap_or_default())
    }
}

impl waiter::RPCPollingBased for RPCClient {}

#[test]
fn test_new_actor() {
    let rc = RPCClient {
        version: Some(result::Version {
            protocol: result::Protocol {
                network: netmode::UnitTestNet,
                milliseconds_per_block: 1000,
                validators_count: 7,
            },
        }),
        ..Default::default()
    };

    let err = NewActor(rc, None, None);
    require::error(err);

    let mut keyz = [None; 4];
    let mut accs = [None; 4];
    let mut faccs = [None; 4];
    let mut pkeys = [None; 4];

    for i in 0..accs.len() {
        keyz[i] = Some(keys::PrivateKey::new().unwrap());
        accs[i] = Some(wallet::Account::from_private_key(keyz[i].as_ref().unwrap()));
        pkeys[i] = Some(keyz[i].as_ref().unwrap().public_key());
        faccs[i] = Some(fake_simple_account(pkeys[i].as_ref().unwrap()));
    }

    let mut multi_accs = [None; 4];
    for i in 0..accs.len() {
        multi_accs[i] = Some(wallet::Account::default());
        *multi_accs[i].as_mut().unwrap() = accs[i].as_ref().unwrap().clone();
        require::no_error(multi_accs[i].as_mut().unwrap().convert_multisig(smartcontract::get_default_honest_node_count(pkeys.len()), &pkeys));
    }

    // nil Contract
    let mut bad_multi_acc0 = wallet::Account::default();
    *bad_multi_acc0 = multi_accs[0].as_ref().unwrap().clone();
    bad_multi_acc0.contract = None;
    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: multi_accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: bad_multi_acc0,
    }], accs[0].as_ref().unwrap());
    require::error(err);

    // Non-standard script.
    bad_multi_acc0.contract = Some(wallet::Contract::default());
    *bad_multi_acc0.contract.as_mut().unwrap() = multi_accs[0].as_ref().unwrap().contract.clone();
    bad_multi_acc0.contract.as_mut().unwrap().script.push(opcode::NOP as u8);
    bad_multi_acc0.address = address::uint160_to_string(&bad_multi_acc0.contract.as_ref().unwrap().script_hash());
    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: bad_multi_acc0.contract.as_ref().unwrap().script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: bad_multi_acc0,
    }], accs[0].as_ref().unwrap());
    require::error(err);

    // Too many keys
    let mut many_keys = [None; 256];
    let mut many_pkeys = [None; 256];
    for i in 0..many_keys.len() {
        many_keys[i] = Some(keys::PrivateKey::new().unwrap());
        many_pkeys[i] = Some(many_keys[i].as_ref().unwrap().public_key());
    }
    let mut big_multi_acc = wallet::Account::default();
    *big_multi_acc = wallet::Account::from_private_key(many_keys[0].as_ref().unwrap());
    require::no_error(big_multi_acc.convert_multisig(129, &many_pkeys));

    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: big_multi_acc.contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: big_multi_acc,
    }], wallet::Account::from_private_key(many_keys[0].as_ref().unwrap()));
    require::error(err);

    // No contract in the simple account.
    let mut bad_simple0 = wallet::Account::default();
    *bad_simple0 = accs[0].as_ref().unwrap().clone();
    bad_simple0.contract = None;
    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: multi_accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: multi_accs[0].as_ref().unwrap().clone(),
    }], bad_simple0);
    require::error(err);

    // Simple account that can't sign.
    bad_simple0 = fake_simple_account(pkeys[0].as_ref().unwrap());
    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: multi_accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: multi_accs[0].as_ref().unwrap().clone(),
    }], bad_simple0);
    require::error(err);

    // Multisig account instead of simple one.
    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: multi_accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: multi_accs[0].as_ref().unwrap().clone(),
    }], multi_accs[0].as_ref().unwrap().clone());
    require::error(err);

    // Main actor freaking out on hash mismatch.
    let err = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: multi_accs[0].as_ref().unwrap().clone(),
    }], accs[0].as_ref().unwrap());
    require::error(err);

    // FB actor freaking out on hash mismatch.
    let mut opts = NewDefaultActorOptions(NewReader(invoker::new(rc, None)), accs[0].as_ref().unwrap());
    opts.fb_signer.signer.account = multi_accs[0].as_ref().unwrap().contract.script_hash();
    let err = NewTunedActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: multi_accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: multi_accs[0].as_ref().unwrap().clone(),
    }], opts);
    require::error(err);

    // Good, one multisig.
    let multi0 = NewActor(rc, vec![actor::SignerAccount {
        signer: transaction::Signer {
            account: multi_accs[0].as_ref().unwrap().contract.script_hash(),
            scopes: transaction::CalledByEntry,
        },
        account: multi_accs[0].as_ref().unwrap().clone(),
    }], accs[0].as_ref().unwrap());
    require::no_error(multi0);

    let script = vec![opcode::RET as u8];
    rc.inv_res = Some(result::Invoke {
        state: "HALT".to_string(),
        gas_consumed: 3,
        script: script.clone(),
        stack: vec![stackitem::Item::make(42)],
    });
    let tx = multi0.make_run(&script).unwrap();
    require::no_error(tx);
    require::equal(1, tx.attributes.len());
    require::equal(transaction::NotaryAssistedT, tx.attributes[0].type_);
    require::equal(&transaction::NotaryAssisted { n_keys: 4 }, tx.attributes[0].value);

    // Good, 4 single sigs with one that can sign and one contract.
    let single4 = NewActor(rc, vec![
        actor::SignerAccount {
            signer: transaction::Signer {
                account: accs[0].as_ref().unwrap().contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: accs[0].as_ref().unwrap().clone(),
        },
        actor::SignerAccount {
            signer: transaction::Signer {
                account: faccs[1].as_ref().unwrap().contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: faccs[1].as_ref().unwrap().clone(),
        },
        actor::SignerAccount {
            signer: transaction::Signer {
                account: faccs[2].as_ref().unwrap().contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: faccs[2].as_ref().unwrap().clone(),
        },
        actor::SignerAccount {
            signer: transaction::Signer {
                account: accs[3].as_ref().unwrap().contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: faccs[3].as_ref().unwrap().clone(),
        },
        actor::SignerAccount {
            signer: transaction::Signer {
                account: util::Uint160::from([1, 2, 3]),
                scopes: transaction::CalledByEntry,
            },
            account: fake_contract_account(util::Uint160::from([1, 2, 3])),
        },
    ], accs[0].as_ref().unwrap());
    require::no_error(single4);

    let tx = single4.make_run(&script).unwrap();
    require::no_error(tx);
    require::equal(1, tx.attributes.len());
    require::equal(transaction::NotaryAssistedT, tx.attributes[0].type_);
    require::equal(&transaction::NotaryAssisted { n_keys: 4 }, tx.attributes[0].value); // One account can sign, three need to collect additional sigs.
}

#[test]
fn test_send_request_exactly() {
    let rc = RPCClient {
        version: Some(result::Version {
            protocol: result::Protocol {
                network: netmode::UnitTestNet,
                milliseconds_per_block: 1000,
                validators_count: 7,
            },
        }),
        ..Default::default()
    };

    let key0 = keys::PrivateKey::new().unwrap();
    let key1 = keys::PrivateKey::new().unwrap();

    let acc0 = wallet::Account::from_private_key(&key0);
    let facc1 = fake_simple_account(&key1.public_key());

    let act = NewActor(rc, vec![
        actor::SignerAccount {
            signer: transaction::Signer {
                account: acc0.contract.script_hash(),
                scopes: transaction::None,
            },
            account: acc0.clone(),
        },
        actor::SignerAccount {
            signer: transaction::Signer {
                account: facc1.contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: facc1.clone(),
        },
    ], &acc0).unwrap();

    let script = vec![opcode::RET as u8];
    let main_tx = transaction::Transaction::new(script.clone(), 1);
    let fb_tx = transaction::Transaction::new(script.clone(), 1);

    // Hashes mismatch
    let err = act.send_request_exactly(&main_tx, &fb_tx);
    require::error(err);

    // Error returned
    rc.err = Some("".to_string());
    let err = act.send_request_exactly(&main_tx, &fb_tx);
    require::error(err);

    // OK returned
    rc.err = None;
    rc.nhash = fb_tx.hash();
    let (m_hash, fb_hash, vub) = act.send_request_exactly(&main_tx, &fb_tx).unwrap();
    require::no_error(m_hash);
    require::equal(main_tx.hash(), m_hash);
    require::equal(fb_tx.hash(), fb_hash);
    require::equal(main_tx.valid_until_block, vub);
}

#[test]
fn test_send_request() {
    let rc = RPCClient {
        version: Some(result::Version {
            protocol: result::Protocol {
                network: netmode::UnitTestNet,
                milliseconds_per_block: 1000,
                validators_count: 7,
            },
        }),
        b_count: 42,
        ..Default::default()
    };

    let key0 = keys::PrivateKey::new().unwrap();
    let key1 = keys::PrivateKey::new().unwrap();

    let acc0 = wallet::Account::from_private_key(&key0);
    let facc0 = fake_simple_account(&key0.public_key());
    let facc1 = fake_simple_account(&key1.public_key());

    let act = NewActor(rc, vec![
        actor::SignerAccount {
            signer: transaction::Signer {
                account: acc0.contract.script_hash(),
                scopes: transaction::None,
            },
            account: acc0.clone(),
        },
        actor::SignerAccount {
            signer: transaction::Signer {
                account: facc1.contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: facc1.clone(),
        },
    ], &acc0).unwrap();

    let script = vec![opcode::RET as u8];
    rc.inv_res = Some(result::Invoke {
        state: "HALT".to_string(),
        gas_consumed: 3,
        script: script.clone(),
        stack: vec![stackitem::Item::make(42)],
    });

    let main_tx = act.make_run(&script).unwrap();

    // No attributes.
    let mut fb_tx = act.fb_actor.make_unsigned_run(&script, None).unwrap();
    fb_tx.attributes = None;
    let err = act.send_request(&main_tx, &fb_tx);
    require::error(err);

    // Bad NVB.
    fb_tx = act.fb_actor.make_unsigned_run(&script, None).unwrap();
    fb_tx.attributes[1].type_ = transaction::HighPriority;
    fb_tx.attributes[1].value = None;
    let err = act.send_request(&main_tx, &fb_tx);
    require::error(err);

    // Bad Conflicts.
    fb_tx = act.fb_actor.make_unsigned_run(&script, None).unwrap();
    fb_tx.attributes[2].type_ = transaction::HighPriority;
    fb_tx.attributes[2].value = None;
    let err = act.send_request(&main_tx, &fb_tx);
    require::error(err);

    // GetBlockCount error.
    fb_tx = act.fb_actor.make_unsigned_run(&script, None).unwrap();
    rc.err = Some("".to_string());
    let err = act.send_request(&main_tx, &fb_tx);
    require::error(err);

    // Can't sign suddenly.
    rc.err = None;
    let mut acc0_backup = acc0.clone();
    acc0 = facc0.clone();
    fb_tx = act.fb_actor.make_unsigned_run(&script, None).unwrap();
    let err = act.send_request(&main_tx, &fb_tx);
    require::error(err);

    // Good.
    acc0 = acc0_backup.clone();
import (
	"context"
	"errors"
	"testing"

	"github.com/google/uuid"
	"github.com/nspcc-dev/neo-go/pkg/config/netmode"
	"github.com/nspcc-dev/neo-go/pkg/core/state"
	"github.com/nspcc-dev/neo-go/pkg/core/transaction"
	"github.com/nspcc-dev/neo-go/pkg/crypto/keys"
	"github.com/nspcc-dev/neo-go/pkg/encoding/address"
	"github.com/nspcc-dev/neo-go/pkg/neorpc/result"
	"github.com/nspcc-dev/neo-go/pkg/network/payload"
	"github.com/nspcc-dev/neo-go/pkg/rpcclient/actor"
	"github.com/nspcc-dev/neo-go/pkg/rpcclient/invoker"
	"github.com/nspcc-dev/neo-go/pkg/rpcclient/waiter"
	"github.com/nspcc-dev/neo-go/pkg/smartcontract"
	"github.com/nspcc-dev/neo-go/pkg/smartcontract/trigger"
	"github.com/nspcc-dev/neo-go/pkg/util"
	"github.com/nspcc-dev/neo-go/pkg/vm/opcode"
	"github.com/nspcc-dev/neo-go/pkg/vm/stackitem"
	"github.com/nspcc-dev/neo-go/pkg/vm/vmstate"
	"github.com/nspcc-dev/neo-go/pkg/wallet"
	"github.com/stretchr/testify/require"
)

type RPCClient struct {
	err     error
	invRes  *result.Invoke
	netFee  int64
	bCount  uint32
	version *result.Version
	hash    util.Uint256
	nhash   util.Uint256
	mirror  bool
	applog  *result.ApplicationLog
}

func (r *RPCClient) InvokeContractVerify(contract util.Uint160, params []smartcontract.Parameter, signers []transaction.Signer, witnesses ...transaction.Witness) (*result.Invoke, error) {
	return r.invRes, r.err
}
func (r *RPCClient) InvokeFunction(contract util.Uint160, operation string, params []smartcontract.Parameter, signers []transaction.Signer) (*result.Invoke, error) {
	return r.invRes, r.err
}
func (r *RPCClient) InvokeScript(script []byte, signers []transaction.Signer) (*result.Invoke, error) {
	return r.invRes, r.err
}
func (r *RPCClient) CalculateNetworkFee(tx *transaction.Transaction) (int64, error) {
	return r.netFee, r.err
}
func (r *RPCClient) GetBlockCount() (uint32, error) {
	return r.bCount, r.err
}
func (r *RPCClient) GetVersion() (*result.Version, error) {
	verCopy := *r.version
	return &verCopy, r.err
}
func (r *RPCClient) SendRawTransaction(tx *transaction.Transaction) (util.Uint256, error) {
	return r.hash, r.err
}
func (r *RPCClient) SubmitP2PNotaryRequest(req *payload.P2PNotaryRequest) (util.Uint256, error) {
	if r.mirror {
		return req.FallbackTransaction.Hash(), nil
	}
	return r.nhash, r.err
}
func (r *RPCClient) TerminateSession(sessionID uuid.UUID) (bool, error) {
	return false, nil // Just a stub, unused by actor.
}
func (r *RPCClient) TraverseIterator(sessionID, iteratorID uuid.UUID, maxItemsCount int) ([]stackitem.Item, error) {
	return nil, nil // Just a stub, unused by actor.
}
func (r *RPCClient) Context() context.Context {
	return context.Background()
}
func (r *RPCClient) GetApplicationLog(hash util.Uint256, trig *trigger.Type) (*result.ApplicationLog, error) {
	return r.applog, nil
}

var _ = waiter.RPCPollingBased(&RPCClient{})

func TestNewActor(t *testing.T) {
	rc := &RPCClient{
		version: &result.Version{
			Protocol: result.Protocol{
				Network:              netmode.UnitTestNet,
				MillisecondsPerBlock: 1000,
				ValidatorsCount:      7,
			},
		},
	}

	_, err := NewActor(rc, nil, nil)
	require.Error(t, err)

	var (
		keyz  [4]*keys.PrivateKey
		accs  [4]*wallet.Account
		faccs [4]*wallet.Account
		pkeys [4]*keys.PublicKey
	)
	for i := range accs {
		keyz[i], err = keys.NewPrivateKey()
		require.NoError(t, err)
		accs[i] = wallet.NewAccountFromPrivateKey(keyz[i])
		pkeys[i] = keyz[i].PublicKey()
		faccs[i] = FakeSimpleAccount(pkeys[i])
	}
	var multiAccs [4]*wallet.Account
	for i := range accs {
		multiAccs[i] = &wallet.Account{}
		*multiAccs[i] = *accs[i]
		require.NoError(t, multiAccs[i].ConvertMultisig(smartcontract.GetDefaultHonestNodeCount(len(pkeys)), pkeys[:]))
	}

	// nil Contract
	badMultiAcc0 := &wallet.Account{}
	*badMultiAcc0 = *multiAccs[0]
	badMultiAcc0.Contract = nil
	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: multiAccs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: badMultiAcc0,
	}}, accs[0])
	require.Error(t, err)

	// Non-standard script.
	badMultiAcc0.Contract = &wallet.Contract{}
	*badMultiAcc0.Contract = *multiAccs[0].Contract
	badMultiAcc0.Contract.Script = append(badMultiAcc0.Contract.Script, byte(opcode.NOP))
	badMultiAcc0.Address = address.Uint160ToString(badMultiAcc0.Contract.ScriptHash())
	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: badMultiAcc0.Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: badMultiAcc0,
	}}, accs[0])
	require.Error(t, err)

	// Too many keys
	var (
		manyKeys  [256]*keys.PrivateKey
		manyPkeys [256]*keys.PublicKey
	)
	for i := range manyKeys {
		manyKeys[i], err = keys.NewPrivateKey()
		require.NoError(t, err)
		manyPkeys[i] = manyKeys[i].PublicKey()
	}
	bigMultiAcc := &wallet.Account{}
	*bigMultiAcc = *wallet.NewAccountFromPrivateKey(manyKeys[0])
	require.NoError(t, bigMultiAcc.ConvertMultisig(129, manyPkeys[:]))

	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: bigMultiAcc.Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: bigMultiAcc,
	}}, wallet.NewAccountFromPrivateKey(manyKeys[0]))
	require.Error(t, err)

	// No contract in the simple account.
	badSimple0 := &wallet.Account{}
	*badSimple0 = *accs[0]
	badSimple0.Contract = nil
	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: multiAccs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: multiAccs[0],
	}}, badSimple0)
	require.Error(t, err)

	// Simple account that can't sign.
	badSimple0 = FakeSimpleAccount(pkeys[0])
	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: multiAccs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: multiAccs[0],
	}}, badSimple0)
	require.Error(t, err)

	// Multisig account instead of simple one.
	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: multiAccs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: multiAccs[0],
	}}, multiAccs[0])
	require.Error(t, err)

	// Main actor freaking out on hash mismatch.
	_, err = NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: accs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: multiAccs[0],
	}}, accs[0])
	require.Error(t, err)

	// FB actor freaking out on hash mismatch.
	opts := NewDefaultActorOptions(NewReader(invoker.New(rc, nil)), accs[0])
	opts.FbSigner.Signer.Account = multiAccs[0].Contract.ScriptHash()
	_, err = NewTunedActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: multiAccs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: multiAccs[0],
	}}, opts)
	require.Error(t, err)

	// Good, one multisig.
	multi0, err := NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: multiAccs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: multiAccs[0],
	}}, accs[0])
	require.NoError(t, err)

	script := []byte{byte(opcode.RET)}
	rc.invRes = &result.Invoke{
		State:       "HALT",
		GasConsumed: 3,
		Script:      script,
		Stack:       []stackitem.Item{stackitem.Make(42)},
	}
	tx, err := multi0.MakeRun(script)
	require.NoError(t, err)
	require.Equal(t, 1, len(tx.Attributes))
	require.Equal(t, transaction.NotaryAssistedT, tx.Attributes[0].Type)
	require.Equal(t, &transaction.NotaryAssisted{NKeys: 4}, tx.Attributes[0].Value)

	// Good, 4 single sigs with one that can sign and one contract.
	single4, err := NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: accs[0].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: accs[0],
	}, {
		Signer: transaction.Signer{
			Account: faccs[1].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: faccs[1],
	}, {
		Signer: transaction.Signer{
			Account: faccs[2].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: faccs[2],
	}, {
		Signer: transaction.Signer{
			Account: accs[3].Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: faccs[3],
	}, {
		Signer: transaction.Signer{
			Account: util.Uint160{1, 2, 3},
			Scopes:  transaction.CalledByEntry,
		},
		Account: FakeContractAccount(util.Uint160{1, 2, 3}),
	}}, accs[0])
	require.NoError(t, err)

	tx, err = single4.MakeRun(script)
	require.NoError(t, err)
	require.Equal(t, 1, len(tx.Attributes))
	require.Equal(t, transaction.NotaryAssistedT, tx.Attributes[0].Type)
	require.Equal(t, &transaction.NotaryAssisted{NKeys: 4}, tx.Attributes[0].Value) // One account can sign, three need to collect additional sigs.
}

func TestSendRequestExactly(t *testing.T) {
	rc := &RPCClient{
		version: &result.Version{
			Protocol: result.Protocol{
				Network:              netmode.UnitTestNet,
				MillisecondsPerBlock: 1000,
				ValidatorsCount:      7,
			},
		},
	}

	key0, err := keys.NewPrivateKey()
	require.NoError(t, err)
	key1, err := keys.NewPrivateKey()
	require.NoError(t, err)

	acc0 := wallet.NewAccountFromPrivateKey(key0)
	facc1 := FakeSimpleAccount(key1.PublicKey())

	act, err := NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: acc0.Contract.ScriptHash(),
			Scopes:  transaction.None,
		},
		Account: acc0,
	}, {
		Signer: transaction.Signer{
			Account: facc1.Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: facc1,
	}}, acc0)
	require.NoError(t, err)

	script := []byte{byte(opcode.RET)}
	mainTx := transaction.New(script, 1)
	fbTx := transaction.New(script, 1)

	// Hashes mismatch
	_, _, _, err = act.SendRequestExactly(mainTx, fbTx)
	require.Error(t, err)

	// Error returned
	rc.err = errors.New("")
	_, _, _, err = act.SendRequestExactly(mainTx, fbTx)
	require.Error(t, err)

	// OK returned
	rc.err = nil
	rc.nhash = fbTx.Hash()
	mHash, fbHash, vub, err := act.SendRequestExactly(mainTx, fbTx)
	require.NoError(t, err)
	require.Equal(t, mainTx.Hash(), mHash)
	require.Equal(t, fbTx.Hash(), fbHash)
	require.Equal(t, mainTx.ValidUntilBlock, vub)
}

func TestSendRequest(t *testing.T) {
	rc := &RPCClient{
		version: &result.Version{
			Protocol: result.Protocol{
				Network:              netmode.UnitTestNet,
				MillisecondsPerBlock: 1000,
				ValidatorsCount:      7,
			},
		},
		bCount: 42,
	}

	key0, err := keys.NewPrivateKey()
	require.NoError(t, err)
	key1, err := keys.NewPrivateKey()
	require.NoError(t, err)

	acc0 := wallet.NewAccountFromPrivateKey(key0)
	facc0 := FakeSimpleAccount(key0.PublicKey())
	facc1 := FakeSimpleAccount(key1.PublicKey())

	act, err := NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: acc0.Contract.ScriptHash(),
			Scopes:  transaction.None,
		},
		Account: acc0,
	}, {
		Signer: transaction.Signer{
			Account: facc1.Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: facc1,
	}}, acc0)
	require.NoError(t, err)

	script := []byte{byte(opcode.RET)}
	rc.invRes = &result.Invoke{
		State:       "HALT",
		GasConsumed: 3,
		Script:      script,
		Stack:       []stackitem.Item{stackitem.Make(42)},
	}

	mainTx, err := act.MakeRun(script)
	require.NoError(t, err)

	// No attributes.
	fbTx, err := act.FbActor.MakeUnsignedRun(script, nil)
	require.NoError(t, err)
	fbTx.Attributes = nil
	_, _, _, err = act.SendRequest(mainTx, fbTx)
	require.Error(t, err)

	// Bad NVB.
	fbTx, err = act.FbActor.MakeUnsignedRun(script, nil)
	require.NoError(t, err)
	fbTx.Attributes[1].Type = transaction.HighPriority
	fbTx.Attributes[1].Value = nil
	_, _, _, err = act.SendRequest(mainTx, fbTx)
	require.Error(t, err)

	// Bad Conflicts.
	fbTx, err = act.FbActor.MakeUnsignedRun(script, nil)
	require.NoError(t, err)
	fbTx.Attributes[2].Type = transaction.HighPriority
	fbTx.Attributes[2].Value = nil
	_, _, _, err = act.SendRequest(mainTx, fbTx)
	require.Error(t, err)

	// GetBlockCount error.
	fbTx, err = act.FbActor.MakeUnsignedRun(script, nil)
	require.NoError(t, err)
	rc.err = errors.New("")
	_, _, _, err = act.SendRequest(mainTx, fbTx)
	require.Error(t, err)

	// Can't sign suddenly.
	rc.err = nil
	acc0Backup := &wallet.Account{}
	*acc0Backup = *acc0
	*acc0 = *facc0
	fbTx, err = act.FbActor.MakeUnsignedRun(script, nil)
	require.NoError(t, err)
	_, _, _, err = act.SendRequest(mainTx, fbTx)
	require.Error(t, err)

	// Good.
	*acc0 = *acc0Backup
	fbTx, err = act.FbActor.MakeUnsignedRun(script, nil)
	require.NoError(t, err)
	_, _, _, err = act.SendRequest(mainTx, fbTx)
	require.Error(t, err)
}

func TestNotarize(t *testing.T) {
	rc := &RPCClient{
		version: &result.Version{
			Protocol: result.Protocol{
				Network:              netmode.UnitTestNet,
				MillisecondsPerBlock: 1000,
				ValidatorsCount:      7,
			},
		},
		bCount: 42,
	}

	key0, err := keys.NewPrivateKey()
	require.NoError(t, err)
	key1, err := keys.NewPrivateKey()
	require.NoError(t, err)

	acc0 := wallet.NewAccountFromPrivateKey(key0)
	facc1 := FakeSimpleAccount(key1.PublicKey())

	act, err := NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: acc0.Contract.ScriptHash(),
			Scopes:  transaction.None,
		},
		Account: acc0,
	}, {
		Signer: transaction.Signer{
			Account: facc1.Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: facc1,
	}}, acc0)
	require.NoError(t, err)

	script := []byte{byte(opcode.RET)}

	// Immediate error from MakeRun.
	rc.invRes = &result.Invoke{
		State:       "FAULT",
		GasConsumed: 3,
		Script:      script,
		Stack:       []stackitem.Item{stackitem.Make(42)},
	}
	_, _, _, err = act.Notarize(act.MakeRun(script))
	require.Error(t, err)

	// Explicitly good transaction. but failure to create a fallback.
	rc.invRes.State = "HALT"
	tx, err := act.MakeRun(script)
	require.NoError(t, err)

	rc.invRes.State = "FAULT"
	_, _, _, err = act.Notarize(tx, nil)
	require.Error(t, err)

	// FB hash mismatch from SendRequestExactly.
	rc.invRes.State = "HALT"
	_, _, _, err = act.Notarize(act.MakeRun(script))
	require.Error(t, err)

	// Good.
	rc.mirror = true
	mHash, fbHash, vub, err := act.Notarize(act.MakeRun(script))
	require.NoError(t, err)
	require.NotEqual(t, util.Uint256{}, mHash)
	require.NotEqual(t, util.Uint256{}, fbHash)
	require.Equal(t, uint32(92), vub)
}

func TestDefaultActorOptions(t *testing.T) {
	rc := &RPCClient{
		version: &result.Version{
			Protocol: result.Protocol{
				Network:              netmode.UnitTestNet,
				MillisecondsPerBlock: 1000,
				ValidatorsCount:      7,
			},
		},
	}
	acc, err := wallet.NewAccount()
	require.NoError(t, err)
	opts := NewDefaultActorOptions(NewReader(invoker.New(rc, nil)), acc)
	rc.invRes = &result.Invoke{
		State:       "HALT",
		GasConsumed: 3,
		Script:      opts.FbScript,
		Stack:       []stackitem.Item{stackitem.Make(42)},
	}
	tx := transaction.New(opts.FbScript, 1)
	require.Error(t, opts.MainCheckerModifier(&result.Invoke{State: "FAULT"}, tx))
	rc.invRes.State = "FAULT"
	require.Error(t, opts.MainCheckerModifier(&result.Invoke{State: "HALT"}, tx))
	rc.invRes.State = "HALT"
	require.NoError(t, opts.MainCheckerModifier(&result.Invoke{State: "HALT"}, tx))
	require.Equal(t, uint32(42), tx.ValidUntilBlock)
}

func TestWait(t *testing.T) {
	rc := &RPCClient{version: &result.Version{Protocol: result.Protocol{MillisecondsPerBlock: 1}}}

	key0, err := keys.NewPrivateKey()
	require.NoError(t, err)
	key1, err := keys.NewPrivateKey()
	require.NoError(t, err)

	acc0 := wallet.NewAccountFromPrivateKey(key0)
	facc1 := FakeSimpleAccount(key1.PublicKey())

	act, err := NewActor(rc, []actor.SignerAccount{{
		Signer: transaction.Signer{
			Account: acc0.Contract.ScriptHash(),
			Scopes:  transaction.None,
		},
		Account: acc0,
	}, {
		Signer: transaction.Signer{
			Account: facc1.Contract.ScriptHash(),
			Scopes:  transaction.CalledByEntry,
		},
		Account: facc1,
	}}, acc0)
	require.NoError(t, err)

	someErr := errors.New("someErr")
	_, err = act.Wait(util.Uint256{}, util.Uint256{}, 0, someErr)
	require.ErrorIs(t, err, someErr)

	_, err = act.WaitSuccess(util.Uint256{}, util.Uint256{}, 0, someErr)
	require.ErrorIs(t, err, someErr)

	cont := util.Uint256{1, 2, 3}
	ex := state.Execution{
		Trigger:     trigger.Application,
		VMState:     vmstate.Halt,
		GasConsumed: 123,
		Stack:       []stackitem.Item{stackitem.Null{}},
	}
	applog := &result.ApplicationLog{
		Container:     cont,
		IsTransaction: true,
		Executions:    []state.Execution{ex},
	}
	rc.applog = applog
	res, err := act.Wait(util.Uint256{}, util.Uint256{}, 0, nil)
	require.NoError(t, err)
	require.Equal(t, &state.AppExecResult{
		Container: cont,
		Execution: ex,
	}, res)

	// Not successful since result has a different hash.
	_, err = act.WaitSuccess(util.Uint256{}, util.Uint256{}, 0, nil)
	require.ErrorIs(t, err, ErrFallbackAccepted)
	_, err = act.WaitSuccess(util.Uint256{}, util.Uint256{1, 2, 3}, 0, nil)
	require.ErrorIs(t, err, ErrFallbackAccepted)

	rc.applog.Executions[0].VMState = vmstate.Fault
	_, err = act.WaitSuccess(util.Uint256{1, 2, 3}, util.Uint256{}, 0, nil)
	require.ErrorIs(t, err, actor.ErrExecFailed)

	rc.applog.Executions[0].VMState = vmstate.Halt
	res, err = act.WaitSuccess(util.Uint256{1, 2, 3}, util.Uint256{}, 0, nil)
	require.NoError(t, err)
	require.Equal(t, &state.AppExecResult{
		Container: cont,
		Execution: ex,
	}, res)
}
