use std::{fs::File, io::Write, sync::Arc};

use anyhow::Result;
use futures::StreamExt;
use itertools::Itertools;
use url::Url;
use amms::amms::{amm::AMM, factory::Factory, path::find_arb_paths_v2, uniswap_v2::UniswapV2Pool};
use log::{info};
use rust::{constants::{
    Env, MIN_WETH_THRESHOLD, UNISWAP_V2_FACTORY_ADDRESS, UNISWAP_V3_FACTORY_ADDRESS, WEI, WETH_ADDRESS, WETH_AMOUNT_IN, WHITELIST_TOKENS
}, math::{format_percent_bp, percentage_change_bp}};

use alloy::{
    primitives::{Address, I256, U256, address},
    providers::{ProviderBuilder, WsConnect},
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
        //TokenWhitelistFilter::new(WHITELIST_TOKENS.to_vec()).into(),
        ValueFilter::new(UNISWAP_V2_FACTORY_ADDRESS, UNISWAP_V3_FACTORY_ADDRESS, WETH_ADDRESS, U256::from(MIN_WETH_THRESHOLD), http_provider.clone()).into(),
    ];

    //let _state_space_manager = sync!(factories, filters, provider);

    let _state_space_manager = Arc::new(StateSpaceBuilder::new(http_provider.clone())
        .from_cache("data/uniswapv2-pools.json".to_string())
        //.with_output_file("src/uniswap-pools.json".to_string())
        //.with_factories(factories)
        //.with_filters(filters)
        .with_pubsub_provider(wss_provider)
        .sync()
        .await?);

    let spreads_file = std::fs::File::options()
        .append(true)
        .create(true)
        .open("data/swaps.log")?;

    
    println!("Latest block: {:?}", _state_space_manager.latest_block.load(std::sync::atomic::Ordering::Relaxed));
    let mut stream = _state_space_manager.subscribe()?;
    while let Some(updated_amms) = stream.next().await {
        if let Ok(amms) = updated_amms {
            println!("Updated AMMs: {:?}", amms);
        }
    }

    /*
    The subscribe method listens for new blocks and fetches
    all logs matching any `sync_events()` specified by the AMM variants in the state space.
    Under the hood, this method applies all state changes to any affected AMMs and returns a Vec of
    addresses, indicating which AMMs have been updated.
    */
    // let mut stream = state_space_manager.subscribe().await?.take(5);
    // while let Some(updated_amms) = stream.next().await {
    //     if let Ok(amms) = updated_amms {
    //         println!("Updated AMMs: {:?}", amms);
    //     }
    // }

    //let state = _state_space_manager.state.read().await;
    //println!("Full State: {:#?}", &*state);
    // or if you only want the map:
    //println!("State Map Length: {}", state.state.keys().len());
    //println!("State Map: {:#?}", state.state);
    // let pools: Vec<&UniswapV2Pool> = state.state.values()
    // .filter_map(|amm| match amm {
    //     AMM::UniswapV2Pool(pool) => Some(pool),
    //     _ => None,
    // })
    // .collect();
    // for p1 in pools.clone() {
    //     println!("{:#?}\n\n", p1);
    // }
    // let amount_in = U256::from(WETH_AMOUNT_IN);
    // let paths = find_arb_paths_v2(pools.into_iter().cloned().collect(), WETH_ADDRESS);
    // for path in paths {
    //     let amount_out = path.simulate(amount_in).expect("Simulation failed");
    //     let pct_gain_bp =  percentage_change_bp(amount_in, amount_out).unwrap_or(I256::ZERO);
    //     // let (amount_out_surplus, overflow) = amount_out.overflowing_sub(U256::from(amount_in));
    //     // let pct_gain = if overflow {
    //     //     U256::ZERO
    //     // } else {
    //     //     amount_out_surplus * U256::from(10000) / U256::from(amount_in)
    //     // };
    //     println!("----Arb Path---");
    //     println!("Path: {:#?}", path);
    //     println!("Amount in: {}", amount_in);
    //     println!("Simulated amount out: {}", amount_out);
    //     println!("Percentage gain: {}%", format_percent_bp(pct_gain_bp));
    //     println!("\n\n");
    // }
    Ok(())
}