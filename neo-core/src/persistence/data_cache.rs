use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use crate::neo_contract::storage_item::StorageItem;
use crate::neo_contract::storage_key::StorageKey;
use crate::persistence::persistence_error::PersistenceError;
use crate::persistence::SeekDirection;

pub trait DataCache {
    fn new() -> Self where Self: Sized;

    fn get(&self, key: &StorageKey) -> Result<StorageItem, PersistenceError> {
        let mut dict = self.get_dictionary();
        if let Some(trackable) = dict.get(key) {
            match trackable.state {
                TrackState::Deleted | TrackState::NotFound => Err(PersistenceError::KeyNotFound),
                _ => Ok(trackable.item.clone().unwrap()),
            }
        } else {
            let item = self.get_internal(key)?;
            dict.insert(key.clone(), Trackable {
                key: key.clone(),
                item: Some(item.clone()),
                state: TrackState::None,
            });
            Ok(item)
        }
    }

    fn add(&self, key: StorageKey, value: StorageItem) -> Result<(), PersistenceError> {
        let mut dict = self.get_dictionary();
        let mut change_set = self.get_change_set();

        if let Some(trackable) = dict.get_mut(&key) {
            trackable.item = Some(value);
            trackable.state = match trackable.state {
                TrackState::Deleted => TrackState::Changed,
                TrackState::NotFound => TrackState::Added,
                _ => return Err(PersistenceError::InternalError("The element currently has an invalid state".to_string())),
            };
        } else {
            dict.insert(key.clone(), Trackable {
                key: key.clone(),
                item: Some(value),
                state: TrackState::Added,
            });
        }
        change_set.insert(key);
        Ok(())
    }

    fn commit(&self) -> Result<(), PersistenceError> {
        let mut dict = self.get_dictionary();
        let mut change_set = self.get_change_set();
        let mut deleted_items = Vec::new();

        for trackable in self.get_change_set_iter() {
            match trackable.state {
                TrackState::Added => self.add_internal(&trackable.key, &trackable.item)?,
                TrackState::Changed => self.update_internal(&trackable.key, &trackable.item)?,
                TrackState::Deleted => {
                    self.delete_internal(&trackable.key)?;
                    deleted_items.push(trackable.key.clone());
                },
                _ => {}
            }
        }

        for key in deleted_items {
            dict.remove(&key);
        }
        change_set.clear();
        Ok(())
    }

    fn delete(&self, key: &StorageKey) -> Result<(), PersistenceError> {
        let mut dict = self.get_dictionary();
        let mut change_set = self.get_change_set();

        if let Some(trackable) = dict.get_mut(key) {
            match trackable.state {
                TrackState::Added => {
                    trackable.state = TrackState::NotFound;
                    change_set.remove(key);
                },
                TrackState::NotFound => {},
                _ => {
                    trackable.state = TrackState::Deleted;
                    change_set.insert(key.clone());
                },
            }
        } else {
            if let Some(item) = self.try_get_internal(key)? {
                dict.insert(key.clone(), Trackable {
                    key: key.clone(),
                    item: Some(item),
                    state: TrackState::Deleted,
                });
                change_set.insert(key.clone());
            }
        }
        Ok(())
    }

    fn find(&self, key_prefix: Option<&[u8]>, direction: SeekDirection)
            -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let seek_prefix = match (key_prefix, direction) {
            (Some(prefix), SeekDirection::Backward) if !prefix.is_empty() => {
                let mut seek_prefix = prefix.to_vec();
                for i in (0..seek_prefix.len()).rev() {
                    if seek_prefix[i] < 0xff {
                        seek_prefix[i] += 1;
                        seek_prefix.truncate(i + 1);
                        break;
                    }
                }
                Some(seek_prefix)
            },
            _ => key_prefix.map(|p| p.to_vec()),
        };

