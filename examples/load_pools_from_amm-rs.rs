use std::sync::Arc;

use anyhow::{Ok, Result};
use itertools::Itertools;
use url::Url;
use amms::amms::{amm::AMM, uniswap_v2::UniswapV2Pool};
use log::{info};
use rust::constants::{
    Env, MIN_WETH_THRESHOLD, UNISWAP_V2_FACTORY_ADDRESS, UNISWAP_V3_FACTORY_ADDRESS, WEI, WETH_ADDRESS, WHITELIST_TOKENS
};

use alloy::{
    primitives::{Address, U256, address},
    providers::ProviderBuilder,
    rpc::client::ClientBuilder,
transports::layers::{RetryBackoffLayer, ThrottleLayer},
};

use amms::{
    amms::{uniswap_v2::UniswapV2Factory, uniswap_v3::UniswapV3Factory},
    state_space::{
        filters::{
            value::ValueFilter,
            whitelist::{PoolWhitelistFilter, TokenWhitelistFilter},
            PoolFilter,
        },
        StateSpaceBuilder,
    },
    sync,
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

    //let uniswapv2_factory_address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
    

     let client = ClientBuilder::default()
        .layer(ThrottleLayer::new(100))
        .layer(RetryBackoffLayer::new(5, 200, 330))
        .http(rpc_https_url);

    let provider = Arc::new(ProviderBuilder::new().connect_client(client));

    let factories = vec![
        // UniswapV2
        UniswapV2Factory::new(
            UNISWAP_V2_FACTORY_ADDRESS,
            300,
            10000835,
        )
        .into()
        // UniswapV3
        // UniswapV3Factory::new(
        //     UNISWAP_V3_FACTORY_ADDRESS,
        //     12369621,
        // )
        // .into(),
    ];

    let filters: Vec<PoolFilter> = vec![
        //PoolWhitelistFilter::new(vec![address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640")]).into(),
        TokenWhitelistFilter::new(WHITELIST_TOKENS.to_vec()).into(),
        ValueFilter::new(UNISWAP_V2_FACTORY_ADDRESS, UNISWAP_V3_FACTORY_ADDRESS, WETH_ADDRESS, U256::from(MIN_WETH_THRESHOLD), provider.clone()).into(),
    ];

    let _state_space_manager = sync!(factories, filters, provider);

    
    println!("Latest block: {:?}", _state_space_manager.latest_block.load(std::sync::atomic::Ordering::Relaxed));
    let state = _state_space_manager.state.read().await;
    //println!("Full State: {:#?}", &*state);
    // or if you only want the map:
    //println!("State Map Length: {}", state.state.keys().len());
    //println!("State Map: {:#?}", state.state);
    for p in state.state.values() {
        println!("Variant: {:#?}", p.variant());
    }
    let pools: Vec<&UniswapV2Pool> = state.state.values()
    .filter_map(|amm| match amm {
        AMM::UniswapV2Pool(pool) => Some(pool),
        _ => None,
    })
    .collect();
    let max_hops = 5;
    for p1 in pools.clone() {
        println!("----Pool---");
        println!("Pool: {:#?}", p1);
        println!("\n\n");
    }
    Ok(())
}