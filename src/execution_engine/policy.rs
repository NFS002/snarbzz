//! Snapshot evaluation. Amounts and profits use raw wrapped-native-token units.
use std::{collections::HashSet, sync::Arc};

use alloy::primitives::{Address, I256, U256};
use amms::amms::{math::percentage_change_bp, path::{SimulationBatch, SimulationContext, UniswapArbPathSnapshot}};
use anyhow::{ensure, Result};
use itertools::Itertools;
use log::{debug, info};

#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    pub chain_id: u64,
    pub wrapped_native_token: Address,
    pub min_amount_in: U256,
    pub max_amount_in: U256,
    pub min_net_profit: U256,
    /// Initial estimate, replaced by RPC estimation before submission.
    pub estimated_gas: u64,
    pub max_gas: u64,
    pub priority_fee_per_gas: u64,
    pub max_fee_per_gas: u64,
    pub slippage_bps: u16,
    pub deadline_seconds: u64,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            chain_id: 1,
            wrapped_native_token: crate::constants::WETH_ADDRESS,
            min_amount_in: U256::from(1_000_000_000_000_000u64), // 0.001 WETH
            max_amount_in: U256::from(100_000_000_000_000_000u64), // 0.1 WETH
            min_net_profit: U256::from(100_000_000_000_000u64),  // 0.0001 WETH
            estimated_gas: 400_000,
            max_gas: 600_000,
            priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 30_000_000_000,
            slippage_bps: 50,
            deadline_seconds: 24,
        }
    }
}

impl ExecutionConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.chain_id != 0, "chain ID is required");
        ensure!(
            self.wrapped_native_token != Address::ZERO,
            "wrapped native token is required"
        );
        ensure!(
            self.min_amount_in > U256::ZERO && self.min_amount_in <= self.max_amount_in,
            "invalid input bounds"
        );
        ensure!(
            self.max_gas > 0 && self.deadline_seconds > 0,
            "gas and deadline must be positive"
        );
        ensure!(
            self.estimated_gas > 0 && self.estimated_gas <= self.max_gas,
            "estimated gas must be positive and no greater than max gas"
        );
        ensure!(self.slippage_bps < 10_000, "slippage must be below 100%");
        ensure!(
            self.priority_fee_per_gas <= self.max_fee_per_gas,
            "priority fee exceeds fee cap"
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionCandidate {
    pub context: Arc<SimulationContext>,
    pub path_snapshot: UniswapArbPathSnapshot,
    pub amount_in: U256,
    pub amount_out: U256,
    pub min_amount_out: U256,
    pub gross_profit: U256,
    /// Snapshot profit minus expected gas cost, not the maximum spending budget.
    pub net_profit: U256,
    pub pct_gain_bp: I256,
    pub predicted_base_fee_per_gas: u64,
    pub expected_fee_per_gas: u64,
    pub estimated_gas: u64,
    pub expected_gas_cost: U256,
    pub max_gas_cost: U256,
    pub min_net_profit: U256,
    pub gas_limit: u64,
    pub max_fee_per_gas: u64,
    pub priority_fee_per_gas: u64,
    pub deadline: u64,
}

/// Ethereum EIP-1559 next-block base fee (elasticity 2, adjustment denominator 8).
/// https://eips.ethereum.org/EIPS/eip-1559
pub fn predict_next_base_fee(base_fee: u64, gas_used: u64, gas_limit: u64) -> Result<u64> {
    let target = gas_limit / 2;
    ensure!(
        target > 0 && gas_used <= gas_limit,
        "invalid block gas usage/limit"
    );
    let base = u128::from(base_fee);
    let delta = base * u128::from(gas_used.abs_diff(target)) / u128::from(target) / 8;
    let next = if gas_used > target {
        base + delta.max(1)
    } else {
        base - delta
    };
    Ok(u64::try_from(next)?)
}

impl ExecutionCandidate {
    /// Replace the screening estimate and enforce the maximum-cost profit floor
    /// using the actual transaction's padded gas limit before submission.
    pub fn apply_gas_estimate(&mut self, estimated: u64, padded: u64) -> Result<bool> {
        ensure!(
            estimated > 0 && padded >= estimated && padded <= self.gas_limit,
            "invalid transaction gas estimate"
        );
        let expected_cost = U256::from(estimated) * U256::from(self.expected_fee_per_gas);
        let max_cost = U256::from(padded) * U256::from(self.max_fee_per_gas);
        let Some(net) = self.gross_profit.checked_sub(expected_cost) else {
            return Ok(false);
        };
        if net < self.min_net_profit {
            return Ok(false);
        }
        let Some(floor) = self
            .amount_in
            .checked_add(max_cost)
            .and_then(|v| v.checked_add(self.min_net_profit))
        else {
            return Ok(false);
        };
        if floor > self.amount_out {
            return Ok(false);
        }
        self.estimated_gas = estimated;
        self.gas_limit = padded;
        self.expected_gas_cost = expected_cost;
        self.max_gas_cost = max_cost;
        self.net_profit = net;
        self.min_amount_out = self.min_amount_out.max(floor);
        Ok(true)
    }
}

/// Invalid batch data; the consumer may log this error and process the next batch.
#[derive(Debug)]
pub struct BatchEvaluationError(pub anyhow::Error);

impl std::fmt::Display for BatchEvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Batch evaluation failed: {:#}", self.0)
    }
}

