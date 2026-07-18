use super::*;
use crate::DataCacheReadObserver;
use crate::persistence::data_cache::DataCacheReadOrigin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReadEvent {
    Point {
        key: StorageKey,
        value: Option<Vec<u8>>,
        origin: DataCacheReadOrigin,
    },
    Range {
        prefix: Option<StorageKey>,
        direction: SeekDirection,
        rows: Vec<(StorageKey, Vec<u8>)>,
    },
}

#[derive(Default)]
struct RecordingObserver {
    events: Mutex<Vec<ReadEvent>>,
}

impl RecordingObserver {
    fn events(&self) -> Vec<ReadEvent> {
        self.events.lock().expect("observer lock").clone()
    }

    fn clear(&self) {
        self.events.lock().expect("observer lock").clear();
    }
}

impl DataCacheReadObserver for RecordingObserver {
    fn observe_point_read(&self, _key: &StorageKey, _value: Option<&StorageItem>) {
        panic!("DataCache must invoke the origin-aware point-read hook");
    }

    fn observe_point_read_with_origin(
        &self,
        key: &StorageKey,
        value: Option<&StorageItem>,
        origin: DataCacheReadOrigin,
    ) {
        self.events
            .lock()
            .expect("observer lock")
            .push(ReadEvent::Point {
                key: key.clone(),
                value: value.map(|item| item.value_bytes().into_owned()),
                origin,
            });
    }

    fn observe_range_read(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
        rows: &[(StorageKey, StorageItem)],
    ) {
        self.events
            .lock()
            .expect("observer lock")
            .push(ReadEvent::Range {
                prefix: prefix.cloned(),
                direction,
                rows: rows
                    .iter()
                    .map(|(key, item)| (key.clone(), item.value_bytes().into_owned()))
                    .collect(),
            });
    }
}

fn observed_child(base: &DataCache) -> (DataCache, Arc<RecordingObserver>) {
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    (
        base.clone_cache().with_read_observer(observer_binding),
        observer,
    )
}

#[test]
fn point_observer_receives_final_present_and_absent_values() {
    let key = StorageKey::new(7, vec![0x01]);
    let missing = StorageKey::new(7, vec![0x02]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let (cache, observer) = observed_child(&base);

    assert_eq!(
        cache.get(&key).map(|item| item.to_value()),
        Some(vec![0xAA])
    );
    assert!(cache.get(&missing).is_none());
    assert_eq!(
        cache.get_ref(&key).map(|item| item.to_value()),
        Some(vec![0xAA])
    );

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Point {
                key: key.clone(),
                value: Some(vec![0xAA]),
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
            ReadEvent::Point {
                key: missing,
                value: None,
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
            ReadEvent::Point {
                key,
                value: Some(vec![0xAA]),
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
        ]
    );
}

#[test]
fn range_observer_receives_materialized_prefix_and_whole_store_rows() {
    let prefix = StorageKey::new(7, vec![0x21]);
    let first = StorageKey::new(7, vec![0x21, 0x01]);
    let second = StorageKey::new(7, vec![0x21, 0x02]);
    let other = StorageKey::new(8, vec![0x01]);
    let base = DataCache::new(false);
    for (key, value) in [
        (first.clone(), vec![0xA1]),
        (second.clone(), vec![0xA2]),
        (other.clone(), vec![0xB1]),
    ] {
        base.add(key, StorageItem::from_bytes(value));
    }
    let (cache, observer) = observed_child(&base);

    let prefixed = cache
        .find(Some(&prefix), SeekDirection::Backward)
        .collect::<Vec<_>>();
    assert_eq!(prefixed.len(), 2);
    let all = cache.find(None, SeekDirection::Forward).collect::<Vec<_>>();
    assert_eq!(all.len(), 3);

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Range {
                prefix: Some(prefix),
                direction: SeekDirection::Backward,
                rows: vec![(second, vec![0xA2]), (first, vec![0xA1])],
            },
            ReadEvent::Range {
                prefix: None,
                direction: SeekDirection::Forward,
                rows: all
                    .into_iter()
                    .map(|(key, item)| (key, item.value_bytes().into_owned()))
                    .collect(),
            },
        ]
    );
}

