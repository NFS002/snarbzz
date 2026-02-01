use std::{path::Path, sync::Arc};

use anyhow::{Ok, Result};
use cfmms::{dex::{Dex, DexVariant as CfmmsDexVariant}, sync::sync_pairs, pool::Pool as CfmmsPool};
use ethers::types::H160;
use ethers_providers::{Provider, Ws};
use log::{info};
use rust::{constants::Env, pools::load_all_pools_from_v2, utils::setup_logger};
use std::str::FromStr;



#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    setup_logger()?;

    let env = Env::new();
    let file_path = Path::new("src/.cached-pools.csv");
    let checkpoint_path = Some("src/sync_pools_checkpoint.json");
    let factory_addresses = vec!["0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac"];
    let router_addresses = vec!["0xd9e1cE17f2641f24aE83637ab66a2cca9C378B9F"];
    let factory_blocks = vec![10794229u64];

    let ws = Ws::connect(env.wss_url.clone()).await?;
    let provider = Arc::new(Provider::new(ws));

    let mut dexes_data = Vec::new();

    for i in 0..factory_addresses.len() {
        dexes_data.push((
            factory_addresses[i].clone(),
            CfmmsDexVariant::UniswapV2,
            factory_blocks[i],
        ))
    }

    let dexes: Vec<_> = dexes_data
        .into_iter()
        .map(|(address, variant, number)| {
            Dex::new(
                H160::from_str(&address).unwrap(),
                variant,
                number,
                Some(3000),
            )
        })
        .collect();

    // let pools_vec = load_all_pools_from_v2(env.wss_url.clone(), factory_addresses, factory_blocks)
    //     .await
    //     .unwrap();
    let pools_vec: Vec<CfmmsPool> = sync_pairs(dexes.clone(), provider.clone(), checkpoint_path).await?;
    info!("Initial pool count: {}", pools_vec.len());
    Ok(())
}