use crate::execution_artifact::{
    ExecutionArtifactError, ExecutionArtifactLimits, ExecutionObservationJournal,
};
use crate::host_access_audit::StorageRangeAccess;
use neo_primitives::FindOptions;
use neo_storage::StorageKey;

#[test]
fn storage_range_rows_are_bounded_aggregately_before_retention() {
    let limits = ExecutionArtifactLimits {
        max_storage_range_rows: 1,
        ..ExecutionArtifactLimits::DEFAULT
    };
    let access = StorageRangeAccess::prefix(
        17,
        b"prefix".to_vec(),
        neo_vm::RangeDirection::Forward,
        FindOptions::None,
        2,
    );
    let mut journal = ExecutionObservationJournal::with_limits(limits);
    journal
        .record_storage_range(
            access.clone(),
            vec![(StorageKey::new(17, b"prefix-a".to_vec()), vec![1])],
        )
        .expect("first range row fits");
    assert_eq!(
        journal
            .record_storage_range(
                access,
                vec![(StorageKey::new(17, b"prefix-b".to_vec()), vec![2])],
            )
            .expect_err("aggregate range rows must be bounded"),
        ExecutionArtifactError::LimitExceeded {
            resource: "storage range rows",
            actual: 2,
            maximum: 1,
        }
    );
}
