use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;
use bigdecimal::BigDecimal;
use neo_core::transaction::{Transaction, Attribute};
use neo_core::util::{Uint160, Uint256};
use neo_core::vm::stackitem::StackItem;
use neo_core::neorpc::result::{Invoke, Iterator};
use neo_core::smartcontract::SmartContract;
use neo_core::require;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<Invoke>,
    tx: Option<Transaction>,
    txh: Option<Uint256>,
    vub: Option<u32>,
}

impl TestAct {
    fn call(&self, contract: Uint160, operation: &str, params: Vec<StackItem>) -> Result<Invoke, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn call_and_expand_iterator(&self, contract: Uint160, method: &str, max_items: i32, params: Vec<StackItem>) -> Result<Invoke, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn terminate_session(&self, session_id: Uuid) -> Result<(), Box<dyn Error>> {
        match &self.err {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    fn traverse_iterator(&self, session_id: Uuid, iterator: &Iterator, num: i32) -> Result<Vec<StackItem>, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.stack.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_run(&self, script: Vec<u8>) -> Result<Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_unsigned_run(&self, script: Vec<u8>, attrs: Vec<Attribute>) -> Result<Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn send_run(&self, script: Vec<u8>) -> Result<(Uint256, u32), Box<dyn Error>> {
        match (&self.txh, &self.vub) {
            (Some(txh), Some(vub)) => Ok((txh.clone(), *vub)),
            _ => Err(self.err.clone().unwrap()),
        }
    }

    fn make_call(&self, contract: Uint160, method: &str, params: Vec<StackItem>) -> Result<Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_unsigned_call(&self, contract: Uint160, method: &str, attrs: Vec<Attribute>, params: Vec<StackItem>) -> Result<Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn send_call(&self, contract: Uint160, method: &str, params: Vec<StackItem>) -> Result<(Uint256, u32), Box<dyn Error>> {
        match (&self.txh, &self.vub) {
            (Some(txh), Some(vub)) => Ok((txh.clone(), *vub)),
            _ => Err(self.err.clone().unwrap()),
        }
    }

    fn sign_and_send(&self, tx: &Transaction) -> Result<(Uint256, u32), Box<dyn Error>> {
        match (&self.txh, &self.vub) {
            (Some(txh), Some(vub)) => Ok((txh.clone(), *vub)),
            _ => Err(self.err.clone().unwrap()),
        }
    }
}

#[test]
fn test_simple_getters() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: None,
    };
    let nns = NewReader(Arc::new(ta), Uint160::new([1, 2, 3]));

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = nns.get_price(A as u8);
    require::error(result);

    let result = nns.is_available("nspcc.neo");
    require::error(result);

    let result = nns.resolve("nspcc.neo", A);
    require::error(result);

    let result = nns.get_record("nspcc.neo", A);
    require::error(result);

    ta.err = None;
    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from(100500)],
    });

    let price = nns.get_price(A as u8).unwrap();
    require::no_error(price);
    require::equal(&BigDecimal::from(100500), &price);

    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from(true)],
    });

    let ava = nns.is_available("nspcc.neo").unwrap();
    require::no_error(ava);
    require::equal(&true, &ava);

    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from("some text")],
    });

    let txt = nns.resolve("nspcc.neo", TXT).unwrap();
    require::no_error(txt);
    require::equal(&"some text".to_string(), &txt);

    let rec = nns.get_record("nspcc.neo", TXT).unwrap();
    require::no_error(rec);
    require::equal(&"some text".to_string(), &rec);
}

