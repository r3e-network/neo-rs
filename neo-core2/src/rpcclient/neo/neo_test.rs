use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;
use bigdecimal::BigDecimal;
use neo_core::state;
use neo_core::transaction;
use neo_core::crypto::keys;
use neo_core::neorpc::result;
use neo_core::util;
use neo_core::vm::stackitem;
use neo_core::require;

struct TestAct {
    err: Option<Box<dyn Error>>,
    ser: Option<Box<dyn Error>>,
    res: Option<result::Invoke>,
    rre: Option<result::Invoke>,
    rer: Option<Box<dyn Error>>,
    tx: Option<transaction::Transaction>,
    txh: util::Uint256,
    vub: u32,
    inv: Option<result::Invoke>,
}

impl TestAct {
    fn call(&self, contract: util::Uint160, operation: &str, params: Vec<stackitem::Item>) -> Result<result::Invoke, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_run(&self, script: Vec<u8>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_unsigned_run(&self, script: Vec<u8>, attrs: Vec<transaction::Attribute>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn send_run(&self, script: Vec<u8>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        Ok((self.txh, self.vub))
    }

    fn make_call(&self, contract: util::Uint160, method: &str, params: Vec<stackitem::Item>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: Vec<transaction::Attribute>, params: Vec<stackitem::Item>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn send_call(&self, contract: util::Uint160, method: &str, params: Vec<stackitem::Item>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        Ok((self.txh, self.vub))
    }

    fn run(&self, script: Vec<u8>) -> Result<result::Invoke, Box<dyn Error>> {
        match &self.rre {
            Some(rre) => Ok(rre.clone()),
            None => Err(self.rer.clone().unwrap()),
        }
    }

    fn make_unsigned_unchecked_run(&self, script: Vec<u8>, sys_fee: i64, attrs: Vec<transaction::Attribute>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn sign(&self, tx: &transaction::Transaction) -> Result<(), Box<dyn Error>> {
        match &self.ser {
            Some(ser) => Err(ser.clone()),
            None => Ok(()),
        }
    }

    fn sign_and_send(&self, tx: &transaction::Transaction) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        Ok((self.txh, self.vub))
    }

    fn call_and_expand_iterator(&self, contract: util::Uint160, method: &str, max_items: i32, params: Vec<stackitem::Item>) -> Result<result::Invoke, Box<dyn Error>> {
        match &self.inv {
            Some(inv) => Ok(inv.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn terminate_session(&self, session_id: Uuid) -> Result<(), Box<dyn Error>> {
        match &self.err {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    fn traverse_iterator(&self, session_id: Uuid, iterator: &result::Iterator, num: i32) -> Result<Vec<stackitem::Item>, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.stack.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }
}

#[test]
fn test_get_account_state() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = NewReader(Arc::new(ta));

    let result = neo.get_account_state(util::Uint160::default());
    assert!(result.is_err());

    ta.err = None;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(42)],
    });
    let result = neo.get_account_state(util::Uint160::default());
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Null],
    });
    let st = neo.get_account_state(util::Uint160::default()).unwrap();
    assert!(st.is_none());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![
            stackitem::Item::from(100500),
            stackitem::Item::from(42),
            stackitem::Item::Null,
        ])],
    });
    let st = neo.get_account_state(util::Uint160::default()).unwrap();
    assert_eq!(
        st,
        Some(state::NEOBalance {
            nep17_balance: state::NEP17Balance {
                balance: BigDecimal::from(100500),
            },
            balance_height: 42,
        })
    );
}

#[test]
fn test_get_all_candidates() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = NewReader(Arc::new(ta));

    let result = neo.get_all_candidates();
    assert!(result.is_err());

    ta.err = None;
    let iid = Uuid::new_v4();
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Interop(result::Iterator {
            id: Some(iid),
            ..Default::default()
        })],
    });
    let result = neo.get_all_candidates();
    assert!(result.is_err());

    // Session-based iterator.
    let sid = Uuid::new_v4();
    ta.res = Some(result::Invoke {
        session: Some(sid),
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Interop(result::Iterator {
            id: Some(iid),
            ..Default::default()
        })],
    });
    let iter = neo.get_all_candidates().unwrap();

    let k = keys::PrivateKey::new().unwrap();
    ta.res = Some(result::Invoke {
        stack: vec![stackitem::Item::Array(vec![
            stackitem::Item::from(k.public_key().to_bytes()),
            stackitem::Item::from(100500),
        ])],
    });
    let vals = iter.next(10).unwrap();
    assert_eq!(vals.len(), 1);
    assert_eq!(
        vals[0],
        result::Validator {
            public_key: k.public_key().clone(),
            votes: 100500,
        }
    );

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = iter.next(1);
    assert!(result.is_err());

    let result = iter.terminate();
    assert!(result.is_err());

    // Value-based iterator.
    ta.err = None;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Interop(result::Iterator {
            values: vec![
                stackitem::Item::from(k.public_key().to_bytes()),
                stackitem::Item::from(100500),
            ],
            ..Default::default()
        })],
    });
    let iter = neo.get_all_candidates().unwrap();

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = iter.terminate();
    assert!(result.is_ok());
}

