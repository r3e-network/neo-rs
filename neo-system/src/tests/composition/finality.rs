use std::sync::Arc;

use neo_blockchain::{BlockPersistContext, FinalizedBlock};
use neo_payloads::{Block, Header};
use neo_storage::{DataCache, EmptyCacheBacking};

use super::{FinalizedBlockConsumer, FinalizedBlockStreamError, FinalizedBlockStreamFactory};

#[derive(Default)]
struct RecordingConsumer {
    heights: parking_lot::Mutex<Vec<u32>>,
    fail_at: Option<u32>,
}

impl FinalizedBlockConsumer<EmptyCacheBacking> for RecordingConsumer {
    fn consume(&self, finalized: &FinalizedBlock<EmptyCacheBacking>) -> Result<(), String> {
        let height = finalized.block().index();
        self.heights.lock().push(height);
        if self.fail_at == Some(height) {
            Err(format!("injected failure at {height}"))
        } else {
            Ok(())
        }
    }
}

fn finalized(height: u32) -> FinalizedBlock<EmptyCacheBacking> {
    let mut header = Header::new();
    header.set_index(height);
    FinalizedBlock::new(
        Arc::new(Block::from_parts(header, Vec::new())),
        Some(Arc::new(DataCache::new(false))),
        Vec::new(),
        BlockPersistContext::live(),
    )
}

#[tokio::test]
async fn publication_waits_for_consumer_acknowledgement() {
    let consumer = Arc::new(RecordingConsumer::default());
    let (handle, stream) = FinalizedBlockStreamFactory::new(1)
        .for_backing::<EmptyCacheBacking>()
        .create(Arc::clone(&consumer));
    let publisher = {
        let handle = handle.clone();
        tokio::spawn(async move { handle.publish(finalized(7)).await })
    };

    tokio::task::yield_now().await;
    assert!(!publisher.is_finished());

    let worker = tokio::spawn(stream.run());
    publisher
        .await
        .expect("publisher task")
        .expect("consumer acknowledgement");
    assert_eq!(consumer.heights.lock().as_slice(), &[7]);

    drop(handle);
    worker.await.expect("worker task").expect("clean close");
}

#[tokio::test]
async fn consumer_failure_reaches_publisher_and_stops_stream() {
    let consumer = Arc::new(RecordingConsumer {
        heights: parking_lot::Mutex::new(Vec::new()),
        fail_at: Some(9),
    });
    let (handle, stream) = FinalizedBlockStreamFactory::default()
        .for_backing::<EmptyCacheBacking>()
        .create(consumer);
    let worker = tokio::spawn(stream.run());

    assert_eq!(
        handle.publish(finalized(9)).await,
        Err(FinalizedBlockStreamError::Consumer(
            "injected failure at 9".to_string()
        ))
    );
    assert_eq!(
        worker.await.expect("worker task"),
        Err(FinalizedBlockStreamError::Consumer(
            "injected failure at 9".to_string()
        ))
    );
}

#[tokio::test]
async fn closed_stream_rejects_publication_without_hanging() {
    let consumer = Arc::new(RecordingConsumer::default());
    let (handle, stream) = FinalizedBlockStreamFactory::default()
        .for_backing::<EmptyCacheBacking>()
        .create(consumer);
    drop(stream);

    assert_eq!(
        handle.publish(finalized(1)).await,
        Err(FinalizedBlockStreamError::Closed)
    );
}