#[test]
fn test_get_all_records() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: None,
    };
    let nns = NewReader(Arc::new(ta), Uint160::new([1, 2, 3]));

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = nns.get_all_records("nspcc.neo");
    require::error(result);

    ta.err = None;
    let iid = Uuid::new_v4();
    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator { id: Some(iid), values: vec![] })],
    });

    let result = nns.get_all_records("nspcc.neo");
    require::error(result);

    // Session-based iterator.
    let sid = Uuid::new_v4();
    ta.res = Some(Invoke {
        session: Some(sid),
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator { id: Some(iid), values: vec![] })],
    });

    let iter = nns.get_all_records("nspcc.neo").unwrap();
    require::no_error(iter);

    ta.res = Some(Invoke {
        stack: vec![StackItem::from(vec![
            StackItem::from("n3"),
            StackItem::from(16),
            StackItem::from("cool"),
        ])],
    });

    let vals = iter.next(10).unwrap();
    require::no_error(vals);
    require::equal(&1, &vals.len());
    require::equal(&RecordState {
        name: "n3".to_string(),
        type_: TXT,
        data: "cool".to_string(),
    }, &vals[0]);

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = iter.next(1);
    require::error(result);

    let result = iter.terminate();
    require::error(result);

    // Value-based iterator.
    ta.err = None;
    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator {
            id: None,
            values: vec![
                StackItem::from("n3"),
                StackItem::from(16),
                StackItem::from("cool"),
            ],
        })],
    });

    let iter = nns.get_all_records("nspcc.neo").unwrap();
    require::no_error(iter);

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = iter.terminate();
    require::no_error(result);

    ta.err = None;
    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator {
            id: None,
            values: vec![
                StackItem::from("valid data"),
                StackItem::from(-1),
            ],
        })],
    });

    let iter = nns.get_all_records("nspcc.neo").unwrap();
    require::no_error(iter);

    let result = iter.next(10);
    require::error(result);
    require::contains(&result.unwrap_err().to_string(), "item #0: ");
}

#[test]
fn test_get_all_records_expanded() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: None,
    };
    let nns = NewReader(Arc::new(ta), Uint160::new([1, 2, 3]));

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = nns.get_all_records_expanded("nspcc.neo", 8);
    require::error(result);

    ta.err = None;
    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from(42)],
    });

    let result = nns.get_all_records_expanded("nspcc.neo", 8);
    require::error(result);

    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from(vec![
            StackItem::from(vec![
                StackItem::from("n3"),
                StackItem::from(16),
                StackItem::from("cool"),
            ]),
        ])],
    });

    let vals = nns.get_all_records_expanded("nspcc.neo", 8).unwrap();
    require::no_error(vals);
    require::equal(&1, &vals.len());
    require::equal(&RecordState {
        name: "n3".to_string(),
        type_: TXT,
        data: "cool".to_string(),
    }, &vals[0]);
}

#[test]
fn test_roots() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: None,
    };
    let nns = NewReader(Arc::new(ta), Uint160::new([1, 2, 3]));

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = nns.roots();
    require::error(result);

    let iid = Uuid::new_v4();

    // Session-based iterator.
    let sid = Uuid::new_v4();
    ta.res = Some(Invoke {
        session: Some(sid),
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator { id: Some(iid), values: vec![] })],
    });

    ta.err = None;
    let iter = nns.roots().unwrap();
    require::no_error(iter);

    ta.res = Some(Invoke {
        stack: vec![StackItem::from(vec![
            StackItem::from("n3"),
            StackItem::from("aaaaaa"),
            StackItem::from("cool"),
        ])],
    });

    let vals = iter.next(10).unwrap();
    require::no_error(vals);
    require::equal(&1, &vals.len());
    require::equal(&"n3".to_string(), &vals[0]);

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = iter.next(1);
    require::error(result);

    let result = iter.terminate();
    require::error(result);

    // Value-based iterator.
    ta.err = None;
    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator {
            id: None,
            values: vec![
                StackItem::from("n3"),
                StackItem::from("aaaaaa"),
                StackItem::from("cool"),
            ],
        })],
    });

    let iter = nns.roots().unwrap();
    require::no_error(iter);

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    let result = iter.terminate();
    require::no_error(result);

    let sid = Uuid::new_v4();
    let iid = Uuid::new_v4();
    ta.res = Some(Invoke {
        session: Some(sid),
        state: "HALT".to_string(),
        stack: vec![StackItem::new_interop(Iterator {
            id: Some(iid),
            values: vec![StackItem::from("incorrect format")],
        })],
    });

    ta.err = None;
    let iter = nns.roots().unwrap();
    require::no_error(iter);

    let result = iter.next(10);
    require::error(result);
    require::equal(&"wrong number of elements".to_string(), &result.unwrap_err().to_string());

    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from(vec![
            StackItem::from(vec![StackItem::from("root1")]),
            StackItem::from(vec![StackItem::from("root2")]),
        ])],
    });

    let roots = nns.roots_expanded(10).unwrap();
    require::no_error(roots);
    require::equal(&vec!["root1".to_string(), "root2".to_string()], &roots);

    ta.res = Some(Invoke {
        state: "HALT".to_string(),
        stack: vec![StackItem::from("incorrect format")], // Not a slice of stackitem.Item
    });

    let result = nns.roots_expanded(10);
    require::error(result);
    require::equal(&"not an array".to_string(), &result.unwrap_err().to_string());

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "call and expand iterator error")));
    let result = nns.roots_expanded(10);
    require::error(result);
    require::equal(&"call and expand iterator error".to_string(), &result.unwrap_err().to_string());
}