#[test]
fn test_get_candidates() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = NewReader(Arc::new(ta));

    let result = neo.get_candidates();
    assert!(result.is_err());

    ta.err = None;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![])],
    });
    let cands = neo.get_candidates().unwrap();
    assert_eq!(cands.len(), 0);

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(42)],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![stackitem::Item::from(42)])],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![stackitem::Item::Array(vec![])])],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![stackitem::Item::Array(vec![
            stackitem::Item::Null,
            stackitem::Item::Null,
        ])])],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![stackitem::Item::Array(vec![
            stackitem::Item::from("some"),
            stackitem::Item::Null,
        ])])],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());

    let k = keys::PrivateKey::new().unwrap();
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![stackitem::Item::Array(vec![
            stackitem::Item::from(k.public_key().to_bytes()),
            stackitem::Item::Null,
        ])])],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![stackitem::Item::Array(vec![
            stackitem::Item::from(k.public_key().to_bytes()),
            stackitem::Item::from("canbeabigint"),
        ])])],
    });
    let result = neo.get_candidates();
    assert!(result.is_err());
}

#[test]
fn test_get_keys() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = NewReader(Arc::new(ta));

    let k = keys::PrivateKey::new().unwrap();

    for m in vec![neo.get_committee, neo.get_next_block_validators] {
        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        let result = m();
        assert!(result.is_err());

        ta.err = None;
        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::Array(vec![stackitem::Item::from(k.public_key().to_bytes())])],
        });
        let ks = m().unwrap();
        assert!(ks.is_some());
        assert_eq!(ks.unwrap().len(), 1);
        assert_eq!(ks.unwrap()[0], k.public_key());
    }
}

#[test]
fn test_get_ints() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = NewReader(Arc::new(ta));

    let meth = vec![neo.get_gas_per_block, neo.get_register_price];

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    for m in &meth {
        let result = m();
        assert!(result.is_err());
    }

    ta.err = None;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(42)],
    });
    for m in &meth {
        let val = m().unwrap();
        assert_eq!(val, 42);
    }
}

#[test]
fn test_unclaimed_gas() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = NewReader(Arc::new(ta));

    let result = neo.unclaimed_gas(util::Uint160::default(), 100500);
    assert!(result.is_err());

    ta.err = None;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Array(vec![])],
    });
    let result = neo.unclaimed_gas(util::Uint160::default(), 100500);
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(42)],
    });
    let val = neo.unclaimed_gas(util::Uint160::default(), 100500).unwrap();
    assert_eq!(val, BigDecimal::from(42));
}

#[test]
fn test_int_setters() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = New(Arc::new(ta));

    let meth = vec![neo.set_gas_per_block, neo.set_register_price];

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    for m in &meth {
        let result = m(42);
        assert!(result.is_err());
    }

    ta.err = None;
    ta.txh = util::Uint256::from([1, 2, 3]);
    ta.vub = 42;
    for m in &meth {
        let (h, vub) = m(100).unwrap();
        assert_eq!(h, ta.txh);
        assert_eq!(vub, ta.vub);
    }
}