#[test]
fn blind_delete_observes_its_backing_lookup_once() {
    let key = StorageKey::new(7, vec![0x01]);
    let missing = StorageKey::new(7, vec![0x02]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let (cache, observer) = observed_child(&base);

    cache.delete(&key);
    cache.delete(&missing);

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Point {
                key,
                value: Some(vec![0xAA]),
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
            ReadEvent::Point {
                key: missing,
                value: None,
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
        ]
    );
}

#[test]
fn detached_backing_reads_are_pinned_for_present_and_absent_keys() {
    let present = StorageKey::new(7, vec![0x31]);
    let absent = StorageKey::new(7, vec![0x32]);
    let base = DataCache::new(false);
    base.add(present.clone(), StorageItem::from_bytes(vec![0xA1]));
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let transaction = base
        .clone_detached_cache()
        .with_read_observer(observer_binding);

    assert_eq!(
        transaction.get(&present).map(|item| item.to_value()),
        Some(vec![0xA1])
    );
    assert!(transaction.get(&absent).is_none());

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Point {
                key: present,
                value: Some(vec![0xA1]),
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
            ReadEvent::Point {
                key: absent,
                value: None,
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
        ]
    );
}

#[test]
fn detached_own_add_update_and_delete_reads_are_overlay_origin() {
    let added = StorageKey::new(7, vec![0x41]);
    let updated = StorageKey::new(7, vec![0x42]);
    let deleted = StorageKey::new(7, vec![0x43]);
    let base = DataCache::new(false);
    base.add(updated.clone(), StorageItem::from_bytes(vec![0xA2]));
    base.add(deleted.clone(), StorageItem::from_bytes(vec![0xA3]));
    let transaction = base.clone_detached_cache();
    transaction.add(added.clone(), StorageItem::from_bytes(vec![0xB1]));
    transaction.update(updated.clone(), StorageItem::from_bytes(vec![0xB2]));
    transaction.delete(&deleted);
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let transaction = transaction.with_read_observer(observer_binding);

    assert!(transaction.get(&added).is_some());
    assert!(transaction.get(&updated).is_some());
    assert!(transaction.get(&deleted).is_none());

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Point {
                key: added,
                value: Some(vec![0xB1]),
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: updated,
                value: Some(vec![0xB2]),
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: deleted,
                value: None,
                origin: DataCacheReadOrigin::Overlay,
            },
        ]
    );
}

#[test]
fn nested_child_preserves_parent_transaction_write_origin_after_read_caching() {
    let added = StorageKey::new(7, vec![0x51]);
    let updated = StorageKey::new(7, vec![0x52]);
    let deleted = StorageKey::new(7, vec![0x53]);
    let base = DataCache::new(false);
    base.add(updated.clone(), StorageItem::from_bytes(vec![0xA2]));
    base.add(deleted.clone(), StorageItem::from_bytes(vec![0xA3]));
    let transaction = base.clone_detached_cache();
    transaction.add(added.clone(), StorageItem::from_bytes(vec![0xC1]));
    transaction.update(updated.clone(), StorageItem::from_bytes(vec![0xC2]));
    transaction.delete(&deleted);
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let transaction = transaction.with_read_observer(observer_binding);
    let child = transaction.clone_cache();

    for key in [&added, &updated, &deleted] {
        let first = child.get(key);
        let second = child.get(key);
        assert_eq!(first, second);
    }

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Point {
                key: added.clone(),
                value: Some(vec![0xC1]),
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: added,
                value: Some(vec![0xC1]),
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: updated.clone(),
                value: Some(vec![0xC2]),
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: updated,
                value: Some(vec![0xC2]),
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: deleted.clone(),
                value: None,
                origin: DataCacheReadOrigin::Overlay,
            },
            ReadEvent::Point {
                key: deleted,
                value: None,
                origin: DataCacheReadOrigin::Overlay,
            },
        ]
    );
}