#[test]
fn test_update() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: None,
    };
    let nns = New(Arc::new(ta), Uint160::new([1, 2, 3]));

    let nef = vec![0x01, 0x02, 0x03];
    let manifest = "manifest data".to_string();

    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "test error")));
    let result = nns.update(&nef, &manifest);
    require::error(result);

    // Test successful update
    ta.err = None;
    ta.txh = Some(Uint256::new([0x04, 0x05, 0x06]));
    let (txh, vub) = nns.update(&nef, &manifest).unwrap();
    require::no_error(txh);
    require::equal(&ta.txh.unwrap(), &txh);
    require::equal(&ta.vub.unwrap(), &vub);

    for fun in vec![nns.update_transaction, nns.update_unsigned] {
        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        let result = fun(&nef, &manifest);
        require::error(result);

        ta.err = None;
        ta.tx = Some(Transaction::new(100500, 42));
        let tx = fun(&nef, &manifest).unwrap();
        require::no_error(tx);
        require::equal(&ta.tx.unwrap(), &tx);
    }
}

#[test]
fn test_add_root() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: None,
    };
    let nns = New(Arc::new(ta), Uint160::new([1, 2, 3]));

    let root = "example.root".to_string();
    let params = SmartContract::new_parameter_from_value(&root).unwrap();
    ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "test error")));
    let result = nns.add_root(&params.value.as_str().unwrap());
    require::error(result);

    // Test success case
    ta.err = None;
    ta.txh = Some(Uint256::new([0x07, 0x08, 0x09]));
    let (txh, vub) = nns.add_root(&root).unwrap();
    require::no_error(txh);
    require::equal(&ta.txh.unwrap(), &txh);
    require::equal(&ta.vub.unwrap(), &vub);

    ta.tx = Some(Transaction::new(100500, 42));
    let tx = nns.add_root_transaction(&root).unwrap();
    require::no_error(tx);
    require::equal(&ta.tx.unwrap(), &tx);

import (
	"errors"
	"math/big"
	"testing"

	"github.com/google/uuid"
	"github.com/nspcc-dev/neo-go/pkg/core/transaction"
	"github.com/nspcc-dev/neo-go/pkg/neorpc/result"
	"github.com/nspcc-dev/neo-go/pkg/smartcontract"
	"github.com/nspcc-dev/neo-go/pkg/util"
	"github.com/nspcc-dev/neo-go/pkg/vm/stackitem"
	"github.com/stretchr/testify/require"
)

type testAct struct {
	err error
	res *result.Invoke
	tx  *transaction.Transaction
	txh util.Uint256
	vub uint32
}

