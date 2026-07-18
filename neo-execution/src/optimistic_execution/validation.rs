use super::TransactionDependencies;
use neo_storage::{StorageItem, StorageKey};
use std::convert::Infallible;

/// Exact reason that a pinned point-read dependency no longer matches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointReadConflictKind {
    /// A previously present key still exists but its serialized value changed.
    PresentValueChanged,
    /// A previously present key was deleted.
    PresentDeleted,
    /// A previously absent key was created.
    AbsentCreated,
}

/// First deterministic point-read conflict found during ordered revalidation.
///
/// The diagnostic retains one capture-bounded key and a fixed-size conflict
/// classification. Expected and current values are deliberately not copied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointReadConflict {
    dependency_index: usize,
    key: StorageKey,
    kind: PointReadConflictKind,
}

impl PointReadConflict {
    /// Zero-based position in canonical point-dependency order.
    #[must_use]
    pub const fn dependency_index(&self) -> usize {
        self.dependency_index
    }

    /// Exact storage key that no longer matches.
    #[must_use]
    pub const fn key(&self) -> &StorageKey {
        &self.key
    }

    /// Present/absent transition or exact value-change classification.
    #[must_use]
    pub const fn kind(&self) -> PointReadConflictKind {
        self.kind
    }
}

/// Result of validating all point reads in canonical key order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PointReadValidation {
    /// Every captured present and absent value still matches exactly.
    Valid {
        /// Number of point reads checked.
        checked_point_reads: usize,
    },
    /// The first mismatching dependency; later keys were not read.
    Conflict(PointReadConflict),
}

impl PointReadValidation {
    /// Returns whether every point dependency remained valid.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }

    /// Returns the first conflict, if any.
    #[must_use]
    pub const fn conflict(&self) -> Option<&PointReadConflict> {
        match self {
            Self::Valid { .. } => None,
            Self::Conflict(conflict) => Some(conflict),
        }
    }

    /// Number of point lookups performed, including a conflicting lookup.
    #[must_use]
    pub const fn checked_point_reads(&self) -> usize {
        match self {
            Self::Valid {
                checked_point_reads,
            } => *checked_point_reads,
            Self::Conflict(conflict) => conflict.dependency_index + 1,
        }
    }
}

impl TransactionDependencies {
    /// Revalidates point reads through a caller-owned canonical lookup.
    ///
    /// The caller should invoke this only after speculative execution has
    /// completed and should supply a point-in-time view of the current ordered
    /// prefix. This API neither pauses nor invokes the capture observer.
    #[must_use]
    pub fn revalidate_point_reads(
        &self,
        mut lookup: impl FnMut(&StorageKey) -> Option<StorageItem>,
    ) -> PointReadValidation {
        match self.try_revalidate_point_reads(|key| Ok::<_, Infallible>(lookup(key))) {
            Ok(result) => result,
            Err(never) => match never {},
        }
    }

    /// Fallible form of [`Self::revalidate_point_reads`].
    ///
    /// Lookup errors stop validation immediately. Callers must treat an error
    /// as unproven and execute the transaction sequentially.
    pub fn try_revalidate_point_reads<E>(
        &self,
        mut lookup: impl FnMut(&StorageKey) -> Result<Option<StorageItem>, E>,
    ) -> Result<PointReadValidation, E> {
        for (dependency_index, dependency) in self.point_reads().iter().enumerate() {
            let actual = lookup(dependency.key())?;
            let kind = classify_conflict(dependency.value(), actual.as_ref());
            if let Some(kind) = kind {
                return Ok(PointReadValidation::Conflict(PointReadConflict {
                    dependency_index,
                    key: dependency.key().clone(),
                    kind,
                }));
            }
        }
        Ok(PointReadValidation::Valid {
            checked_point_reads: self.point_reads().len(),
        })
    }
}