impl std::error::Error for BatchEvaluationError {}

/// Bounded search over snapshots; this is an approximation, not a guaranteed optimum.
/// Run on a blocking worker rather than occupying the subscription's async task.

pub fn evaluate_batch_and_map_err(
    batch: SimulationBatch,
    config: &ExecutionConfig,
) -> Result<Vec<ExecutionCandidate>> {
    // Configuration errors are fatal; only batch evaluation errors are recoverable.
    config.validate()?;
    evaluate_batch(batch, config).map_err(|error| BatchEvaluationError(error).into())
}

fn evaluate_batch(
    batch: SimulationBatch,
    config: &ExecutionConfig,
) -> Result<Vec<ExecutionCandidate>> {
    ensure!(
        batch.context.chain_id == config.chain_id,
        "batch chain does not match execution config"
    );
    let base_fee = batch
        .context
        .base_fee_per_gas
        .ok_or_else(|| anyhow::anyhow!("missing base_fee_per_gas"))?;
    let predicted_base_fee =
        predict_next_base_fee(base_fee, batch.context.gas_used, batch.context.gas_limit)?;
    let expected_fee = predicted_base_fee
        .checked_add(config.priority_fee_per_gas)
        .ok_or_else(|| anyhow::anyhow!("expected gas fee overflow"))?;
    if expected_fee > config.max_fee_per_gas {
        debug!(
            "Rejecting batch {}: expected fee {} exceeds cap {}",
            batch.context.block_hash, expected_fee, config.max_fee_per_gas
        );
        return Ok(vec![]);
    }
    // Fee headroom is a spending limit, not the expected transaction price.
    let max_fee = base_fee
        .saturating_mul(2)
        .saturating_add(config.priority_fee_per_gas)
        .max(expected_fee)
        .min(config.max_fee_per_gas);
    let expected_gas_cost = U256::from(config.estimated_gas) * U256::from(expected_fee);
    let max_gas_cost = U256::from(config.max_gas) * U256::from(max_fee);
    let deadline = batch
        .context
        .block_timestamp
        .checked_add(config.deadline_seconds)
        .ok_or_else(|| anyhow::anyhow!("deadline overflow"))?;
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for result in batch.results {
        let path = result.path_snapshot;
        let path_id = path.id();
        let block_hash = batch.context.block_hash;
        if !valid_cycle(&path, config.wrapped_native_token) {
            debug!("Rejecting path {path_id} in block {block_hash}: invalid wrapped-native-token cycle");
            continue;
        }
        if !seen.insert(path_id) {
            debug!("Rejecting path {path_id} in block {block_hash}: duplicate path");
            continue;
        }
        let Some((amount_in, amount_out, gross_profit)) = optimise(&path, config, result.amount_in)
        else {
            debug!("Rejecting path {path_id} in block {block_hash}: no valid non-loss-making simulation within input bounds");
            continue;
        };
        let Some(net_profit) = gross_profit.checked_sub(expected_gas_cost) else {
            debug!("Rejecting path {path_id} in block {block_hash}: gross profit {gross_profit} below expected gas cost {expected_gas_cost}");
            continue;
        };
        if net_profit < config.min_net_profit {
            debug!("Rejecting path {path_id} in block {block_hash}: net profit {net_profit} below minimum {}", config.min_net_profit);
            continue;
        }
        let Some(profit_floor) = amount_in
            .checked_add(expected_gas_cost)
            .and_then(|v| v.checked_add(config.min_net_profit))
        else {
            debug!("Rejecting path {path_id} in block {block_hash}: profit floor overflow");
            continue;
        };
        let Some(pct_gain_bp) = percentage_change_bp(amount_in, amount_out).ok() else {
            debug!("Rejecting path {path_id} in block {block_hash}: unable to calculate percentage_gain");
            continue;
        };
        // Divide first to avoid an intermediate multiplication overflowing U256.
        let slippage_discount = (amount_out / U256::from(10_000)) * U256::from(config.slippage_bps);
        let min_amount_out = (amount_out - slippage_discount).max(profit_floor);
        candidates.push(ExecutionCandidate {
            context: Arc::clone(&batch.context),
            path_snapshot: path,
            amount_in,
            amount_out,
            min_amount_out,
            gross_profit,
            net_profit,
            pct_gain_bp,
            predicted_base_fee_per_gas: predicted_base_fee,
            expected_fee_per_gas: expected_fee,
            estimated_gas: config.estimated_gas,
            expected_gas_cost,
            max_gas_cost,
            min_net_profit: config.min_net_profit,
            gas_limit: config.max_gas,
            max_fee_per_gas: max_fee,
            priority_fee_per_gas: config.priority_fee_per_gas,
            deadline,
        });
    }
    // (!candidates.is_empty()).then(|| candidates.sort_by(|a, b| b.net_profit.cmp(&a.net_profit)));
    candidates.sort_by(|a, b| b.net_profit.cmp(&a.net_profit));
    if candidates.len() >= 2 {
        info!("Selecting first from {} candidates", candidates.len());
    }
    for candidate in candidates.iter().skip(1) {
        debug!(
            "Rejecting path {} in block {}: lower-ranked candidate (net profit {})",
            candidate.path_snapshot.id(),
            candidate.context.block_hash,
            candidate.net_profit
        );
    }
    let top_candidate = candidates.into_iter().take(1).collect_vec();
    Ok(top_candidate)
}

