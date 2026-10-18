//! Public-RPC submission using a caller-supplied signing middleware and executor adapter.
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use ethers::{
    providers::Middleware,
    types::{
        transaction::eip2718::TypedTransaction, Address, BlockNumber, Bytes,
        Eip1559TransactionRequest, H256, U256,
    },
};
use tokio::sync::Mutex;

use super::policy::ExecutionCandidate;

/// Implement for the deployed contract. The adapter must enforce min_amount_out
/// and deadline on chain and select the exact pools represented by the snapshot.
/// Funding assumes the executor already owns the input tokens (no flashloan).
/// Router mappings and any extra fees belong in the adapter. It must reject
/// unsupported paths, funding modes, or requirements rather than omit them.
pub trait ExecutorAdapter: Send + Sync {
    fn encode(&self, candidate: &ExecutionCandidate) -> Result<Bytes>;
}

/// Explicit placeholder: the repository's fallback ABI does not describe its
/// calldata layout or prove that minimum output and deadlines are enforced.
pub struct UnconfiguredExecutor;
impl ExecutorAdapter for UnconfiguredExecutor {
    fn encode(&self, _: &ExecutionCandidate) -> Result<Bytes> {
        bail!("Executor adapter is not configured: supply the deployed ABI, pool/router mapping, and on-chain output/deadline enforcement")
    }
}

#[derive(Debug, Clone)]
pub struct SubmissionConfig {
    pub enabled: bool,
    pub executor: Option<Address>,
    pub receipt_timeout_seconds: u64,
}
impl Default for SubmissionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            executor: None,
            receipt_timeout_seconds: 120,
        }
    }
}

pub struct LiveSubmitter<M> {
    client: Arc<M>,
    adapter: Arc<dyn ExecutorAdapter>,
    config: SubmissionConfig,
    // Serialises nonce selection/submission and prevents resubmitting the same head.
    last_submitted: Mutex<Option<H256>>,
}