func (t *testAct) Call(contract util.Uint160, operation string, params ...any) (*result.Invoke, error) {
	return t.res, t.err
}
func (t *testAct) CallAndExpandIterator(contract util.Uint160, method string, maxItems int, params ...any) (*result.Invoke, error) {
	return t.res, t.err
}
func (t *testAct) TerminateSession(sessionID uuid.UUID) error {
	return t.err
}
func (t *testAct) TraverseIterator(sessionID uuid.UUID, iterator *result.Iterator, num int) ([]stackitem.Item, error) {
	return t.res.Stack, t.err
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

func (t *testAct) SignAndSend(tx *transaction.Transaction) (util.Uint256, uint32, error) {
	return t.txh, t.vub, t.err
}

func TestSimpleGetters(t *testing.T) {
	ta := &testAct{}
	nns := NewReader(ta, util.Uint160{1, 2, 3})

	ta.err = errors.New("")
	_, err := nns.GetPrice(uint8(A))
	require.Error(t, err)
	_, err = nns.IsAvailable("nspcc.neo")
	require.Error(t, err)
	_, err = nns.Resolve("nspcc.neo", A)
	require.Error(t, err)
	_, err = nns.GetRecord("nspcc.neo", A)
	require.Error(t, err)

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(100500),
		},
	}
	price, err := nns.GetPrice(uint8(A))
	require.NoError(t, err)
	require.Equal(t, new(big.Int).SetInt64(100500), price)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(true),
		},
	}
	ava, err := nns.IsAvailable("nspcc.neo")
	require.NoError(t, err)
	require.Equal(t, true, ava)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make("some text"),
		},
	}
	txt, err := nns.Resolve("nspcc.neo", TXT)
	require.NoError(t, err)
	require.Equal(t, "some text", txt)

	rec, err := nns.GetRecord("nspcc.neo", TXT)
	require.NoError(t, err)
	require.Equal(t, "some text", rec)
}

func TestGetAllRecords(t *testing.T) {
	ta := &testAct{}
	nns := NewReader(ta, util.Uint160{1, 2, 3})

	ta.err = errors.New("")
	_, err := nns.GetAllRecords("nspcc.neo")
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
	_, err = nns.GetAllRecords("nspcc.neo")
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
	iter, err := nns.GetAllRecords("nspcc.neo")
	require.NoError(t, err)

	ta.res = &result.Invoke{
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make("n3"),
				stackitem.Make(16),
				stackitem.Make("cool"),
			}),
		},
	}
	vals, err := iter.Next(10)
	require.NoError(t, err)
	require.Equal(t, 1, len(vals))
	require.Equal(t, RecordState{
		Name: "n3",
		Type: TXT,
		Data: "cool",
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
					stackitem.Make("n3"),
					stackitem.Make(16),
					stackitem.Make("cool"),
				},
			}),
		},
	}
	iter, err = nns.GetAllRecords("nspcc.neo")
	require.NoError(t, err)

	ta.err = errors.New("")
	err = iter.Terminate()
	require.NoError(t, err)

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.NewInterop(result.Iterator{
				Values: []stackitem.Item{
					stackitem.Make("valid data"),
					stackitem.Make(-1),
				},
			}),
		},
	}
	iter, err = nns.GetAllRecords("nspcc.neo")
	require.NoError(t, err)

	_, err = iter.Next(10)

	require.Error(t, err)
	require.Contains(t, err.Error(), "item #0: ")
}

func TestGetAllRecordsExpanded(t *testing.T) {
	ta := &testAct{}
	nns := NewReader(ta, util.Uint160{1, 2, 3})

	ta.err = errors.New("")
	_, err := nns.GetAllRecordsExpanded("nspcc.neo", 8)
	require.Error(t, err)

	ta.err = nil
	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(42),
		},
	}
	_, err = nns.GetAllRecordsExpanded("nspcc.neo", 8)
	require.Error(t, err)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{
					stackitem.Make("n3"),
					stackitem.Make(16),
					stackitem.Make("cool"),
				}),
			}),
		},
	}
	vals, err := nns.GetAllRecordsExpanded("nspcc.neo", 8)
	require.NoError(t, err)
	require.Equal(t, 1, len(vals))
	require.Equal(t, RecordState{
		Name: "n3",
		Type: TXT,
		Data: "cool",
	}, vals[0])
}