fn valid_cycle(path: &UniswapArbPathSnapshot, token: Address) -> bool {
    let (Some(first), Some(last)) = (path.hops().first(), path.hops().last()) else {
        return false;
    };
    let mut pools = HashSet::new();
    first.base == token
        && last.quote == token
        && path
            .hops()
            .iter()
            .all(|hop| hop.base != hop.quote && pools.insert(hop.pool.address()))
        && path
            .hops()
            .windows(2)
            .all(|hops| hops[0].quote == hops[1].base)
}

fn optimise(
    path: &UniswapArbPathSnapshot,
    config: &ExecutionConfig,
    original: U256,
) -> Option<(U256, U256, U256)> {
    let mut best = None;
    let mut score = |amount| {
        let out = path.simulate(amount).ok()?;
        let profit = out.checked_sub(amount)?;
        if best
            .as_ref()
            .is_none_or(|&(_, _, previous)| profit > previous)
        {
            best = Some((amount, out, profit));
        }
        Some(profit)
    };
    score(original.clamp(config.min_amount_in, config.max_amount_in));
    score(config.min_amount_in);
    score(config.max_amount_in);
    // A coarse grid avoids relying only on the original (possibly unprofitable) input.
    let width = config.max_amount_in - config.min_amount_in;
    for step in 1..16u64 {
        score(config.min_amount_in + (width / U256::from(16)) * U256::from(step));
    }
    let mut low = config.min_amount_in;
    let mut high = config.max_amount_in;
    for _ in 0..48 {
        let third = (high - low) / U256::from(3);
        if third == U256::ZERO {
            break;
        }
        let left = low + third;
        let right = high - third;
        let left_profit = score(left).unwrap_or_default();
        let right_profit = score(right).unwrap_or_default();
        if left_profit < right_profit {
            low = left;
        } else {
            high = right;
        }
    }
    score(low);
    score(high);
    best
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alloy::primitives::B256;
    use amms::amms::{
        amm::UniswapPool,
        path::{UniswapHopSnapshot, UniswapSimulationResult},
        uniswap_v2::UniswapV2Pool,
    };

    pub(crate) fn fixture() -> SimulationBatch {
        let weth = crate::constants::WETH_ADDRESS;
        let token = Address::repeat_byte(2);
        let make_pool = |address, reserve_1| {
            let mut pool = UniswapV2Pool::new(Address::repeat_byte(address), 300);
            pool.token_a.address = weth;
            pool.token_b.address = token;
            pool.token_a.decimals = 18;
            pool.token_b.decimals = 18;
            pool.reserve_0 = 1000 * 10u128.pow(18);
            pool.reserve_1 = reserve_1 * 10u128.pow(18);
            UniswapPool::V2(pool)
        };
        let hops = vec![
            UniswapHopSnapshot {
                pool: make_pool(10, 2000),
                base: weth,
                quote: token,
            },
            UniswapHopSnapshot {
                pool: make_pool(11, 1000),
                base: token,
                quote: weth,
            },
        ];
        let path_snapshot = UniswapArbPathSnapshot::from_hops(hops);
        let amount_in = ExecutionConfig::default().min_amount_in;
        let amount_out = path_snapshot.simulate(amount_in).unwrap();
        SimulationBatch {
            context: Arc::new(SimulationContext {
                chain_id: 1,
                block_number: 10,
                block_hash: B256::repeat_byte(10),
                parent_hash: B256::repeat_byte(9),
                block_timestamp: 120,
                base_fee_per_gas: Some(1_000_000_000),
                gas_used: 15_000_000,
                gas_limit: 30_000_000,
                simulated_at: chrono::Utc::now(),
            }),
            results: vec![UniswapSimulationResult {
                path_snapshot,
                amount_in,
                amount_out,
                pct_gain_bp: amms::amms::math::percentage_change_bp(amount_in, amount_out).unwrap(),
            }],
        }
    }

    #[test]
    fn optimises_amount_and_reserves_gas_and_profit_in_output_floor() {
        let config = ExecutionConfig::default();
        let batch = fixture();
        let original = batch.results[0].amount_in;
        let candidates = evaluate_batch_and_map_err(batch, &config).unwrap();
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert!(candidate.amount_in > original);
        assert!(candidate.amount_in <= config.max_amount_in);
        assert_eq!(
            candidate
                .path_snapshot
                .simulate(candidate.amount_in)
                .unwrap(),
            candidate.amount_out
        );
        let gas = candidate.expected_gas_cost;
        assert_eq!(
            candidate.net_profit,
            candidate.amount_out - candidate.amount_in - gas
        );
        assert!(candidate.min_amount_out >= candidate.amount_in + gas + config.min_net_profit);
        assert!(candidate.min_amount_out <= candidate.amount_out);
    }

    #[test]
    fn rejects_missing_fees_and_skips_excessive_fees_and_unprofitable_candidates() {
        let config = ExecutionConfig::default();
        let mut batch = fixture();
        Arc::make_mut(&mut batch.context).base_fee_per_gas = None;
        assert!(evaluate_batch_and_map_err(batch, &config)
            .unwrap_err()
            .is::<BatchEvaluationError>());
        let mut batch = fixture();
        Arc::make_mut(&mut batch.context).base_fee_per_gas = Some(config.max_fee_per_gas);
        assert!(evaluate_batch_and_map_err(batch, &config)
            .unwrap()
            .is_empty());
        let config = ExecutionConfig {
            min_net_profit: U256::MAX,
            ..config
        };
        assert!(evaluate_batch_and_map_err(fixture(), &config)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_non_native_cycles_and_reused_pools() {
        let config = ExecutionConfig::default();
        let mut batch = fixture();
        let mut hops = batch.results[0].path_snapshot.hops().to_vec();
        hops[0].base = Address::ZERO;
        batch.results[0].path_snapshot = UniswapArbPathSnapshot::from_hops(hops);
        assert!(evaluate_batch_and_map_err(batch, &config)
            .unwrap()
            .is_empty());
        let mut batch = fixture();
        let mut hops = batch.results[0].path_snapshot.hops().to_vec();
        hops[1].pool = hops[0].pool.clone();
        batch.results[0].path_snapshot = UniswapArbPathSnapshot::from_hops(hops);
        assert!(evaluate_batch_and_map_err(batch, &config)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn validates_config_and_chain_and_deduplicates_routes() {
        let mut config = ExecutionConfig::default();
        config.min_amount_in = U256::ZERO;
        assert!(evaluate_batch_and_map_err(fixture(), &config).is_err());
        let config = ExecutionConfig::default();
        let mut batch = fixture();
        Arc::make_mut(&mut batch.context).chain_id = 2;
        assert!(evaluate_batch_and_map_err(batch, &config).is_err());
        let mut batch = fixture();
        batch.results.push(batch.results[0].clone());
        assert_eq!(evaluate_batch_and_map_err(batch, &config).unwrap().len(), 1);
    }
    #[test]
    fn next_base_fee_matches_eip1559_rounding_and_limits() {
        assert_eq!(
            predict_next_base_fee(1_000_000_000, 15_000_000, 30_000_000).unwrap(),
            1_000_000_000
        );
        assert_eq!(
            predict_next_base_fee(1_000_000_000, 30_000_000, 30_000_000).unwrap(),
            1_125_000_000
        );
        assert_eq!(
            predict_next_base_fee(1_000_000_000, 0, 30_000_000).unwrap(),
            875_000_000
        );
        assert_eq!(predict_next_base_fee(1, 15_000_001, 30_000_000).unwrap(), 2);
        assert_eq!(predict_next_base_fee(1, 14_999_999, 30_000_000).unwrap(), 1);
        assert!(predict_next_base_fee(1, 1, 0).is_err());
        assert!(predict_next_base_fee(1, 31, 30).is_err());
        assert!(predict_next_base_fee(u64::MAX, 30, 30).is_err());
    }

    #[test]
    fn candidate_screening_uses_expected_cost_not_maximum_budget() {
        let config = ExecutionConfig {
            max_amount_in: ExecutionConfig::default().min_amount_in,
            ..ExecutionConfig::default()
        };
        let mut candidate = evaluate_batch_and_map_err(fixture(), &config)
            .unwrap()
            .remove(0);
        assert_eq!(candidate.predicted_base_fee_per_gas, 1_000_000_000);
        assert_eq!(candidate.expected_fee_per_gas, 2_000_000_000);
        assert_eq!(candidate.max_fee_per_gas, 3_000_000_000);
        assert!(candidate.gross_profit < candidate.max_gas_cost);
        assert!(candidate.net_profit >= config.min_net_profit);
        assert!(candidate.min_amount_out <= candidate.amount_out);
        assert!(candidate.apply_gas_estimate(100_000, 120_000).unwrap());
        assert_eq!(
            candidate.expected_gas_cost,
            U256::from(200_000_000_000_000u64)
        );
        assert_eq!(candidate.max_gas_cost, U256::from(360_000_000_000_000u64));
        assert!(
            candidate.min_amount_out
                >= candidate.amount_in + candidate.max_gas_cost + config.min_net_profit
        );
        assert_eq!(candidate.gas_limit, 120_000);
    }

    #[test]
    fn submission_profit_protection_can_reject_a_screened_candidate() {
        let config = ExecutionConfig {
            max_amount_in: ExecutionConfig::default().min_amount_in,
            ..ExecutionConfig::default()
        };
        let mut candidate = evaluate_batch_and_map_err(fixture(), &config)
            .unwrap()
            .remove(0);
        assert!(!candidate.apply_gas_estimate(300_000, 360_000).unwrap());
        assert!(candidate.apply_gas_estimate(700_000, 800_000).is_err());
    }

    #[test]
    fn fee_headroom_is_capped_without_rejecting_an_affordable_prediction() {
        let mut batch = fixture();
        Arc::make_mut(&mut batch.context).base_fee_per_gas = Some(20_000_000_000);
        let candidate = evaluate_batch_and_map_err(batch, &ExecutionConfig::default())
            .unwrap()
            .remove(0);
        assert_eq!(candidate.expected_fee_per_gas, 21_000_000_000);
        assert_eq!(candidate.max_fee_per_gas, 30_000_000_000);
    }
}