#[test]
fn detached_blind_delete_observes_the_pinned_prefix_once() {
    let present = StorageKey::new(7, vec![0x61]);
    let absent = StorageKey::new(7, vec![0x62]);
    let base = DataCache::new(false);
    base.add(present.clone(), StorageItem::from_bytes(vec![0xD1]));
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let transaction = base
        .clone_detached_cache()
        .with_read_observer(observer_binding);

    transaction.delete(&present);
    transaction.delete(&absent);

    assert_eq!(
        observer.events(),
        vec![
            ReadEvent::Point {
                key: present,
                value: Some(vec![0xD1]),
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
            ReadEvent::Point {
                key: absent,
                value: None,
                origin: DataCacheReadOrigin::PinnedPrefix,
            },
        ]
    );
}

#[test]
fn child_and_isolated_caches_inherit_without_recursive_duplicates() {
    let key = StorageKey::new(7, vec![0x01]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let (parent, observer) = observed_child(&base);
    let child = parent.clone_cache();
    let isolated = parent.fork_isolated();

    assert!(parent.has_read_observer());
    assert!(child.has_read_observer());
    assert!(isolated.has_read_observer());
    assert!(child.get(&key).is_some());
    assert!(isolated.get(&key).is_some());

    assert_eq!(observer.events().len(), 2);
}

#[test]
fn layered_delete_merge_observes_its_backing_lookup_once() {
    let key = StorageKey::new(7, vec![0x01]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let config = DataCacheConfig {
        track_reads_in_write_cache: false,
        ..DataCacheConfig::default()
    };
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let parent = base
        .clone_cache_with_config(config)
        .with_read_observer(observer_binding);
    let child = parent.clone_cache_with_config(config);

    child.delete(&key);
    observer.clear();
    child.commit();

    assert_eq!(
        observer.events(),
        vec![ReadEvent::Point {
            key,
            value: Some(vec![0xAA]),
            origin: DataCacheReadOrigin::PinnedPrefix,
        }]
    );
}

#[test]
fn committing_child_routes_merge_reads_to_its_own_observer() {
    let key = StorageKey::new(7, vec![0x01]);
    let config = DataCacheConfig {
        track_reads_in_write_cache: false,
        ..DataCacheConfig::default()
    };
    let base = DataCache::with_config(false, config);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let parent = base.clone_cache_with_config(config);
    let observer = Arc::new(RecordingObserver::default());
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let child = parent
        .clone_cache_with_config(config)
        .with_read_observer(observer_binding);

    child.delete(&key);
    observer.clear();
    child.commit();

    assert_eq!(
        observer.events(),
        vec![ReadEvent::Point {
            key,
            value: Some(vec![0xAA]),
            origin: DataCacheReadOrigin::PinnedPrefix,
        }]
    );
}

struct BlockingObserver {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    calls: AtomicUsize,
}

impl DataCacheReadObserver for BlockingObserver {
    fn observe_point_read(&self, _key: &StorageKey, _value: Option<&StorageItem>) {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.send(()).expect("callback entry receiver");
            self.release
                .lock()
                .expect("callback release lock")
                .recv()
                .expect("callback release signal");
        }
    }

    fn observe_range_read(
        &self,
        _prefix: Option<&StorageKey>,
        _direction: SeekDirection,
        _rows: &[(StorageKey, StorageItem)],
    ) {
    }
}

#[test]
fn pause_waits_for_in_flight_callback_and_blocks_late_appends() {
    let key = StorageKey::new(7, vec![0x01]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let observer = Arc::new(BlockingObserver {
        entered: entered_tx,
        release: Mutex::new(release_rx),
        calls: AtomicUsize::new(0),
    });
    let observer_binding: Arc<dyn DataCacheReadObserver> = observer.clone();
    let cache = Arc::new(base.clone_cache().with_read_observer(observer_binding));

    let reader_cache = Arc::clone(&cache);
    let reader_key = key.clone();
    let reader = std::thread::spawn(move || reader_cache.get(&reader_key));
    entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first callback enters");

    let pause_cache = Arc::clone(&cache);
    let (pause_started_tx, pause_started_rx) = mpsc::channel();
    let (pause_tx, pause_rx) = mpsc::channel();
    let pause_thread = std::thread::spawn(move || {
        pause_started_tx.send(()).expect("pause start receiver");
        let pause = pause_cache.pause_read_observation();
        pause_tx.send(pause).expect("pause receiver");
    });
    pause_started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("pause thread starts");
    assert!(pause_rx.recv_timeout(Duration::from_millis(100)).is_err());

    release_tx.send(()).expect("release first callback");
    let pause = pause_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("pause waits for callback quiescence");
    assert!(reader.join().expect("reader thread").is_some());
    assert_eq!(observer.calls.load(Ordering::SeqCst), 1);

    assert!(cache.get(&key).is_some());
    assert_eq!(observer.calls.load(Ordering::SeqCst), 1);
    drop(pause);
    assert!(cache.get(&key).is_some());
    assert_eq!(observer.calls.load(Ordering::SeqCst), 2);
    pause_thread.join().expect("pause thread");
}

#[test]
fn pause_and_disable_are_shared_without_changing_read_results() {
    let key = StorageKey::new(7, vec![0x01]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let (parent, observer) = observed_child(&base);
    let child = parent.clone_cache();

    {
        let _pause = parent.pause_read_observation();
        assert!(child.get(&key).is_some());
    }
    assert!(observer.events().is_empty());
    assert!(child.get(&key).is_some());
    assert_eq!(observer.events().len(), 1);

    parent.disable_read_observation();
    assert!(child.get(&key).is_some());
    assert_eq!(observer.events().len(), 1);
}

#[test]
fn ordinary_cache_has_no_observer_and_preserves_results() {
    let key = StorageKey::new(7, vec![0x01]);
    let missing = StorageKey::new(7, vec![0x02]);
    let base = DataCache::new(false);
    base.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    let child = base.clone_cache();

    assert!(!base.has_read_observer());
    assert!(!child.has_read_observer());
    assert_eq!(
        child.get(&key).map(|item| item.to_value()),
        Some(vec![0xAA])
    );
    assert!(child.get(&missing).is_none());
    child.delete(&key);
    assert!(child.get(&key).is_none());
}
