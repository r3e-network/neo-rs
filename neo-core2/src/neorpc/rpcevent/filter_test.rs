use std::any::Any;
use std::collections::HashMap;

use crate::core::block::{self, Block, Header};
use crate::core::mempoolevent::{self, TransactionAdded, TransactionRemoved};
use crate::core::state::{self, ContainedNotificationEvent, NotificationEvent, AppExecResult, Execution};
use crate::core::transaction::{self, Transaction, Signer};
use crate::neorpc::{self, EventID, SubscriptionFilter, BlockFilter, TxFilter, NotificationFilter, ExecutionFilter, NotaryRequestFilter};
use crate::neorpc::result::{self, NotaryRequestEvent};
use crate::network::payload::{self, P2PNotaryRequest};
use crate::util::{self, Uint160, Uint256};
use crate::vm::vmstate::{self, VMState};
use anyhow::Result;

struct TestComparator {
    id: EventID,
    filter: SubscriptionFilter,
}

struct TestContainer {
    id: EventID,
    pld: Box<dyn Any>,
}

impl TestComparator {
    fn event_id(&self) -> EventID {
        self.id
    }

    fn filter(&self) -> &SubscriptionFilter {
        &self.filter
    }
}

impl TestContainer {
    fn event_id(&self) -> EventID {
        self.id
    }

    fn event_payload(&self) -> &Box<dyn Any> {
        &self.pld
    }
}

