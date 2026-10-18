# Execution engine

`StateSpaceManager::subscribe()` yields an owned `SimulationBatch` after releasing
its state lock. `ExecutionEngine::channel()` creates a bounded queue; `spawn()`
runs the supplied batch handler in a separate Tokio task. Full queues apply
backpressure without dropping updates. Empty batches and same-height reorg
replacements are delivered too.

`examples/test_reorg_state.rs` runs snapshot evaluation on a blocking worker and
logs candidates. It does **not** load a signer for execution or send transactions.

## Starting configuration

`ExecutionConfig::default()` assumes Ethereum mainnet and WETH-denominated cycles.
All amounts are raw token units; profit is comparable to native gas cost only
for the configured wrapped native token.

| Setting | Default |
| --- | --- |
| Input range | 0.001–0.1 WETH |
| Minimum net profit | 0.0001 WETH |
| Initial estimated gas use | 400,000 |
| Maximum gas limit | 600,000 |
| Priority fee | 1 gwei |
| Maximum fee cap | 30 gwei |
| Slippage ceiling | 50 basis points (0.5%) |
| Deadline | Source block timestamp + 24 seconds |
| Live submission | Disabled |
| Executor address | Unconfigured |
| Funding | Executor already holds the input tokens |
| Submission transport | Public RPC via supplied middleware |

These are editable starting values, not calibrated profitability predictions.
`evaluate_batch` performs a bounded approximate search for profitable inputs,
rejects discontinuous routes and repeated pools, and ranks candidates by profit
net of expected gas cost. Next-block base fee follows Ethereum EIP-1559's
integer formula (elasticity 2, adjustment denominator 8), without random noise.
Expected cost is `estimated_gas * (predicted_base_fee + priority_fee)`.
The 400,000 gas screening estimate is configurable, not a per-route RPC estimate.

The separate transaction fee cap is `2 * current_base_fee + priority_fee`, at
least the expected fee and no higher than the configured fee cap. A prediction
that cannot accommodate the full configured tip within that cap is rejected.
`max_gas_cost` records `gas_limit * max_fee_per_gas`; it is not subtracted when
screening/ranking candidates. The provisional output floor covers slippage and
expected cost plus minimum profit.

Before submission, RPC estimation replaces the screening estimate. The gas limit
is tightened to that estimate plus 20%, within the configured maximum, and the
output floor is strengthened to cover this limit at the maximum fee plus minimum
profit. A candidate that cannot meet this final protection is skipped. If the
updated floor changes calldata, gas is re-estimated and must fit the tightened
limit; that limit is then also used as a conservative expected-gas estimate.
Thus selection uses expected economics while submission retains maximum-cost
profit protection. No flashloan fees or non-native asset conversion are assumed.
V3 snapshots can be evaluated; their completeness and route support must also be
validated by the executor adapter.

## Connecting live submission

`LiveSubmitter::process_batch` evaluates a batch and submits at most its best
candidate. It validates the chain and requires the batch hash/number to match the
provider's latest head, verifies executor code, estimates gas with a 20% margin,
checks the sender's gas balance, performs a pending-state `eth_call`, rechecks the
head, obtains the pending nonce, sends the transaction, and checks the receipt.
Transactions with uncertain outcomes are not automatically retried. Receipt
success means inclusion, not finality. A mined revert still consumes gas.

The following are intentionally left for your deployment:

1. Supply a signing middleware, for example `SignerMiddleware<Provider<Http>,
   LocalWallet>`. Existing environment variable names are `HTTPS_URL`, `CHAIN_ID`,
   `PRIVATE_KEY`, and `BOT_ADDRESS`; do not log private-key values. The example's
   existing environment loader may require other application variables too.
2. Implement `ExecutorAdapter::encode` for the **actual deployed contract**. It
   must encode the exact pool route and enforce `min_amount_out` and `deadline`
   on chain. Supply pool-to-router mappings if that contract uses routers, and
   reject unsupported pool types. It must use the stated funding model or account
   explicitly for additional costs. `UnconfiguredExecutor` returns an error.
3. Set `SubmissionConfig.executor` and explicitly set `enabled: true` only after
   supplying that adapter. The legacy `Bundler::order_tx` fallback encoding is not
   automatically selected: its ABI does not establish these protections.

With `client` and `adapter` supplied by your application:

```rust,ignore
let submitter = Arc::new(LiveSubmitter::new(
    client,
    adapter,
    SubmissionConfig {
        enabled: true,
        executor: Some(executor_address),
        ..Default::default()
    },
));
let config = ExecutionConfig::default();
let task = engine.spawn(move |batch| {
    let submitter = Arc::clone(&submitter);
    let config = config.clone();
    async move {
        submitter.process_batch(batch, config).await?;
        Ok(())
    }
});
```

Use one submitter for the dedicated signing account: its mutex serialises nonce
selection and submission. Do not concurrently send unrelated transactions from
that account. Engine errors terminate the worker and propagate through the task
handle; inspect a submitted hash before restarting after an RPC error or timeout.
Private relays and flashloans remain deployment-specific extensions.

Run the offline engine tests with:

```sh
cargo test --offline --lib execution_engine::
```
