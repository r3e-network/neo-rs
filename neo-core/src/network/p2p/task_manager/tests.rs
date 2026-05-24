use super::*;
use crate::akka::ActorPath;
use crate::neo_system::NeoSystem;
use crate::protocol_settings::ProtocolSettings;
use std::sync::Weak;
use tokio::sync::mpsc;

fn insert_session(manager: &mut TaskManager, actor: &ActorRef) -> String {
    let path = actor.path().to_string();
    manager.sessions.insert(
        path.clone(),
        SessionEntry {
            actor: actor.clone(),
            session: TaskSession::new(&VersionPayload::default()),
        },
    );
    path
}

fn block_with_index_and_nonce(index: u32, nonce: u64) -> Block {
    let mut block = Block::new();
    block.header.set_index(index);
    block.header.set_nonce(nonce);
    block
}

fn block_hash(block: &Block) -> UInt256 {
    let mut clone = block.clone();
    clone.try_hash().expect("block hash")
}

#[test]
fn complete_inventory_records_non_header_hashes_only() {
    let mut manager = TaskManager::new();
    let (mailbox, _rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-known-hash"),
        mailbox,
        Weak::new(),
    );
    let block_hash = UInt256::from([9u8; 32]);

    manager.complete_inventory(&actor, HEADER_TASK_HASH, None, None);
    assert!(!manager.is_known_hash(&HEADER_TASK_HASH));

    manager.complete_inventory(&actor, block_hash, None, None);
    assert!(manager.is_known_hash(&block_hash));
}

#[test]
fn mempool_request_is_one_shot_and_marks_sent_before_delivery_result() {
    let mut session = TaskSession::new(&VersionPayload::default());
    let (mailbox, mut rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-mempool"),
        mailbox,
        Weak::new(),
    );

    assert!(request_mempool_once(&actor, &mut session));
    assert!(session.mempool_sent);
    assert!(rx.try_recv().is_ok());

    assert!(!request_mempool_once(&actor, &mut session));
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn request_tasks_rolls_back_header_and_index_tasks_when_peer_send_fails() {
    let neo_system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut manager = TaskManager::new();
    manager.system = Some(neo_system.context());
    let (mailbox, rx) = mpsc::channel(1);
    drop(rx);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-send-failure"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    {
        let session = &mut manager.sessions.get_mut(&path).expect("session").session;
        session.is_full_node = true;
        session.last_block_index = 5;
    }

    manager.with_session_mut(&path, |entry, this| {
        this.request_tasks_entry(entry);
    });

    let session = &manager.sessions.get(&path).expect("session").session;
    assert!(manager.global_inv_tasks.is_empty());
    assert!(session.inv_tasks.is_empty());
    assert!(manager.global_index_tasks.is_empty());
    assert!(session.index_tasks.is_empty());

    neo_system.shutdown().await.expect("shutdown");
}

#[test]
fn complete_inventory_stores_hashable_block() {
    let mut manager = TaskManager::new();
    let (mailbox, mut rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-valid-block"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    let block = block_with_index_and_nonce(7, 1);
    let hash = block_hash(&block);

    manager.complete_inventory(&actor, hash, Some(block), Some(7));

    let session = &manager.sessions.get(&path).expect("session").session;
    assert!(session.received_block.contains_key(&7));
    assert!(rx.try_recv().is_err());
}

#[test]
fn complete_inventory_disconnects_conflicting_block() {
    let mut manager = TaskManager::new();
    let (mailbox, mut rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-conflict-block"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    let existing = block_with_index_and_nonce(7, 1);
    manager
        .sessions
        .get_mut(&path)
        .expect("session")
        .session
        .store_received_block(7, existing);
    let incoming = block_with_index_and_nonce(7, 2);
    let incoming_hash = block_hash(&incoming);

    manager.complete_inventory(&actor, incoming_hash, Some(incoming), Some(7));

    assert!(rx.try_recv().is_ok());
}

#[test]
fn persist_completed_disconnects_mismatched_cached_block() {
    let mut manager = TaskManager::new();
    let (mailbox, mut rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-persist-mismatch"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    let stored = block_with_index_and_nonce(9, 1);
    manager
        .sessions
        .get_mut(&path)
        .expect("session")
        .session
        .store_received_block(9, stored);
    let persisted = block_with_index_and_nonce(9, 2);

    manager.on_persist_completed(&persisted);

    assert!(rx.try_recv().is_ok());
    assert!(manager
        .sessions
        .get(&path)
        .expect("session")
        .session
        .received_block
        .is_empty());
}