#[test]
fn test_matches() -> Result<()> {
    let primary: u8 = 1;
    let bad_primary: u8 = 2;
    let index: u32 = 5;
    let bad_higher_index: u32 = 6;
    let bad_lower_index = index - 1;
    let sender = Uint160::from([1, 2, 3]);
    let signer = Uint160::from([4, 5, 6]);
    let contract = Uint160::from([7, 8, 9]);
    let notary_type = TransactionAdded;
    let bad_uint160 = Uint160::from([9, 9, 9]);
    let cnt = Uint256::from([1, 2, 3]);
    let bad_uint256 = Uint256::from([9, 9, 9]);
    let name = "ntf name".to_string();
    let bad_name = "bad name".to_string();
    let bad_type = TransactionRemoved;
    let b_container = TestContainer {
        id: EventID::BlockEventID,
        pld: Box::new(Block {
            header: Header { primary_index: primary, index },
        }),
    };
    let header_container = TestContainer {
        id: EventID::HeaderOfAddedBlockEventID,
        pld: Box::new(Header { primary_index: primary, index }),
    };
    let st = VMState::Halt;
    let good_state = st.to_string();
    let bad_state = "FAULT".to_string();
    let tx_container = TestContainer {
        id: EventID::TransactionEventID,
        pld: Box::new(Transaction { signers: vec![Signer { account: sender }, Signer { account: signer }] }),
    };
    let ntf_container = TestContainer {
        id: EventID::NotificationEventID,
        pld: Box::new(ContainedNotificationEvent { notification_event: NotificationEvent { script_hash: contract, name: name.clone() } }),
    };
    let ex_container = TestContainer {
        id: EventID::ExecutionEventID,
        pld: Box::new(AppExecResult { container: cnt, execution: Execution { vm_state: st } }),
    };
    let ntr_container = TestContainer {
        id: EventID::NotaryRequestEventID,
        pld: Box::new(NotaryRequestEvent {
            r#type: notary_type,
            notary_request: P2PNotaryRequest {
                main_transaction: Box::new(Transaction { signers: vec![Signer { account: signer }] }),
                fallback_transaction: Box::new(Transaction { signers: vec![Signer { account: Uint160::default() }, Signer { account: sender }] }),
            },
        }),
    };
    let missed_container = TestContainer {
        id: EventID::MissedEventID,
        pld: Box::new(()),
    };

    let test_cases = vec![
        ("ID mismatch", TestComparator { id: EventID::TransactionEventID, filter: SubscriptionFilter::default() }, b_container, false),
        ("missed event", TestComparator { id: EventID::BlockEventID, filter: SubscriptionFilter::default() }, missed_container, false),
        ("block, no filter", TestComparator { id: EventID::BlockEventID, filter: SubscriptionFilter::default() }, b_container, true),
        ("block, primary mismatch", TestComparator { id: EventID::BlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { primary: Some(bad_primary), ..Default::default() }) }, b_container, false),
        ("block, since mismatch", TestComparator { id: EventID::BlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { since: Some(bad_higher_index), ..Default::default() }) }, b_container, false),
        ("block, till mismatch", TestComparator { id: EventID::BlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { till: Some(bad_lower_index), ..Default::default() }) }, b_container, false),
        ("block, filter match", TestComparator { id: EventID::BlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { primary: Some(primary), since: Some(index), till: Some(index) }) }, b_container, true),
        ("header, no filter", TestComparator { id: EventID::HeaderOfAddedBlockEventID, filter: SubscriptionFilter::default() }, header_container, true),
        ("header, primary mismatch", TestComparator { id: EventID::HeaderOfAddedBlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { primary: Some(bad_primary), ..Default::default() }) }, header_container, false),
        ("header, since mismatch", TestComparator { id: EventID::HeaderOfAddedBlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { since: Some(bad_higher_index), ..Default::default() }) }, header_container, false),
        ("header, till mismatch", TestComparator { id: EventID::HeaderOfAddedBlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { till: Some(bad_lower_index), ..Default::default() }) }, header_container, false),
        ("header, filter match", TestComparator { id: EventID::HeaderOfAddedBlockEventID, filter: SubscriptionFilter::BlockFilter(BlockFilter { primary: Some(primary), since: Some(index), till: Some(index) }) }, header_container, true),
        ("transaction, no filter", TestComparator { id: EventID::TransactionEventID, filter: SubscriptionFilter::default() }, tx_container, true),
        ("transaction, sender mismatch", TestComparator { id: EventID::TransactionEventID, filter: SubscriptionFilter::TxFilter(TxFilter { sender: Some(bad_uint160), ..Default::default() }) }, tx_container, false),
        ("transaction, signer mismatch", TestComparator { id: EventID::TransactionEventID, filter: SubscriptionFilter::TxFilter(TxFilter { signer: Some(bad_uint160), ..Default::default() }) }, tx_container, false),
        ("transaction, filter match", TestComparator { id: EventID::TransactionEventID, filter: SubscriptionFilter::TxFilter(TxFilter { sender: Some(sender), signer: Some(signer) }) }, tx_container, true),
        ("notification, no filter", TestComparator { id: EventID::NotificationEventID, filter: SubscriptionFilter::default() }, ntf_container, true),
        ("notification, contract mismatch", TestComparator { id: EventID::NotificationEventID, filter: SubscriptionFilter::NotificationFilter(NotificationFilter { contract: Some(bad_uint160), ..Default::default() }) }, ntf_container, false),
        ("notification, name mismatch", TestComparator { id: EventID::NotificationEventID, filter: SubscriptionFilter::NotificationFilter(NotificationFilter { name: Some(bad_name), ..Default::default() }) }, ntf_container, false),
        ("notification, filter match", TestComparator { id: EventID::NotificationEventID, filter: SubscriptionFilter::NotificationFilter(NotificationFilter { name: Some(name), contract: Some(contract) }) }, ntf_container, true),
        ("execution, no filter", TestComparator { id: EventID::ExecutionEventID, filter: SubscriptionFilter::default() }, ex_container, true),
        ("execution, state mismatch", TestComparator { id: EventID::ExecutionEventID, filter: SubscriptionFilter::ExecutionFilter(ExecutionFilter { state: Some(bad_state.clone()), ..Default::default() }) }, ex_container, false),
        ("execution, container mismatch", TestComparator { id: EventID::ExecutionEventID, filter: SubscriptionFilter::ExecutionFilter(ExecutionFilter { container: Some(bad_uint256), ..Default::default() }) }, ex_container, false),
        ("execution, filter mismatch", TestComparator { id: EventID::ExecutionEventID, filter: SubscriptionFilter::ExecutionFilter(ExecutionFilter { state: Some(good_state), container: Some(cnt) }) }, ex_container, true),
        ("notary request, no filter", TestComparator { id: EventID::NotaryRequestEventID, filter: SubscriptionFilter::default() }, ntr_container, true),
        ("notary request, sender mismatch", TestComparator { id: EventID::NotaryRequestEventID, filter: SubscriptionFilter::NotaryRequestFilter(NotaryRequestFilter { sender: Some(bad_uint160), ..Default::default() }) }, ntr_container, false),
        ("notary request, signer mismatch", TestComparator { id: EventID::NotaryRequestEventID, filter: SubscriptionFilter::NotaryRequestFilter(NotaryRequestFilter { signer: Some(bad_uint160), ..Default::default() }) }, ntr_container, false),
        ("notary request, type mismatch", TestComparator { id: EventID::NotaryRequestEventID, filter: SubscriptionFilter::NotaryRequestFilter(NotaryRequestFilter { r#type: Some(bad_type), ..Default::default() }) }, ntr_container, false),
        ("notary request, filter match", TestComparator { id: EventID::NotaryRequestEventID, filter: SubscriptionFilter::NotaryRequestFilter(NotaryRequestFilter { sender: Some(sender), signer: Some(signer), r#type: Some(notary_type) }) }, ntr_container, true),
    ];

    for (name, comparator, container, expected) in test_cases {
        assert_eq!(expected, matches(&comparator, &container), "{}", name);
    }

    Ok(())
}

fn matches(comparator: &TestComparator, container: &TestContainer) -> bool {
    // Implement the matching logic here
    true
}