        Box::new(self.find_internal(key_prefix.map(|p| p.to_vec()), seek_prefix, direction))
    }

    fn find_range(&self, start: &[u8], end: &[u8], direction: SeekDirection)
                  -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let comparer = match direction {
            SeekDirection::Forward => ByteArrayComparer::Default,
            SeekDirection::Backward => ByteArrayComparer::Reverse,
        };

        Box::new(self.seek(Some(start), direction)
            .take_while(move |(key, _)| comparer.compare(key.to_array(), end) < Ordering::Equal))
    }

    fn contains(&self, key: &StorageKey) -> bool {
        let dict = self.get_dictionary();
        match dict.get(key) {
            Some(trackable) => trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound,
            None => self.contains_internal(key),
        }
    }

    fn get_and_change(&self, key: &StorageKey, factory: Option<Box<dyn Fn() -> StorageItem>>)
                      -> Result<Option<StorageItem>, PersistenceError> {
        let mut dict = self.get_dictionary();
        let mut change_set = self.get_change_set();

        if let Some(trackable) = dict.get_mut(key) {
            match trackable.state {
                TrackState::Deleted | TrackState::NotFound => {
                    if let Some(f) = factory {
                        trackable.item = Some(f());
                        trackable.state = if trackable.state == TrackState::Deleted {
                            TrackState::Changed
                        } else {
                            TrackState::Added
                        };
                        change_set.insert(key.clone());
                        Ok(Some(trackable.item.clone()))
                    } else {
                        Ok(None)
                    }
                },
                TrackState::None => {
                    trackable.state = TrackState::Changed;
                    change_set.insert(key.clone());
                    Ok(Some(trackable.item.clone()))
                },
                _ => Ok(Some(trackable.item.clone())),
            }
        } else {
            let mut trackable = Trackable {
                key: key.clone(),
                item: self.try_get_internal(key)?,
                state: TrackState::None,
            };

            if trackable.item.is_none() {
                if let Some(f) = factory {
                    trackable.item = Some(f());
                    trackable.state = TrackState::Added;
                } else {
                    return Ok(None);
                }
            } else {
                trackable.state = TrackState::Changed;
            }

            let item = trackable.item.clone();
            dict.insert(key.clone(), trackable);
            change_set.insert(key.clone());
            Ok(item)
        }
    }

    fn get_or_add(&self, key: &StorageKey, factory: Box<dyn Fn() -> StorageItem>)
                  -> Result<StorageItem, &'static str> {
        let mut dict = self.get_dictionary();
        let mut change_set = self.get_change_set();

        if let Some(trackable) = dict.get_mut(key) {
            match trackable.state {
                TrackState::Deleted | TrackState::NotFound => {
                    trackable.item = Some(factory());
                    trackable.state = if trackable.state == TrackState::Deleted {
                        TrackState::Changed
                    } else {
                        TrackState::Added
                    };
                    change_set.insert(key.clone());
                },
                _ => {},
            }
            Ok(trackable.item.clone().unwrap())
        } else {
            let mut trackable = Trackable {
                key: key.clone(),
                item: self.try_get_internal(key)?,
                state: TrackState::None,
            };

            if trackable.item.is_none() {
                trackable.item = Some(factory());
                trackable.state = TrackState::Added;
                change_set.insert(key.clone());
            }

            let item = trackable.item.clone().unwrap();
            dict.insert(key.clone(), trackable);
            Ok(item)
        }
    }

    fn try_get(&self, key: &StorageKey) -> Result<Option<StorageItem>, PersistenceError> {
        let mut dict = self.get_dictionary();

        if let Some(trackable) = dict.get(key) {
            match trackable.state {
                TrackState::Deleted | TrackState::NotFound => Ok(None),
                _ => Ok(Some(trackable.item.clone())),
            }
        } else {
            let value = self.try_get_internal(key)?;
            if let Some(item) = &value {
                dict.insert(key.clone(), Trackable {
                    key: key.clone(),
                    item: Some(item.clone()),
                    state: TrackState::None,
                });
            }
            Ok(value)
        }
    }

    // Internal methods that need to be implemented by the concrete type
    fn get_internal(&self, key: &StorageKey) -> Result<StorageItem, PersistenceError>;
    fn add_internal(&self, key: &StorageKey, value: &StorageItem) -> Result<(), PersistenceError>;
    fn delete_internal(&self, key: &StorageKey) -> Result<(), PersistenceError>;
    fn contains_internal(&self, key: &StorageKey) -> bool;
    fn try_get_internal(&self, key: &StorageKey) -> Result<Option<StorageItem>, &'static str>;
    fn update_internal(&self, key: &StorageKey, value: &StorageItem) -> Result<(), PersistenceError>;
    fn seek_internal(&self, key_or_prefix: &[u8], direction: SeekDirection)
                     -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_>;

    // Helper methods for accessing internal state
    fn get_dictionary(&self) -> std::sync::MutexGuard<'_, HashMap<StorageKey, Trackable>>;
    fn get_change_set(&self) -> std::sync::MutexGuard<'_, HashSet<StorageKey>>;
    fn get_change_set_iter(&self) -> Box<dyn Iterator<Item = Trackable> + '_>;
}

pub struct Trackable {
    pub key: StorageKey,
    pub item: Option<StorageItem>,
    pub state: TrackState,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum TrackState {
    None,
    Added,
    Changed,
    Deleted,
    NotFound,
}

enum ByteArrayComparer {
    Default,
    Reverse,
}

impl ByteArrayComparer {
    fn compare(&self, a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        match self {
            ByteArrayComparer::Default => a.cmp(b),
            ByteArrayComparer::Reverse => b.cmp(a),
        }
    }
}