fn classify_conflict(
    expected: Option<&StorageItem>,
    actual: Option<&StorageItem>,
) -> Option<PointReadConflictKind> {
    match (expected, actual) {
        (Some(expected), Some(actual)) if expected != actual => {
            Some(PointReadConflictKind::PresentValueChanged)
        }
        (Some(_), None) => Some(PointReadConflictKind::PresentDeleted),
        (None, Some(_)) => Some(PointReadConflictKind::AbsentCreated),
        (Some(_), Some(_)) | (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimistic_execution::{
        BlockPrefixIdentity, DependencyCaptureError, DependencyCaptureLimits, PinnedBlockPrefix,
    };
    use neo_primitives::UInt256;
    use neo_storage::DataCache;
    use std::collections::BTreeMap;

    fn key(suffix: &[u8]) -> StorageKey {
        StorageKey::new(7, suffix.to_vec())
    }

    fn item(value: &[u8]) -> StorageItem {
        StorageItem::from_bytes(value.to_vec())
    }

    fn capture(
        present: &[(StorageKey, StorageItem)],
        reads: &[StorageKey],
        limits: DependencyCaptureLimits,
    ) -> Result<TransactionDependencies, DependencyCaptureError> {
        let canonical = DataCache::new(false);
        for (key, value) in present {
            canonical.add(key.clone(), value.clone());
        }
        let prefix = PinnedBlockPrefix::capture(
            BlockPrefixIdentity::new(UInt256::default(), 42, 0),
            &canonical,
        );
        let (overlay, capture) = prefix
            .transaction_overlay_with_dependency_capture(0, limits)
            .expect("tracked overlay");
        for key in reads {
            overlay.snapshot_cache().get(key);
        }
        capture.snapshot()
    }

    #[test]
    fn unchanged_present_and_absent_dependencies_validate() {
        let present = key(b"present");
        let absent = key(b"absent");
        let expected = item(b"same");
        let dependencies = capture(
            &[(present.clone(), expected.clone())],
            &[present.clone(), absent],
            DependencyCaptureLimits::default(),
        )
        .expect("dependencies");
        let current = BTreeMap::from([(present, expected)]);

        let result = dependencies.revalidate_point_reads(|key| current.get(key).cloned());

        assert_eq!(
            result,
            PointReadValidation::Valid {
                checked_point_reads: 2,
            }
        );
        assert!(result.is_valid());
        assert_eq!(result.checked_point_reads(), 2);
        assert_eq!(result.conflict(), None);
    }

    #[test]
    fn changed_present_value_conflicts() {
        let key = key(b"key");
        let dependencies = capture(
            &[(key.clone(), item(b"before"))],
            std::slice::from_ref(&key),
            DependencyCaptureLimits::default(),
        )
        .expect("dependencies");
        let current = BTreeMap::from([(key.clone(), item(b"after"))]);

        let result = dependencies.revalidate_point_reads(|key| current.get(key).cloned());

        assert_eq!(
            result,
            PointReadValidation::Conflict(PointReadConflict {
                dependency_index: 0,
                key,
                kind: PointReadConflictKind::PresentValueChanged,
            })
        );
    }

    #[test]
    fn deleted_present_value_conflicts() {
        let key = key(b"key");
        let dependencies = capture(
            &[(key.clone(), item(b"before"))],
            std::slice::from_ref(&key),
            DependencyCaptureLimits::default(),
        )
        .expect("dependencies");

        let result = dependencies.revalidate_point_reads(|_| None);

        let conflict = result.conflict().expect("deleted conflict");
        assert_eq!(conflict.dependency_index(), 0);
        assert_eq!(conflict.key(), &key);
        assert_eq!(conflict.kind(), PointReadConflictKind::PresentDeleted);
        assert_eq!(result.checked_point_reads(), 1);
    }

    #[test]
    fn created_absent_value_conflicts() {
        let key = key(b"key");
        let dependencies = capture(
            &[],
            std::slice::from_ref(&key),
            DependencyCaptureLimits::default(),
        )
        .expect("dependencies");

        let result = dependencies.revalidate_point_reads(|_| Some(item(b"created")));

        assert_eq!(
            result.conflict().map(PointReadConflict::kind),
            Some(PointReadConflictKind::AbsentCreated)
        );
    }

    #[test]
    fn first_conflict_is_canonical_and_stops_later_lookups() {
        let earlier = key(b"a");
        let later = key(b"z");
        let dependencies = capture(
            &[],
            &[later.clone(), earlier.clone()],
            DependencyCaptureLimits::default(),
        )
        .expect("dependencies");
        let mut visited = Vec::new();

        let result = dependencies.revalidate_point_reads(|key| {
            visited.push(key.clone());
            Some(item(b"created"))
        });

        assert_eq!(visited, vec![earlier.clone()]);
        assert_eq!(
            result.conflict().map(PointReadConflict::key),
            Some(&earlier)
        );
        assert_eq!(result.checked_point_reads(), 1);
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct LookupFailure;

    #[test]
    fn lookup_failure_is_unproven_and_stops_validation() {
        let first = key(b"a");
        let second = key(b"z");
        let dependencies = capture(&[], &[second, first], DependencyCaptureLimits::default())
            .expect("dependencies");
        let mut calls = 0;

        let result = dependencies.try_revalidate_point_reads(|_| {
            calls += 1;
            Err::<Option<StorageItem>, _>(LookupFailure)
        });

        assert_eq!(result, Err(LookupFailure));
        assert_eq!(calls, 1);
    }

    #[test]
    fn capture_limit_failure_produces_no_validatable_dependency_set() {
        let key = key(b"key");
        let result = capture(
            &[(key.clone(), item(b"value"))],
            &[key],
            DependencyCaptureLimits::new(0, usize::MAX),
        );

        assert_eq!(
            result,
            Err(DependencyCaptureError::PointReadLimitExceeded { limit: 0 })
        );
    }
}
