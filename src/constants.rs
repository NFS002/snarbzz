use alloy::primitives::{address, Address as HexAddress};
use ethers::{
    abi::Tokenizable,
    prelude::Lazy,
    types::{Address, H160, U256, U64},
};
use std::{ops::{Add, Mul}, str::FromStr};

pub const UNISWAP_V2_FACTORY_ADDRESS: HexAddress =
    address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
pub const UNISWAP_V3_FACTORY_ADDRESS: HexAddress =
    address!("1F98431c8aD98523631AE4a59f267346ea31F984");
pub const WETH_ADDRESS: HexAddress = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
pub const USDC_ADDRESS: HexAddress = address!("A0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
pub const USDT_ADDRESS: HexAddress = address!("dAC17F958D2ee523a2206206994597C13D831ec7");
pub const DAI_ADDRESS: HexAddress = address!("6B175474E89094C44Da98b954EedeAC495271d0F");
pub const WBTC_ADDRESS: HexAddress = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
pub const MIN_WETH_THRESHOLD: u128 = 10u128.pow(19); // 10 WETH (18 decimals)
pub const WETH_AMOUNT_IN: u128 = 5_800_000_000_000_000;


pub static WEI: Lazy<U256> = Lazy::new(|| U256::from(10).pow(U256::from(18)));
pub static GWEI: Lazy<U256> = Lazy::new(|| U256::from(10).pow(U256::from(9)));
pub static DUNE_QUERY_ID: u32 = 6572025;

pub static ZERO_ADDRESS: Lazy<Address> =
    Lazy::new(|| Address::from_str("0x0000000000000000000000000000000000000000").unwrap());

pub fn get_env(key: &str) -> String {
    std::env::var(key).unwrap()
}

// pub struct Erc20Addresses {
//     weth: Address,
//     usdc: Address,
//     usdt: Address,
//     wbtc: Address,
//     dai: Address,
// }

// pub mod erc20_addresses {

// }

#[derive(Debug, Clone)]
pub struct Env {
    pub https_url: String,
    pub wss_url: String,
    pub chain_id: U64,
    pub private_key: String,
    pub signing_key: String,
    pub bot_address: String,
    pub dune_api_key: String,
}

impl Env {
    pub fn new() -> Self {
        Env {
            https_url: get_env("HTTPS_URL"),
            wss_url: get_env("WSS_URL"),
            chain_id: U64::from_str(&get_env("CHAIN_ID")).unwrap(),
            private_key: get_env("PRIVATE_KEY"),
            signing_key: get_env("SIGNING_KEY"),
            bot_address: get_env("BOT_ADDRESS"),
            dune_api_key: get_env("DUNE_API_KEY"),
        }
    }
}


pub const WHITELIST_TOKENS: [HexAddress; 5] = [
    WETH_ADDRESS,
    USDT_ADDRESS,
    USDC_ADDRESS,
    DAI_ADDRESS,
    WBTC_ADDRESS
];


pub fn get_blacklist_tokens() -> Vec<H160> {
    vec!["0x9469603F3Efbcf17e4A5868d81C701BDbD222555"]
        .into_iter()
        .map(|addr| H160::from_str(addr).unwrap())
        .collect()
}

// Use later for broadcasting to multiple builders
// static BUILDER_URLS: &[&str] = &[
//     "https://builder0x69.io",
//     "https://rpc.beaverbuild.org",
//     "https://relay.flashbots.net",
//     "https://rsync-builder.xyz",
//     "https://rpc.titanbuilder.xyz",
//     "https://api.blocknative.com/v1/auction",
//     "https://mev.api.blxrbdn.com",
//     "https://eth-builder.com",
//     "https://builder.gmbit.co/rpc",
//     "https://buildai.net",
//     "https://rpc.payload.de",
//     "https://rpc.lightspeedbuilder.info",
//     "https://rpc.nfactorial.xyz",
// ];