func TestRoots(t *testing.T) {
	ta := &testAct{}
	nns := NewReader(ta, util.Uint160{1, 2, 3})
	ta.err = errors.New("")
	_, err := nns.Roots()
	require.Error(t, err)
	iid := uuid.New()

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
	ta.err = nil
	iter, err := nns.Roots()
	require.NoError(t, err)

	ta.res = &result.Invoke{
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make("n3"),
				stackitem.Make("aaaaaa"),
				stackitem.Make("cool"),
			}),
		},
	}
	vals, err := iter.Next(10)
	require.NoError(t, err)
	require.Equal(t, 1, len(vals))
	require.Equal(t, "n3", vals[0])

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
					stackitem.Make("n3"),
					stackitem.Make("aaaaaa"),
					stackitem.Make("cool"),
				},
			}),
		},
	}
	iter, err = nns.Roots()
	require.NoError(t, err)

	ta.err = errors.New("")
	err = iter.Terminate()
	require.NoError(t, err)

	sid = uuid.New()
	iid = uuid.New()
	ta.res = &result.Invoke{
		Session: sid,
		State:   "HALT",
		Stack: []stackitem.Item{
			stackitem.NewInterop(result.Iterator{
				ID: &iid,
				Values: []stackitem.Item{
					stackitem.Make("incorrect format"),
				},
			}),
		},
	}
	ta.err = nil
	iter, err = nns.Roots()
	require.NoError(t, err)

	_, err = iter.Next(10)
	require.Error(t, err)
	require.Equal(t, "wrong number of elements", err.Error())

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make([]stackitem.Item{
				stackitem.Make([]stackitem.Item{
					stackitem.Make("root1"),
				}),
				stackitem.Make([]stackitem.Item{
					stackitem.Make("root2"),
				}),
			}),
		},
	}

	roots, err := nns.RootsExpanded(10)
	require.NoError(t, err)
	require.Equal(t, []string{"root1", "root2"}, roots)

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make("incorrect format"), // Not a slice of stackitem.Item
		},
	}

	_, err = nns.RootsExpanded(10)
	require.Error(t, err)
	require.Equal(t, "not an array", err.Error())

	ta.err = errors.New("call and expand iterator error")
	_, err = nns.RootsExpanded(10)
	require.Error(t, err)
	require.Equal(t, "call and expand iterator error", err.Error())
}

func TestUpdate(t *testing.T) {
	ta := &testAct{}
	nns := New(ta, util.Uint160{1, 2, 3})

	nef := []byte{0x01, 0x02, 0x03}
	manifest := "manifest data"

	ta.err = errors.New("test error")
	_, _, err := nns.Update(nef, manifest)
	require.Error(t, err)

	// Test successful update
	ta.err = nil
	ta.txh = util.Uint256{0x04, 0x05, 0x06}
	txh, vub, err := nns.Update(nef, manifest)
	require.NoError(t, err)
	require.Equal(t, ta.txh, txh)
	require.Equal(t, ta.vub, vub)

	for _, fun := range []func(nef []byte, manifest string) (*transaction.Transaction, error){
		nns.UpdateTransaction,
		nns.UpdateUnsigned,
	} {
		ta.err = errors.New("")
		_, err := fun(nil, "")
		require.Error(t, err)

		ta.err = nil
		ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
		tx, err := fun(nil, "")
		require.NoError(t, err)
		require.Equal(t, ta.tx, tx)
	}
}

func TestAddRoot(t *testing.T) {
	ta := &testAct{}
	nns := New(ta, util.Uint160{1, 2, 3})

	root := "example.root"
	params, err := smartcontract.NewParameterFromValue(root)
	require.NoError(t, err)
	ta.err = errors.New("test error")
	_, _, err = nns.AddRoot(params.Value.(string))
	require.Error(t, err)

	// Test success case
	ta.err = nil
	ta.txh = util.Uint256{0x07, 0x08, 0x09}
	txh, vub, err := nns.AddRoot(root)
	require.NoError(t, err)
	require.Equal(t, ta.txh, txh)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := nns.AddRootTransaction(root)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
	tx, err = nns.AddRootUnsigned(root)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	ta.err = errors.New("")
	_, err = nns.AddRootTransaction(root)
	require.Error(t, err)

	ta.err = errors.New("")
	_, err = nns.AddRootUnsigned(root)
	require.Error(t, err)
}

