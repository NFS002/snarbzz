//! The batch-consuming task for execution policy.
//!
//! Batches are processed in arrival order, including empty batches and same-height
//! reorg replacements. The handler owns the batch and never borrows manager state.
pub mod live;
pub mod policy;

use std::{future::Future, num::NonZeroUsize};

use amms::amms::path::SimulationBatch;
use anyhow::{Context, Result};
use tokio::{sync::mpsc, task::JoinHandle};

pub struct ExecutionEngine {
    batches: mpsc::Receiver<SimulationBatch>,
}

impl ExecutionEngine {
    /// Creates a bounded queue. Sending waits when full; affected-path batches
    /// must not be silently dropped because they contain different path updates.
    pub fn channel(capacity: NonZeroUsize) -> (mpsc::Sender<SimulationBatch>, Self) {
        let (sender, batches) = mpsc::channel(capacity.get());
        (sender, Self { batches })
    }

    /// Runs execution policy in a separate Tokio task.
    ///
    /// Dropping all senders drains the queue and stops the engine. A handler error
    /// stops the engine unless it is a recoverable BatchEvaluationError, which is
    /// logged before continuing with the next batch.
    /// The handler is responsible for fee estimates, optimisation, fresh-state
    /// validation, and submission. This task does not submit orders on its own.
    pub fn spawn<H, F>(self, handler: H) -> JoinHandle<Result<()>>
    where
        H: FnMut(SimulationBatch) -> F + Send + 'static,
        F: Future<Output = Result<()>> + Send + 'static,
    {
        tokio::spawn(self.run(handler))
    }

    pub async fn run<H, F>(mut self, mut handler: H) -> Result<()>
    where
        H: FnMut(SimulationBatch) -> F,
        F: Future<Output = Result<()>>,
    {
        while let Some(batch) = self.batches.recv().await {
            let block_hash = batch.context.block_hash;
            if let Err(error) = handler(batch).await {
                if error.is::<policy::BatchEvaluationError>() {
                    log::warn!("Skipping failed execution batch for block {block_hash}: {error:#}");
                    continue;
                }
                return Err(error)
                    .with_context(|| format!("Execution engine failed for block {block_hash}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::B256;
    use amms::amms::path::SimulationContext;
    use std::sync::{Arc, Mutex};

    fn batch(hash: u8) -> SimulationBatch {
        SimulationBatch {
            context: Arc::new(SimulationContext {
                chain_id: 1,
                block_number: 10,
                block_hash: B256::repeat_byte(hash),
                parent_hash: B256::ZERO,
                block_timestamp: 120,
                base_fee_per_gas: Some(100),
                gas_used: 15_000_000,
                gas_limit: 30_000_000,
                simulated_at: chrono::Utc::now(),
            }),
            results: vec![],
        }
    }

    #[tokio::test]
    async fn processes_empty_and_replacement_batches_in_order_and_drains_on_shutdown() {
        let (sender, engine) = ExecutionEngine::channel(NonZeroUsize::new(2).unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&received);
        let task = engine.spawn(move |batch| {
            let observed = Arc::clone(&observed);
            async move {
                observed.lock().unwrap().push(batch.context.block_hash);
                Ok(())
            }
        });
        sender.send(batch(1)).await.unwrap();
        sender.send(batch(2)).await.unwrap();
        drop(sender);
        task.await.unwrap().unwrap();
        assert_eq!(
            *received.lock().unwrap(),
            vec![B256::repeat_byte(1), B256::repeat_byte(2)]
        );
    }

    #[tokio::test]
    async fn handler_failure_closes_channel_and_is_reported() {
        let (sender, engine) = ExecutionEngine::channel(NonZeroUsize::new(1).unwrap());
        let task = engine.spawn(|_| async { anyhow::bail!("policy failed") });
        sender.send(batch(1)).await.unwrap();
        let error = task.await.unwrap().unwrap_err();
        assert!(format!("{error:#}").contains("policy failed"));
        assert!(sender.send(batch(2)).await.is_err());
    }

    #[tokio::test]
    async fn invalid_batch_is_skipped_and_next_batch_is_processed() {
        let (sender, engine) = ExecutionEngine::channel(NonZeroUsize::new(2).unwrap());
        let mut invalid = batch(1);
        Arc::make_mut(&mut invalid.context).base_fee_per_gas = None;
        sender.send(invalid).await.unwrap();
        sender.send(batch(2)).await.unwrap();
        drop(sender);
        let processed = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&processed);
        engine
            .run(move |batch| {
                let observed = Arc::clone(&observed);
                async move {
                    let hash = batch.context.block_hash;
                    policy::evaluate_batch_and_map_err(batch, &policy::ExecutionConfig::default())?;
                    observed.lock().unwrap().push(hash);
                    Ok(())
                }
            })
            .await
            .unwrap();
        assert_eq!(*processed.lock().unwrap(), vec![B256::repeat_byte(2)]);
    }

    #[tokio::test]
    async fn invalid_config_stops_consumer_even_for_a_bad_batch() {
        let (sender, engine) = ExecutionEngine::channel(NonZeroUsize::new(2).unwrap());
        let mut invalid = batch(1);
        Arc::make_mut(&mut invalid.context).base_fee_per_gas = None;
        sender.send(invalid).await.unwrap();
        sender.send(batch(2)).await.unwrap();
        let error = engine
            .run(|batch| async move {
                let config = policy::ExecutionConfig {
                    chain_id: 0,
                    ..Default::default()
                };
                policy::evaluate_batch_and_map_err(batch, &config)?;
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("chain ID is required"));
        assert!(!error.is::<policy::BatchEvaluationError>());
        assert!(sender.send(batch(3)).await.is_err());
    }

    #[tokio::test]
    async fn full_queue_applies_backpressure_without_dropping_batches() {
        let (sender, engine) = ExecutionEngine::channel(NonZeroUsize::new(1).unwrap());
        sender.send(batch(1)).await.unwrap();
        assert!(matches!(
            sender.try_send(batch(2)),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        drop(sender);
        engine.run(|_| async { Ok(()) }).await.unwrap();
    }
}