#[test]
fn test_int_transactions() {
    let ta = TestAct {
        err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
        ser: None,
        res: None,
        rre: None,
        rer: None,
        tx: None,
        txh: util::Uint256::default(),
        vub: 0,
        inv: None,
    };
    let neo = New(Arc::new(ta));

    for fun in vec![
        neo.set_gas_per_block_transaction,
        neo.set_gas_per_block_unsigned,
        neo.set_register_price_transaction,
        neo.set_register_price_unsigned,
    ] {
        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        let result = fun(1);
        assert!(result.is_err());

        ta.err = None;
        ta.tx = Some(transaction::Transaction {
            nonce: 100500,
            valid_until_block: 42,
            ..Default::default()
        });
import (
	"errors"
	"math/big"
	"testing"

	"github.com/google/uuid"
	"github.com/nspcc-dev/neo-go/pkg/core/state"
	"github.com/nspcc-dev/neo-go/pkg/core/transaction"
	"github.com/nspcc-dev/neo-go/pkg/crypto/keys"
	"github.com/nspcc-dev/neo-go/pkg/neorpc/result"
	"github.com/nspcc-dev/neo-go/pkg/util"
	"github.com/nspcc-dev/neo-go/pkg/vm/stackitem"
	"github.com/stretchr/testify/require"
)

type testAct struct {
	err error
	ser error
	res *result.Invoke
	rre *result.Invoke
	rer error
	tx  *transaction.Transaction
	txh util.Uint256
	vub uint32
	inv *result.Invoke
}

func (t *testAct) Call(contract util.Uint160, operation string, params ...any) (*result.Invoke, error) {
	return t.res, t.err
}
func (t *testAct) MakeRun(script []byte) (*transaction.Transaction, error) {
	return t.tx, t.err
}
func (t *testAct) MakeUnsignedRun(script []byte, attrs []transaction.Attribute) (*transaction.Transaction, error) {
	return t.tx, t.err
}
func (t *testAct) SendRun(script []byte) (util.Uint256, uint32, error) {
	return t.txh, t.vub, t.err
}
func (t *testAct) MakeCall(contract util.Uint160, method string, params ...any) (*transaction.Transaction, error) {
	return t.tx, t.err
}
func (t *testAct) MakeUnsignedCall(contract util.Uint160, method string, attrs []transaction.Attribute, params ...any) (*transaction.Transaction, error) {
	return t.tx, t.err
}
func (t *testAct) SendCall(contract util.Uint160, method string, params ...any) (util.Uint256, uint32, error) {
	return t.txh, t.vub, t.err
}
func (t *testAct) Run(script []byte) (*result.Invoke, error) {
	return t.rre, t.rer
}
func (t *testAct) MakeUnsignedUncheckedRun(script []byte, sysFee int64, attrs []transaction.Attribute) (*transaction.Transaction, error) {
	return t.tx, t.err
}
func (t *testAct) Sign(tx *transaction.Transaction) error {
	return t.ser
}
func (t *testAct) SignAndSend(tx *transaction.Transaction) (util.Uint256, uint32, error) {
	return t.txh, t.vub, t.err
}
func (t *testAct) CallAndExpandIterator(contract util.Uint160, method string, maxItems int, params ...any) (*result.Invoke, error) {
	return t.inv, t.err
}
func (t *testAct) TerminateSession(sessionID uuid.UUID) error {
	return t.err
}
func (t *testAct) TraverseIterator(sessionID uuid.UUID, iterator *result.Iterator, num int) ([]stackitem.Item, error) {
	return t.res.Stack, t.err
}

func TestGetAccountState(t *testing.T) {
	ta := &testAct{}
	neo := NewReader(ta)

	ta.err = errors.New("")
	_, err := neo.GetAccountState(util.Uint160{})
	require.Error(t, err)

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(42),
		},
	}
	_, err = neo.GetAccountState(util.Uint160{})
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Null{},
		},
	}
	st, err := neo.GetAccountState(util.Uint160{})
	require.NoError(t, err)
	require.Nil(t, st)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make(100500),
				stackitem.Make(42),
				stackitem.Null{},
			}),
		},
	}
	st, err = neo.GetAccountState(util.Uint160{})
	require.NoError(t, err)
	require.Equal(t, &state.NEOBalance{
		NEP17Balance: state.NEP17Balance{
			Balance: *big.NewInt(100500),
		},
		BalanceHeight: 42,
	}, st)
}