func TestSetPrice(t *testing.T) {
	ta := &testAct{}
	nns := New(ta, util.Uint160{1, 2, 3})

	priceList := []int64{100, 200}
	ta.err = errors.New("test error")
	_, _, err := nns.SetPrice(priceList)
	require.Error(t, err)
	_, err = nns.SetPriceTransaction(priceList)
	require.Error(t, err)
	_, err = nns.SetPriceUnsigned(priceList)
	require.Error(t, err)

	// Test success case
	ta.err = nil
	ta.txh = util.Uint256{0x0A, 0x0B, 0x0C}
	ta.vub = 42

	ta.res = &result.Invoke{
		State: "HALT",
		Stack: []stackitem.Item{
			stackitem.Make(42),
		},
	}

	txh, vub, err := nns.SetPrice(priceList)
	require.NoError(t, err)
	require.Equal(t, ta.txh, txh)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := nns.SetPriceTransaction(priceList)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)
	tx, err = nns.SetPriceUnsigned(priceList)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	ta.err = errors.New("")
	_, err = nns.SetPriceTransaction(priceList)
	require.Error(t, err)

	ta.err = errors.New("")
	_, err = nns.SetPriceUnsigned(priceList)
	require.Error(t, err)
}

func TestRegister(t *testing.T) {
	ta := &testAct{}
	nns := New(ta, util.Uint160{1, 2, 3})

	name := "example.neo"
	owner := util.Uint160{0x0D, 0x0E, 0x0F}

	ta.err = errors.New("test error")
	txh, vub, err := nns.Register(name, owner)
	require.Error(t, err)
	require.Equal(t, util.Uint256{}, txh) // Check if returned Uint256 is zero-initialized
	require.Equal(t, uint32(0), vub)

	// Test success case
	ta.err = nil
	ta.txh = util.Uint256{0x10, 0x11, 0x12}
	txh, vub, err = nns.Register(name, owner)
	require.NoError(t, err)
	require.Equal(t, ta.txh, txh)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := nns.RegisterTransaction(name, owner)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	tx, err = nns.RegisterUnsigned(name, owner)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	ta.err = errors.New("")
	_, err = nns.RegisterTransaction(name, owner)
	require.Error(t, err)

	ta.err = errors.New("")
	_, err = nns.RegisterUnsigned(name, owner)
	require.Error(t, err)
}

func TestRenew(t *testing.T) {
	ta := &testAct{}
	nns := New(ta, util.Uint160{1, 2, 3})

	name := "example.neo"

	ta.err = errors.New("test error")
	_, _, err := nns.Renew(name)
	require.Error(t, err)

	// Test success case
	ta.err = nil
	ta.txh = util.Uint256{0x13, 0x14, 0x15}
	txh, vub, err := nns.Renew(name)
	require.NoError(t, err)
	require.Equal(t, ta.txh, txh)
	require.Equal(t, ta.vub, vub)

	txh, vub, err = nns.Renew2(name, 1)
	require.NoError(t, err)
	require.Equal(t, ta.txh, txh)
	require.Equal(t, ta.vub, vub)

	ta.tx = &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	tx, err := nns.RenewTransaction(name)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	tx, err = nns.RenewUnsigned(name)
	require.NoError(t, err)
	require.Equal(t, ta.tx, tx)

	ta.err = errors.New("")
	_, err = nns.RenewTransaction(name)
	require.Error(t, err)

	ta.err = errors.New("")
	_, err = nns.RenewUnsigned(name)
	require.Error(t, err)
}

