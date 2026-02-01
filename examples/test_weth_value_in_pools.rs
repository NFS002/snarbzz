use std::sync::Arc;

use anyhow::{Ok, Result};
use url::Url;
use log::{info};
use rust::constants::{
    Env, MIN_WETH_THRESHOLD, UNISWAP_V2_FACTORY_ADDRESS, UNISWAP_V3_FACTORY_ADDRESS, WEI, WETH_ADDRESS
};

use alloy::{
    primitives::{Address, U256, address}, providers::ProviderBuilder, rpc::client::ClientBuilder, sol_types::SolValue, transports::layers::{RetryBackoffLayer, ThrottleLayer}
};

use amms::{
    amms::{uniswap_v2::UniswapV2Factory, uniswap_v3::UniswapV3Factory},
    state_space::{
        StateSpaceBuilder, filters::{
            PoolFilter, value::{ValueFilter, WethValueInPools::{PoolInfo, PoolInfoReturn}, WethValueInPoolsBatchRequest}, whitelist::{PoolWhitelistFilter, TokenWhitelistFilter}
        }
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

    let pool_info1: PoolInfo = PoolInfo { poolType: 1, poolAddress: address!("4f9356293dda89a31a924fe1f867825b3adda4d8")};

    let pools: Vec<PoolInfo> = vec![pool_info1];

    let provider = Arc::new(ProviderBuilder::new().connect_client(client));
    let deployer = WethValueInPoolsBatchRequest::deploy_builder(
                        provider,
                        UNISWAP_V2_FACTORY_ADDRESS,
                        UNISWAP_V3_FACTORY_ADDRESS,
                        WETH_ADDRESS,
                        pools.clone(),
                    );

    println!("About to call");
    let res = deployer.call_raw().await?;
    let return_data = <Vec<PoolInfoReturn> as SolValue>::abi_decode(&res)?;
    println!("Called");
    for pool_info_return in return_data {
        println!("Pool info return: address={}, type={}, weth={}", pool_info_return.poolAddress, pool_info_return.poolType, pool_info_return.wethValue);
    }
    Ok(())
}