func TestGetAllCandidates(t *testing.T) {
	ta := &testAct{}
	neo := NewReader(ta)

	ta.err = errors.New("")
	_, err := neo.GetAllCandidates()
	require.Error(t, err)

	ta.err = nil
	iid := uuid.New()
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.NewInterop(result.Iterator{
				ID: &iid,
			}),
		},
	}
	_, err = neo.GetAllCandidates()
	require.Error(t, err)

	// Session-based iterator.
	sid := uuid.New()
	ta.res = &result.Invoke{
		Session: sid,
		State:   "HALT",
		Stack: []stackitem.Item{
			stackitem.NewInterop(result.Iterator{
				ID: &iid,
			}),
		},
	}
	iter, err := neo.GetAllCandidates()
	require.NoError(t, err)

	k, err := keys.NewPrivateKey()
	require.NoError(t, err)
	ta.res = &result.Invoke{
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make(k.PublicKey().Bytes()),
				stackitem.Make(100500),
			}),
		},
	}
	vals, err := iter.Next(10)
	require.NoError(t, err)
	require.Equal(t, 1, len(vals))
	require.Equal(t, result.Validator{
		PublicKey: *k.PublicKey(),
		Votes:     100500,
	}, vals[0])

	ta.err = errors.New("")
	_, err = iter.Next(1)
	require.Error(t, err)

	err = iter.Terminate()
	require.Error(t, err)

	// Value-based iterator.
	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.NewInterop(result.Iterator{
				Values: []stackitem.Item{
					stackitem.Make(k.PublicKey().Bytes()),
					stackitem.Make(100500),
				},
			}),
		},
	}
	iter, err = neo.GetAllCandidates()
	require.NoError(t, err)

	ta.err = errors.New("")
	err = iter.Terminate()
	require.NoError(t, err)
}

func TestGetCandidates(t *testing.T) {
	ta := &testAct{}
	neo := NewReader(ta)

	ta.err = errors.New("")
	_, err := neo.GetCandidates()
	require.Error(t, err)

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{}),
		},
	}
	cands, err := neo.GetCandidates()
	require.NoError(t, err)
	require.Equal(t, 0, len(cands))

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{stackitem.Make(42)},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make(42),
			}),
		},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{}),
			}),
		},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{
					stackitem.Null{},
					stackitem.Null{},
				}),
			}),
		},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{
					stackitem.Make("some"),
					stackitem.Null{},
				}),
			}),
		},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)

	k, err := keys.NewPrivateKey()
	require.NoError(t, err)
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{
					stackitem.Make(k.PublicKey().Bytes()),
					stackitem.Null{},
				}),
			}),
		},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{
					stackitem.Make(k.PublicKey().Bytes()),
					stackitem.Make("canbeabigint"),
				}),
			}),
		},
	}
	_, err = neo.GetCandidates()
	require.Error(t, err)
}

func TestGetKeys(t *testing.T) {
	ta := &testAct{}
	neo := NewReader(ta)

	k, err := keys.NewPrivateKey()
	require.NoError(t, err)

	for _, m := range []func() (keys.PublicKeys, error){neo.GetCommittee, neo.GetNextBlockValidators} {
		ta.err = errors.New("")
		_, err := m()
		require.Error(t, err)

		ta.err = nil
		ta.res = &result.Invoke{
			State: "HALT",
			Stack: []stackitem.Item{
				stackitem.Make([]stackitem.Item{stackitem.Make(k.PublicKey().Bytes())}),
			},
		}
		ks, err := m()
		require.NoError(t, err)
		require.NotNil(t, ks)
		require.Equal(t, 1, len(ks))
		require.Equal(t, k.PublicKey(), ks[0])
	}
}

func TestGetInts(t *testing.T) {
	ta := &testAct{}
	neo := NewReader(ta)

	meth := []func() (int64, error){
		neo.GetGasPerBlock,
		neo.GetRegisterPrice,
	}

	ta.err = errors.New("")
	for _, m := range meth {
		_, err := m()
		require.Error(t, err)
	}

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(42),
		},
	}
	for _, m := range meth {
		val, err := m()
		require.NoError(t, err)
		require.Equal(t, int64(42), val)
	}
}

func TestUnclaimedGas(t *testing.T) {
	ta := &testAct{}
	neo := NewReader(ta)

	ta.err = errors.New("")
	_, err := neo.UnclaimedGas(util.Uint160{}, 100500)
	require.Error(t, err)

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{}),
		},
	}
	_, err = neo.UnclaimedGas(util.Uint160{}, 100500)
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(42),
		},
	}
	val, err := neo.UnclaimedGas(util.Uint160{}, 100500)
	require.NoError(t, err)
	require.Equal(t, big.NewInt(42), val)
}

func TestIntSetters(t *testing.T) {
	ta := new(testAct)
	neo := New(ta)

	meth := []func(int64) (util.Uint256, uint32, error){
		neo.SetGasPerBlock,
		neo.SetRegisterPrice,
	}

	ta.err = errors.New("")
	for _, m := range meth {
		_, _, err := m(42)
		require.Error(t, err)
	}

	ta.err = nil
	ta.txh = util.Uint256{1, 2, 3}
	ta.vub = 42
	for _, m := range meth {
		h, vub, err := m(100)
		require.NoError(t, err)
		require.Equal(t, ta.txh, h)
		require.Equal(t, ta.vub, vub)
	}
}