func TestSetAdmin(t *testing.T) {
	ta := &testAct{}
	c := New(ta, util.Uint160{1, 2, 3})

	name := "example.neo"
	admin := util.Uint160{4, 5, 6}
	txMock := &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	txhMock := util.Uint256{0x13, 0x14, 0x15}

	testCases := []struct {
		name     string
		setup    func()
		testFunc func() (interface{}, error)
		want     interface{}
		wantErr  bool
	}{
		{
			name: "SetAdmin - Error",
			setup: func() {
				ta.err = errors.New("test error")
			},
			testFunc: func() (interface{}, error) {
				txh, vub, err := c.SetAdmin(name, admin)
				return []interface{}{txh, vub}, err
			},
			wantErr: true,
		},
		{
			name: "SetAdmin - Success",
			setup: func() {
				ta.err = nil
				ta.txh = txhMock
				ta.vub = 42
			},
			testFunc: func() (interface{}, error) {
				txh, vub, err := c.SetAdmin(name, admin)
				return []interface{}{txh, vub}, err
			},
			want: []interface{}{txhMock, uint32(42)},
		},
		{
			name: "SetAdminTransaction - Success",
			setup: func() {
				ta.err = nil
				ta.tx = txMock
			},
			testFunc: func() (interface{}, error) {
				return c.SetAdminTransaction(name, admin)
			},
			want: txMock,
		},
		{
			name: "SetAdminTransaction - Error",
			setup: func() {
				ta.err = errors.New("test error")
			},
			testFunc: func() (interface{}, error) {
				return c.SetAdminTransaction(name, admin)
			},
			wantErr: true,
		},
		{
			name: "SetAdminUnsigned - Success",
			setup: func() {
				ta.err = nil
				ta.tx = txMock
			},
			testFunc: func() (interface{}, error) {
				return c.SetAdminUnsigned(name, admin)
			},
			want: txMock,
		},
		{
			name: "SetAdminUnsigned - Error",
			setup: func() {
				ta.err = errors.New("test error")
			},
			testFunc: func() (interface{}, error) {
				return c.SetAdminUnsigned(name, admin)
			},
			wantErr: true,
		},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			tc.setup()
			got, err := tc.testFunc()
			if tc.wantErr {
				require.Error(t, err)
			} else {
				require.NoError(t, err)
				require.Equal(t, tc.want, got)
			}
		})
	}
}

func TestSetRecord(t *testing.T) {
	ta := &testAct{}
	c := New(ta, util.Uint160{1, 2, 3})

	name := "example.neo"
	typev := A
	data := "record data"
	txMock := &transaction.Transaction{Nonce: 100500, ValidUntilBlock: 42}
	txhMock := util.Uint256{0x13, 0x14, 0x15}

	testCases := []struct {
		name     string
		setup    func()
		testFunc func() (interface{}, error)
		want     interface{}
		wantErr  bool
	}{
		{
			name: "SetRecord - Error",
			setup: func() {
				ta.err = errors.New("test error")
			},
			testFunc: func() (interface{}, error) {
				txh, vub, err := c.SetRecord(name, typev, data)
				return []interface{}{txh, vub}, err
			},
			wantErr: true,
		},
		{
			name: "SetRecord - Success",
			setup: func() {
				ta.err = nil
				ta.txh = txhMock
				ta.vub = 42
			},
			testFunc: func() (interface{}, error) {
				txh, vub, err := c.SetRecord(name, typev, data)
				return []interface{}{txh, vub}, err
			},
			want: []interface{}{txhMock, uint32(42)},
		},
		{
			name: "SetRecordTransaction - Success",
			setup: func() {
				ta.err = nil
				ta.tx = txMock
			},
			testFunc: func() (interface{}, error) {
				return c.SetRecordTransaction(name, typev, data)
			},
			want: txMock,
		},
		{
			name: "SetRecordTransaction - Error",
			setup: func() {
				ta.err = errors.New("test error")
			},
			testFunc: func() (interface{}, error) {
				return c.SetRecordTransaction(name, typev, data)
			},
			wantErr: true,
		},
		{
			name: "SetRecordUnsigned - Success",
			setup: func() {
				ta.err = nil
				ta.tx = txMock
			},
			testFunc: func() (interface{}, error) {
				return c.SetRecordUnsigned(name, typev, data)
			},
			want: txMock,
		},
		{
			name: "SetRecordUnsigned - Error",
			setup: func() {
				ta.err = errors.New("test error")
			},
			testFunc: func() (interface{}, error) {
				return c.SetRecordUnsigned(name, typev, data)
			},
			wantErr: true,
		},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			tc.setup()
			got, err := tc.testFunc()
			if tc.wantErr {
				require.Error(t, err)
			} else {
				require.NoError(t, err)
				require.Equal(t, tc.want, got)
			}
		})
	}
}