impl<M: Middleware + 'static> LiveSubmitter<M> {
    pub fn new(
        client: Arc<M>,
        adapter: Arc<dyn ExecutorAdapter>,
        config: SubmissionConfig,
    ) -> Self {
        Self {
            client,
            adapter,
            config,
            last_submitted: Mutex::new(None),
        }
    }

    /// Evaluate on a blocking worker and submit at most the best candidate per batch.
    pub async fn process_batch(
        &self,
        batch: amms::amms::path::SimulationBatch,
        config: super::policy::ExecutionConfig,
    ) -> Result<Option<H256>> {
        ensure!(self.config.enabled, "Live submission is disabled");
        let candidates =
            tokio::task::spawn_blocking(move || super::policy::evaluate_batch(batch, &config))
                .await??;
        match candidates.into_iter().next() {
            Some(candidate) => self.submit(candidate).await,
            None => Ok(None),
        }
    }

    /// Returns None for a stale candidate. Errors stop the engine; never blindly
    /// retry a failed submission because an RPC error can hide an accepted tx.
    pub async fn submit(&self, mut candidate: ExecutionCandidate) -> Result<Option<H256>> {
        ensure!(self.config.enabled, "Live submission is disabled");
        let executor = self
            .config
            .executor
            .filter(|a| *a != Address::zero())
            .context("Executor address is not configured")?;
        ensure!(
            self.config.receipt_timeout_seconds > 0,
            "Receipt timeout must be positive"
        );
        let sender = self
            .client
            .default_sender()
            .context("Signing middleware is not configured")?;
        let mut last_submitted = self.last_submitted.lock().await;
        let hash = H256::from_slice(candidate.context.block_hash.as_slice());
        if *last_submitted == Some(hash) {
            return Ok(None);
        }
        let chain_id = self.client.get_chainid().await?;
        ensure!(
            chain_id == U256::from(candidate.context.chain_id),
            "Provider chain does not match candidate"
        );
        if !self.is_current(&candidate).await? {
            return Ok(None);
        }
        ensure!(
            !self.client.get_code(executor, None).await?.is_empty(),
            "Executor has no contract code"
        );
        let data = self.adapter.encode(&candidate)?;
        ensure!(!data.is_empty(), "Executor adapter returned empty calldata");
        let mut tx: TypedTransaction = Eip1559TransactionRequest::new()
            .from(sender)
            .to(executor)
            .chain_id(candidate.context.chain_id)
            .data(data)
            .value(U256::zero())
            .max_fee_per_gas(candidate.max_fee_per_gas)
            .max_priority_fee_per_gas(candidate.priority_fee_per_gas)
            .gas(candidate.gas_limit)
            .into();
        let pending_block = Some(BlockNumber::Pending.into());
        let estimated = self.client.estimate_gas(&tx, pending_block).await?;
        let padded = estimated
            .checked_mul(U256::from(120))
            .context("gas overflow")?
            / U256::from(100);
        ensure!(
            padded <= U256::from(candidate.gas_limit),
            "Estimated gas plus 20% margin exceeds configured limit"
        );
        if !candidate.apply_gas_estimate(estimated.as_u64(), padded.as_u64())? {
            return Ok(None);
        }
        // Tightening the on-chain output floor can change calldata and its gas cost.
        let final_data = self.adapter.encode(&candidate)?;
        ensure!(
            !final_data.is_empty(),
            "Executor adapter returned empty calldata"
        );
        let changed = tx.data() != Some(&final_data);
        tx.set_data(final_data);
        tx.set_gas(padded);
        if changed {
            let final_estimate = self.client.estimate_gas(&tx, pending_block).await?;
            if final_estimate > padded {
                return Ok(None);
            }
            // Use the padded limit as the final conservative estimate, avoiding
            // another calldata adjustment/estimation cycle.
            if !candidate.apply_gas_estimate(padded.as_u64(), padded.as_u64())? {
                return Ok(None);
            }
        }
        let gas_cost = padded
            .checked_mul(U256::from(candidate.max_fee_per_gas))
            .context("gas cost overflow")?;
        ensure!(
            self.client.get_balance(sender, pending_block).await? >= gas_cost,
            "Insufficient native balance for gas"
        );
        // The contract's min-output check must cover both gas and profit, even if
        // reserves change between this pending-state simulation and inclusion.
        self.client.call(&tx, pending_block).await?;
        if !self.is_current(&candidate).await? {
            return Ok(None);
        }
        tx.set_nonce(
            self.client
                .get_transaction_count(sender, pending_block)
                .await?,
        );
        // Mark before sending: an ambiguous RPC failure must not trigger a retry.
        *last_submitted = Some(hash);
        let pending = self.client.send_transaction(tx, None).await?;
        let tx_hash = pending.tx_hash();
        log::info!("Submitted transaction {tx_hash:#x}");
        let receipt = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.receipt_timeout_seconds),
            pending,
        )
        .await
        .with_context(|| {
            format!("Receipt timeout for {tx_hash:#x}; transaction may still be pending")
        })??
        .with_context(|| {
            format!("Transaction {tx_hash:#x} dropped; check nonce before retrying")
        })?;
        ensure!(
            receipt.status == Some(1u64.into()),
            "Transaction {tx_hash:#x} reverted"
        );
        Ok(Some(tx_hash))
    }

    async fn is_current(&self, candidate: &ExecutionCandidate) -> Result<bool> {
        let latest = self
            .client
            .get_block(BlockNumber::Latest)
            .await?
            .context("Latest block unavailable")?;
        Ok(
            latest.hash == Some(H256::from_slice(candidate.context.block_hash.as_slice()))
                && latest.number == Some(candidate.context.block_number.into())
                && latest.timestamp < U256::from(candidate.deadline),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::{evaluate_batch, tests::fixture, ExecutionConfig};
    use super::*;
    use ethers::providers::Provider;

    fn candidate() -> ExecutionCandidate {
        evaluate_batch(fixture(), &ExecutionConfig::default())
            .unwrap()
            .remove(0)
    }

    #[tokio::test]
    async fn defaults_cannot_submit_and_missing_executor_is_explicit() {
        let (provider, _) = Provider::mocked();
        let client = Arc::new(provider);
        let submitter = LiveSubmitter::new(
            client.clone(),
            Arc::new(UnconfiguredExecutor),
            SubmissionConfig::default(),
        );
        assert!(submitter
            .submit(candidate())
            .await
            .unwrap_err()
            .to_string()
            .contains("disabled"));
        let submitter = LiveSubmitter::new(
            client,
            Arc::new(UnconfiguredExecutor),
            SubmissionConfig {
                enabled: true,
                ..SubmissionConfig::default()
            },
        );
        assert!(submitter
            .submit(candidate())
            .await
            .unwrap_err()
            .to_string()
            .contains("address"));
        assert!(UnconfiguredExecutor.encode(&candidate()).is_err());
    }

    #[tokio::test]
    async fn skips_stale_or_orphaned_candidates_before_encoding_or_sending() {
        let (provider, mock) = Provider::mocked();
        let client = Arc::new(provider.with_sender(Address::repeat_byte(1)));
        let submitter = LiveSubmitter::new(
            client,
            Arc::new(UnconfiguredExecutor),
            SubmissionConfig {
                enabled: true,
                executor: Some(Address::repeat_byte(2)),
                ..SubmissionConfig::default()
            },
        );
        let mut latest: ethers::types::Block<H256> = Default::default();
        latest.number = Some(10u64.into());
        latest.hash = Some(H256::repeat_byte(99)); // Same-height reorg.
        latest.timestamp = 120u64.into();
        mock.push(Some(latest)).unwrap();
        mock.push(U256::one()).unwrap();
        assert!(submitter.submit(candidate()).await.unwrap().is_none());
        mock.assert_request("eth_chainId", ()).unwrap();
        mock.assert_request("eth_getBlockByNumber", ("latest", false))
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_wrong_network_before_sending() {
        let (provider, mock) = Provider::mocked();
        let submitter = LiveSubmitter::new(
            Arc::new(provider.with_sender(Address::repeat_byte(1))),
            Arc::new(UnconfiguredExecutor),
            SubmissionConfig {
                enabled: true,
                executor: Some(Address::repeat_byte(2)),
                ..SubmissionConfig::default()
            },
        );
        mock.push(U256::from(2)).unwrap();
        assert!(submitter
            .submit(candidate())
            .await
            .unwrap_err()
            .to_string()
            .contains("chain"));
        mock.assert_request("eth_chainId", ()).unwrap();
    }
    struct TestAdapter;
    impl ExecutorAdapter for TestAdapter {
        fn encode(&self, candidate: &ExecutionCandidate) -> Result<Bytes> {
            assert!(candidate.min_amount_out > candidate.amount_in);
            Ok(Bytes::from(vec![1, 2, 3, 4]))
        }
    }

    fn current_block() -> ethers::types::Block<H256> {
        let candidate = candidate();
        ethers::types::Block {
            hash: Some(H256::from_slice(candidate.context.block_hash.as_slice())),
            number: Some(candidate.context.block_number.into()),
            timestamp: candidate.context.block_timestamp.into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn submits_preflighted_order_and_checks_receipt_without_duplicate_submission() {
        for success in [true, false] {
            let (provider, mock) = Provider::mocked();
            let provider = provider
                .with_sender(Address::repeat_byte(1))
                .interval(std::time::Duration::from_millis(1));
            let submitter = LiveSubmitter::new(
                Arc::new(provider),
                Arc::new(TestAdapter),
                SubmissionConfig {
                    enabled: true,
                    executor: Some(Address::repeat_byte(2)),
                    ..SubmissionConfig::default()
                },
            );
            let hash = H256::repeat_byte(42);
            let receipt = ethers::types::TransactionReceipt {
                transaction_hash: hash,
                status: Some(u64::from(success).into()),
                block_number: Some(11u64.into()),
                ..Default::default()
            };
            // Mock responses are consumed last-in, first-out.
            mock.push(Some(receipt)).unwrap();
            mock.push(Some(ethers::types::Transaction {
                hash,
                block_number: Some(11u64.into()),
                ..Default::default()
            }))
            .unwrap();
            mock.push(hash).unwrap();
            mock.push(U256::zero()).unwrap(); // pending nonce
            mock.push(Some(current_block())).unwrap(); // final freshness check
            mock.push::<Bytes, _>(Bytes::new()).unwrap(); // pending eth_call
            mock.push(U256::MAX).unwrap(); // gas balance
            mock.push(U256::from(100_000)).unwrap(); // estimateGas
            mock.push::<Bytes, _>(Bytes::from(vec![1])).unwrap(); // executor code
            mock.push(Some(current_block())).unwrap();
            mock.push(U256::one()).unwrap();
            let result = submitter.submit(candidate()).await;
            if success {
                assert_eq!(result.unwrap(), Some(hash));
            } else {
                assert!(result.unwrap_err().to_string().contains("reverted"));
            }
            assert!(submitter.submit(candidate()).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn rejects_estimated_gas_above_cap_before_sending() {
        let (provider, mock) = Provider::mocked();
        let submitter = LiveSubmitter::new(
            Arc::new(provider.with_sender(Address::repeat_byte(1))),
            Arc::new(TestAdapter),
            SubmissionConfig {
                enabled: true,
                executor: Some(Address::repeat_byte(2)),
                ..SubmissionConfig::default()
            },
        );
        mock.push(U256::from(600_000)).unwrap(); // Margin would exceed cap.
        mock.push::<Bytes, _>(Bytes::from(vec![1])).unwrap();
        mock.push(Some(current_block())).unwrap();
        mock.push(U256::one()).unwrap();
        assert!(submitter
            .submit(candidate())
            .await
            .unwrap_err()
            .to_string()
            .contains("gas"));
    }
    #[tokio::test]
    async fn tightened_profit_floor_is_reencoded_and_gas_checked_again() {
        struct FloorAdapter(std::sync::Mutex<Vec<alloy::primitives::U256>>);
        impl ExecutorAdapter for FloorAdapter {
            fn encode(&self, candidate: &ExecutionCandidate) -> Result<Bytes> {
                self.0.lock().unwrap().push(candidate.min_amount_out);
                Ok(Bytes::from(
                    candidate.min_amount_out.to_be_bytes::<32>().to_vec(),
                ))
            }
        }
        let config = ExecutionConfig {
            slippage_bps: 9999,
            ..ExecutionConfig::default()
        };
        let candidate = evaluate_batch(fixture(), &config).unwrap().remove(0);
        let (provider, mock) = Provider::mocked();
        let adapter = Arc::new(FloorAdapter(std::sync::Mutex::new(vec![])));
        let submitter = LiveSubmitter::new(
            Arc::new(provider.with_sender(Address::repeat_byte(1))),
            adapter.clone(),
            SubmissionConfig {
                enabled: true,
                executor: Some(Address::repeat_byte(2)),
                ..SubmissionConfig::default()
            },
        );
        mock.push(U256::from(600_000)).unwrap(); // Changed calldata exceeds tightened 540k limit.
        mock.push(U256::from(450_000)).unwrap();
        mock.push::<Bytes, _>(Bytes::from(vec![1])).unwrap();
        mock.push(Some(current_block())).unwrap();
        mock.push(U256::one()).unwrap();
        assert!(submitter.submit(candidate).await.unwrap().is_none());
        let floors = adapter.0.lock().unwrap();
        assert_eq!(floors.len(), 2);
        assert!(floors[1] > floors[0]);
    }
}
