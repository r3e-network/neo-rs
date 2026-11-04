use super::*;
use crate::neo_system::NeoSystem;
use crate::protocol_settings::ProtocolSettings;
use crate::runtime::ActorPath;
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
    manager
        .sessions
        .get_mut(&path)
        .expect("session")
        .session
        .register_index_task(7);
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
fn persist_failed_releases_cached_height_for_retry() {
    let mut manager = TaskManager::new();
    let (mailbox, _rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-persist-retry"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    let block = block_with_index_and_nonce(9, 1);
    let session = &mut manager.sessions.get_mut(&path).expect("session").session;
    session.store_received_block(9, block);
    session.register_index_task(9);
    assert!(manager.increment_index_task(9));

    manager.on_persist_failed(9);

    let session = &manager.sessions.get(&path).expect("session").session;
    assert!(!session.received_block.contains_key(&9));
    assert!(!session.index_tasks.contains_key(&9));
    assert!(!manager.global_index_tasks.is_tracked(&9));
}

#[test]
fn stalled_frontier_is_released_and_directly_requested_after_persistence_timeout() {
    let mut manager = TaskManager::new();
    let (mailbox, mut rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-stalled-frontier"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    manager.last_seen_persisted_index = 8;
    manager.last_persisted_at =
        std::time::Instant::now() - manager.task_timeout - Duration::from_secs(1);
    let session = &mut manager.sessions.get_mut(&path).expect("session").session;
    session.store_received_block(9, block_with_index_and_nonce(9, 1));
    session.register_index_task(9);
    assert!(manager.increment_index_task(9));

    manager.prune_timeouts();

    let session = &manager.sessions.get(&path).expect("session").session;
    assert!(!session.received_block.contains_key(&9));
    assert!(session.index_tasks.contains_key(&9));
    assert!(manager.global_index_tasks.is_tracked(&9));
    assert!(
        rx.try_recv().is_ok(),
        "frontier retry must bypass generic scheduling"
    );
}

#[test]
fn saturated_frontier_retry_does_not_evict_other_peer_caches() {
    let mut manager = TaskManager::new();
    manager.last_seen_persisted_index = 8;
    manager.last_persisted_at =
        std::time::Instant::now() - manager.task_timeout - Duration::from_secs(1);
    for _ in 0..MAX_CONCURRENT_TASKS {
        assert!(manager.increment_index_task(9));
    }

    let mut sessions = Vec::new();
    for number in 0..4 {
        let (mailbox, _rx) = mpsc::channel(8);
        let actor = ActorRef::new(
            ActorPath::root("test", format!("peer-frontier-cache-{number}")),
            mailbox,
            Weak::new(),
        );
        let path = insert_session(&mut manager, &actor);
        let session = &mut manager.sessions.get_mut(&path).expect("session").session;
        session.store_received_block(9, block_with_index_and_nonce(9, number));
        session.store_received_block(10, block_with_index_and_nonce(10, number));
        sessions.push(path);
    }

    manager.prune_timeouts();

    for path in sessions {
        let session = &manager.sessions.get(&path).expect("session").session;
        assert!(session.received_block.contains_key(&9));
        assert!(session.received_block.contains_key(&10));
    }
}

#[tokio::test]
async fn completed_downloads_count_toward_peer_pipeline_capacity() {
    let neo_system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut manager = TaskManager::new();
    manager.system = Some(neo_system.context());
    let (mailbox, _rx) = mpsc::channel(8);
    let actor = ActorRef::new(
        ActorPath::root("test", "peer-pipeline-capacity"),
        mailbox,
        Weak::new(),
    );
    let path = insert_session(&mut manager, &actor);
    manager
        .sessions
        .get_mut(&path)
        .expect("session")
        .session
        .last_block_index = 1_000;

    manager.with_session_mut(&path, |entry, this| {
        this.request_tasks_entry(entry);
    });
    assert_eq!(
        manager
            .sessions
            .get(&path)
            .expect("session")
            .session
            .index_tasks
            .len(),
        BLOCK_INDEX_REQUEST_BATCH as usize
    );

    manager.with_session_mut(&path, |entry, this| {
        assert!(entry.session.complete_index_task(1));
        this.decrement_index_task(1);
        entry
            .session
            .store_received_block(1, block_with_index_and_nonce(1, 1));
        this.request_tasks_entry(entry);
    });

    let session = &manager.sessions.get(&path).expect("session").session;
    assert_eq!(session.index_tasks.len(), 99);
    assert_eq!(session.received_block.len(), 1);
    neo_system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn timed_out_block_peer_does_not_reclaim_batch_before_other_peers() {
    let neo_system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut manager = TaskManager::new();
    manager.system = Some(neo_system.context());
    manager.last_seen_persisted_index = 8;
    manager.last_persisted_at = std::time::Instant::now();

    let (timed_out_mailbox, mut timed_out_rx) = mpsc::channel(32);
    let timed_out_actor = ActorRef::new(
        ActorPath::root("test", "peer-timed-out-batch"),
        timed_out_mailbox,
        Weak::new(),
    );
    let timed_out_path = insert_session(&mut manager, &timed_out_actor);
    let (healthy_mailbox, mut healthy_rx) = mpsc::channel(32);
    let healthy_actor = ActorRef::new(
        ActorPath::root("test", "peer-healthy-batch"),
        healthy_mailbox,
        Weak::new(),
    );
    let healthy_path = insert_session(&mut manager, &healthy_actor);

    for path in [&timed_out_path, &healthy_path] {
        manager
            .sessions
            .get_mut(path)
            .expect("session")
            .session
            .last_block_index = 20;
    }
    let timed_out_session = &mut manager
        .sessions
        .get_mut(&timed_out_path)
        .expect("timed out session")
        .session;
    timed_out_session.index_tasks.insert(
        9,
        std::time::Instant::now() - manager.task_timeout - Duration::from_secs(1),
    );
    assert!(manager.increment_index_task(9));

    manager.prune_timeouts();

    let timed_out_session = &manager
        .sessions
        .get(&timed_out_path)
        .expect("timed out session")
        .session;
    assert!(timed_out_session.index_tasks.is_empty());
    assert!(
        timed_out_rx.try_recv().is_err(),
        "the timed out peer must cool down for this scheduling round"
    );
    let healthy_session = &manager
        .sessions
        .get(&healthy_path)
        .expect("healthy session")
        .session;
    assert!(healthy_session.index_tasks.contains_key(&9));
    assert!(manager.global_index_tasks.is_tracked(&9));
    assert!(
        healthy_rx.try_recv().is_ok(),
        "a healthy peer must be allowed to take over the expired batch"
    );

    neo_system.shutdown().await.expect("shutdown");
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
    assert!(
        manager
            .sessions
            .get(&path)
            .expect("session")
            .session
            .received_block
            .is_empty()
    );
}
