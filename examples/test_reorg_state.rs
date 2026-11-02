use std::sync::Arc;

use amms::amms::{amm::AMM, factory::Factory, math::format_percent_bp, uniswap_v2::UniswapV2Pool};
use anyhow::Result;
use futures::StreamExt;
use log::{debug, info};
use rust::{
    constants::{
        Env, MIN_WETH_THRESHOLD, TEST_POOLS, UNISWAP_V2_FACTORY_ADDRESS,
        UNISWAP_V3_FACTORY_ADDRESS, WETH_ADDRESS,
    },
    execution_engine::policy::evaluate_batch_and_map_err,
};
use url::Url;

use alloy::{
    primitives::U256,
    providers::{ProviderBuilder, WsConnect},
    rpc::client::ClientBuilder,
    transports::layers::{RetryBackoffLayer, ThrottleLayer},
};

use amms::{
    amms::uniswap_v2::UniswapV2Factory,
    state_space::{
        filters::{value::ValueFilter, whitelist::PoolWhitelistFilter, PoolFilter},
        StateSpaceBuilder,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    //setup_logger()?;
    tracing_subscriber::fmt::init();

    let env = Env::new();
    // let factory_addresses = vec!["0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac"];
    // let router_addresses = vec!["0xd9e1cE17f2641f24aE83637ab66a2cca9C378B9F"];
    // let factory_blocks = vec![10794229u64];

    let rpc_https_url = Url::parse(env.https_url.as_str())?;
    let wss_url = Url::parse(env.wss_url.as_str())?;

    //let uniswapv2_factory_address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");

    let http_client = ClientBuilder::default()
        .layer(ThrottleLayer::new(100))
        .layer(RetryBackoffLayer::new(5, 200, 330))
        .http(rpc_https_url);

    let ws_client = ClientBuilder::default()
        .layer(ThrottleLayer::new(100))
        .pubsub(WsConnect::new(wss_url))
        .await?;

    let http_provider = Arc::new(ProviderBuilder::new().connect_client(http_client));
    let wss_provider = Arc::new(ProviderBuilder::new().connect_client(ws_client));

    let factories: Vec<Factory> = vec![
        UniswapV2Factory::new(UNISWAP_V2_FACTORY_ADDRESS, 300, 10000835).into(),
        // UniswapV3Factory::new(UNISWAP_V3_FACTORY_ADDRESS, 12369621)
    ];

    let pool_filter: Vec<PoolFilter> = vec![PoolWhitelistFilter::new(TEST_POOLS.to_vec()).into()];

    let value_filters: Vec<PoolFilter> = vec![
        //PoolWhitelistFilter::new(vec![address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640")]).into(),
        //TokenWhitelistFilter::new(WHITELIST_TOKENS.to_vec()).into(),
        ValueFilter::new(
            UNISWAP_V2_FACTORY_ADDRESS,
            UNISWAP_V3_FACTORY_ADDRESS,
            WETH_ADDRESS,
            U256::from(MIN_WETH_THRESHOLD),
            http_provider.clone(),
        )
        .into(),
    ];

    //let _state_space_manager = sync!(factories, filters, provider);

    let test_amms: Vec<AMM> = TEST_POOLS
        .into_iter()
        .map(|address| UniswapV2Pool::new(address, 300).into())
        .collect();

    let _state_space_manager = Arc::new(
        StateSpaceBuilder::new(http_provider.clone())
            .from_cache("cache/pools/test-pools-v2.json".to_string())
            //.with_amms(test_amms)
            //.with_factories(factories)
            .with_pubsub_provider(wss_provider)
            //.with_filters(pool_filter)
            // .to_cache(Some(
            //     "cache/pools/test-pools-v2.json".to_string()
            // ))
            .sync()
            .await?,
    );

    let (execution_tx, engine) =
        rust::execution_engine::ExecutionEngine::channel(std::num::NonZeroUsize::new(8).unwrap());
    // Defaults evaluate WETH cycles only. This example never signs or sends orders.
    let execution_config = rust::execution_engine::policy::ExecutionConfig::default();
    execution_config.validate()?;

    let mut engine_task = engine.spawn(move |batch| {
        let config = execution_config.clone();
        async move {
            let block_hash = batch.context.block_hash;
            for result in &batch.results {
                debug!(
                    "----Arb Path---\nPath: {:#?}\nAmount in: {}\nSimulated amount out: {}\nPercentage gain: {}",
                    result.path_snapshot,
                    result.amount_in,
                    result.amount_out,
                    format_percent_bp(&result.pct_gain_bp),
                );
            }
            let candidates = tokio::task::spawn_blocking(move || {
                evaluate_batch_and_map_err(batch, &config)
            })
            .await??;
            for candidate in candidates {
                info!(
                    "Candidate {}: block_hash={}, input={}, output={}, estimated net profit={}, gross_gain_pct={}% (expected gas cost={}, maximum gas budget={})\n\n{}",
                    candidate.path_snapshot.id(),
                    block_hash,
                    candidate.amount_in,
                    candidate.amount_out,
                    candidate.net_profit,
                    format_percent_bp(&candidate.pct_gain_bp),
                    candidate.expected_gas_cost,
                    candidate.max_gas_cost,
                    candidate.path_snapshot
                );
            }
            Ok(())
        }
    });

    // This future owns the sender, so finishing or dropping it closes the queue.
    let forwarding = async move {
        let mut stream = _state_space_manager.subscribe()?;
        while let Some(batch) = stream.next().await {
            let batch = batch.map_err(|err| {
                log::error!("Indexer failed: {err:#?}");
                err
            })?;
            execution_tx.send(batch).await?;
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = forwarding => {
            // The sender is gone; let the engine drain its queue and finish.
            engine_task.await??;
            result
        }
        result = &mut engine_task => {
            result??;
            anyhow::bail!("Execution engine stopped while subscription was active")
        }
    }
}