func TestIntTransactions(t *testing.T) {
	ta := new(testAct)
	neo := New(ta)

	for _, fun := range []func(int64) (*transaction.Transaction, error){
		neo.SetGasPerBlockTransaction,
		neo.SetGasPerBlockUnsigned,
		neo.SetRegisterPriceTransaction,
		neo.SetRegisterPriceUnsigned,
	} {
		ta.err = errors.New("")
		_, err := fun(1)
		require.Error(t, err)

		ta.err = nil
		ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
		tx, err := fun(1)
		require.NoError(t, err)
		require.Equal(t, ta.tx, tx)
	}
}

func TestVote(t *testing.T) {
	ta := new(testAct)
	neo := New(ta)

	k, err := keys.NewPrivateKey()
	require.NoError(t, err)

	ta.err = errors.New("")
	_, _, err = neo.Vote(util.Uint160{}, nil)
	require.Error(t, err)
	_, _, err = neo.Vote(util.Uint160{}, k.PublicKey())
	require.Error(t, err)
	_, err = neo.VoteTransaction(util.Uint160{}, nil)
	require.Error(t, err)
	_, err = neo.VoteTransaction(util.Uint160{}, k.PublicKey())
	require.Error(t, err)
	_, err = neo.VoteUnsigned(util.Uint160{}, nil)
	require.Error(t, err)
	_, err = neo.VoteUnsigned(util.Uint160{}, k.PublicKey())
	require.Error(t, err)

	ta.err = nil
	ta.txh = util.Uint256{1, 2, 3}
	ta.vub = 42

	h, vub, err := neo.Vote(util.Uint160{}, nil)
	require.NoError(t, err)
	require.Equal(t, ta.txh, h)
	require.Equal(t, ta.vub, vub)
	h, vub, err = neo.Vote(util.Uint160{}, k.PublicKey())
	require.NoError(t, err)
	require.Equal(t, ta.txh, h)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := neo.VoteTransaction(util.Uint160{}, nil)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
	tx, err = neo.VoteUnsigned(util.Uint160{}, k.PublicKey())
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
}

func TestRegisterCandidate(t *testing.T) {
	ta := new(testAct)
	neo := New(ta)

	k, err := keys.NewPrivateKey()
	require.NoError(t, err)
	pk := k.PublicKey()

	ta.rer = errors.New("")
	_, _, err = neo.RegisterCandidate(pk)
	require.Error(t, err)
	_, err = neo.RegisterCandidateTransaction(pk)
	require.Error(t, err)
	_, err = neo.RegisterCandidateUnsigned(pk)
	require.Error(t, err)

	ta.rer = nil
	ta.txh = util.Uint256{1, 2, 3}
	ta.vub = 42
	ta.rre = &result.Invoke{
		GasConsumed: 100500,
	}
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(42),
		},
	}

	h, vub, err := neo.RegisterCandidate(pk)
	require.NoError(t, err)
	require.Equal(t, ta.txh, h)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := neo.RegisterCandidateTransaction(pk)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
	tx, err = neo.RegisterCandidateUnsigned(pk)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	ta.ser = errors.New("")
	_, err = neo.RegisterCandidateTransaction(pk)
	require.Error(t, err)

	ta.err = errors.New("")
	_, err = neo.RegisterCandidateUnsigned(pk)
	require.Error(t, err)
}

func TestUnregisterCandidate(t *testing.T) {
	ta := new(testAct)
	neo := New(ta)

	k, err := keys.NewPrivateKey()
	require.NoError(t, err)
	pk := k.PublicKey()

	ta.err = errors.New("")
	_, _, err = neo.UnregisterCandidate(pk)
	require.Error(t, err)
	_, err = neo.UnregisterCandidateTransaction(pk)
	require.Error(t, err)
	_, err = neo.UnregisterCandidateUnsigned(pk)
	require.Error(t, err)

	ta.err = nil
	ta.txh = util.Uint256{1, 2, 3}
	ta.vub = 42

	h, vub, err := neo.UnregisterCandidate(pk)
	require.NoError(t, err)
	require.Equal(t, ta.txh, h)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := neo.UnregisterCandidateTransaction(pk)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
	tx, err = neo.UnregisterCandidateUnsigned(pk)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
}
