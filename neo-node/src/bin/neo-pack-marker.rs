//! Read-only inspection of the canonical authoritative state-pack marker.
//!
//! The tool reports the exact raw marker digest needed by guarded checkpoint
//! activation. It never opens MDBX for writing and never decodes an unknown
//! legacy marker as a current record.
//!
//! Usage: `neo-pack-marker --mdbx <canonical-store-dir>`

use std::path::PathBuf;

use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Crypto;
use neo_state_packs::authority::{AUTHORITATIVE_HIGH_WATER_KEY, AuthoritativeHighWaterRecord};
use neo_state_service::MDBX_STATE_SERVICE_NAMESPACE;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{StoreFactory, TransactionalStore};
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Arguments {
    mdbx_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct InspectionReport {
    schema_version: u32,
    mdbx_path: String,
    marker: MarkerInspection,
}

#[derive(Debug, Serialize)]
#[serde(tag = "format", rename_all = "kebab-case")]
enum MarkerInspection {
    Absent,
    Legacy {
        bytes: u64,
        sha256: String,
        current_decode_error: String,
    },
    Current {
        bytes: u64,
        sha256: String,
        network_magic: String,
        store_identity_sha256: String,
        epoch: u64,
        segment_id: u64,
        frame_end: u64,
        frame_sha256: String,
        frame_block_start: u32,
        frame_block_end: u32,
        block_index: u32,
        state_root: String,
        state_root_internal_bytes: String,
    },
}

fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    inspect(&arguments)
}

fn inspect(arguments: &Arguments) -> Result<()> {
    let canonical: std::sync::Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: arguments.mdbx_path.clone(),
            read_only: true,
            ..StorageConfig::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("open read-only MDBX store: {error}"))?;
    let state_store = canonical
        .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
        .context("open coordinated MDBX StateService namespace")?;
    let marker = match state_store
        .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
        .context("read authoritative marker")?
    {
        None => MarkerInspection::Absent,
        Some(bytes) => inspect_marker(bytes),
    };
    let report = InspectionReport {
        schema_version: 1,
        mdbx_path: arguments.mdbx_path.display().to_string(),
        marker,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn inspect_marker(bytes: Vec<u8>) -> MarkerInspection {
    let sha256 = formatted_hash(Crypto::sha256(&bytes));
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    match AuthoritativeHighWaterRecord::decode(&bytes) {
        Ok(marker) => MarkerInspection::Current {
            bytes: length,
            sha256,
            network_magic: format!("0x{:08X}", marker.network_magic),
            store_identity_sha256: formatted_hash(marker.store_identity),
            epoch: marker.epoch,
            segment_id: marker.segment_id.get(),
            frame_end: marker.frame_end,
            frame_sha256: formatted_hash(marker.frame_sha256),
            frame_block_start: marker.frame_context.block_start,
            frame_block_end: marker.frame_context.block_end,
            block_index: marker.block_index,
            state_root: displayed_root(marker.state_root),
            state_root_internal_bytes: formatted_hash(marker.state_root),
        },
        Err(error) => MarkerInspection::Legacy {
            bytes: length,
            sha256,
            current_decode_error: error.to_string(),
        },
    }
}

fn parse_arguments() -> Result<Arguments> {
    parse_arguments_from(std::env::args().skip(1))
}

fn parse_arguments_from(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut mdbx_path = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--mdbx" => {
                ensure!(mdbx_path.is_none(), "--mdbx may be specified only once");
                mdbx_path = Some(PathBuf::from(
                    arguments.next().context("--mdbx requires a value")?,
                ));
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(Arguments {
        mdbx_path: mdbx_path.context("--mdbx is required")?,
    })
}

fn formatted_hash(hash: [u8; 32]) -> String {
    format!("0x{}", hex::encode(hash))
}

fn displayed_root(mut internal: [u8; 32]) -> String {
    internal.reverse();
    formatted_hash(internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_requires_one_mdbx_path() {
        assert_eq!(
            parse_arguments_from(["--mdbx".to_owned(), "/ledger".to_owned()])
                .expect("parse marker inspector"),
            Arguments {
                mdbx_path: PathBuf::from("/ledger")
            }
        );
        assert!(parse_arguments_from(Vec::<String>::new()).is_err());
        assert!(
            parse_arguments_from([
                "--mdbx".to_owned(),
                "first".to_owned(),
                "--mdbx".to_owned(),
                "second".to_owned(),
            ])
            .is_err()
        );
    }

    #[test]
    fn unknown_bytes_are_reported_as_legacy_without_reinterpretation() {
        let bytes = b"legacy".to_vec();
        match inspect_marker(bytes.clone()) {
            MarkerInspection::Legacy {
                bytes: length,
                sha256,
                ..
            } => {
                assert_eq!(length, bytes.len() as u64);
                assert_eq!(sha256, formatted_hash(Crypto::sha256(&bytes)));
            }
            other => panic!("unexpected marker inspection: {other:?}"),
        }
    }
}
