use crate::core::block;
use crate::core::state;
use crate::core::transaction;
use crate::neorpc;
use crate::neorpc::result;

pub trait Comparator {
    fn event_id(&self) -> neorpc::EventID;
    fn filter(&self) -> Option<neorpc::SubscriptionFilter>;
}

pub trait Container {
    fn event_id(&self) -> neorpc::EventID;
    fn event_payload(&self) -> &dyn std::any::Any;
}

pub fn matches(f: &dyn Comparator, r: &dyn Container) -> bool {
    let expected_event = f.event_id();
    let filter = f.filter();
    if r.event_id() != expected_event {
        return false;
    }
    if filter.is_none() {
        return true;
    }
    match f.event_id() {
        neorpc::EventID::BlockEventID | neorpc::EventID::HeaderOfAddedBlockEventID => {
            let filt = filter.unwrap().as_block_filter().unwrap();
            let b = if f.event_id() == neorpc::EventID::HeaderOfAddedBlockEventID {
                r.event_payload().downcast_ref::<block::Header>().unwrap()
            } else {
                &r.event_payload().downcast_ref::<block::Block>().unwrap().header
            };
            let primary_ok = filt.primary.map_or(true, |p| p == b.primary_index);
            let since_ok = filt.since.map_or(true, |s| s <= b.index);
            let till_ok = filt.till.map_or(true, |t| b.index <= t);
            primary_ok && since_ok && till_ok
        }
        neorpc::EventID::TransactionEventID => {
            let filt = filter.unwrap().as_tx_filter().unwrap();
            let tx = r.event_payload().downcast_ref::<transaction::Transaction>().unwrap();
            let sender_ok = filt.sender.map_or(true, |s| tx.sender().equals(&s));
            let mut signer_ok = true;
            if let Some(signer) = filt.signer {
                signer_ok = tx.signers.iter().any(|s| s.account.equals(&signer));
            }
            sender_ok && signer_ok
        }
        neorpc::EventID::NotificationEventID => {
            let filt = filter.unwrap().as_notification_filter().unwrap();
            let notification = r.event_payload().downcast_ref::<state::ContainedNotificationEvent>().unwrap();
            let hash_ok = filt.contract.map_or(true, |c| notification.script_hash.equals(&c));
            let name_ok = filt.name.map_or(true, |n| notification.name == n);
            hash_ok && name_ok
        }
        neorpc::EventID::ExecutionEventID => {
            let filt = filter.unwrap().as_execution_filter().unwrap();
            let applog = r.event_payload().downcast_ref::<state::AppExecResult>().unwrap();
            let state_ok = filt.state.map_or(true, |s| applog.vm_state.to_string() == s);
            let container_ok = filt.container.map_or(true, |c| applog.container.equals(&c));
            state_ok && container_ok
        }
        neorpc::EventID::NotaryRequestEventID => {
            let filt = filter.unwrap().as_notary_request_filter().unwrap();
            let req = r.event_payload().downcast_ref::<result::NotaryRequestEvent>().unwrap();
            let type_ok = filt.type_.map_or(true, |t| req.type_ == t);
            let sender_ok = filt.sender.map_or(true, |s| req.notary_request.fallback_transaction.signers[1].account == s);
            let mut signer_ok = true;
            if let Some(signer) = filt.signer {
                signer_ok = req.notary_request.main_transaction.signers.iter().any(|s| s.account.equals(&signer));
            }
            sender_ok && signer_ok && type_ok
        }
        _ => false,
    }
}
