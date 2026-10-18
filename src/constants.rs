use alloy::primitives::{address, Address as HexAddress};
use ethers::{
    abi::Tokenizable,
    prelude::Lazy,
    types::{Address, H160, U256, U64},
};
use std::{
    ops::{Add, Mul},
    str::FromStr,
};

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

pub const HIGH_LIQUIDITY_TEST_POOLS: [HexAddress; 10] = [
    address!("0xA43fe16908251ee70EF74718545e4FE6C5cCEc9f"), // PEPE/ETH
    address!("0x52c77b0CB827aFbAD022E6d6CAF2C44452eDbc39"), // SPX/ETH
    address!("0x76A411f14A704099Ba476CE8dFFC288a53295218"), // ASTEROID/ETH
    address!("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc"), // ETH/USDC
    address!("0x0d4a11d5EEaaC28EC3F61d100daF4d40471f1852"), // ETH/USDT
    address!("0x0d4a11d5EEaaC28EC3F61d100daF4d40471f1852"), // NPC/ETH
    address!("0xC555D55279023E732CcD32D812114cAF5838fD46"), // Neiro/ETH
    address!("0x2621CC0B3F3c079c1Db0E80794AA24976F0b9e3c"), // SKY/USDS
    address!("0x9C4Fe5FFD9A9fC5678cFBd93Aa2D4FD684b67C4C"), // PAXG/ETH 
    address!("0xc2eaB7d33d3cB97692eCB231A5D0e4A649Cb539d") // Mog/ETH
];


// 26 pools, 10 WETH triangles, 20 directed three-hop paths.
pub const TEST_POOLS: [HexAddress; 26] = [
    address!("0x0d4a11d5eeaac28ec3f61d100daf4d40471f1852"), // WETH / USDT
    address!("0x132bc4ea9e5282889fdcfe7bc7a91ea901a686d6"), // 0xa9d54f37ebb99f83b603cc95fc1a5f3907aaccfd / WETH
    address!("0x17202e8e31fe0eebdb401b2e11492f5be52c8312"), // 0x2c9aceb63181cd08a093d052ec041e191f229692 / WETH
    address!("0x2345f02b6654fe58f70758009dcd116388d5105e"), // 0xa9d54f37ebb99f83b603cc95fc1a5f3907aaccfd / USDT
    address!("0x23d15edceb5b5b3a23347fa425846de80a2e8e5c"), // WETH / 0xeef9f339514298c6a857efcfc1a762af84438dee
    address!("0x25647e01bd0967c1b9599fa3521939871d1d0888"), // WETH / 0xe53ec727dbdeb9e2d5456c3be40cff031ab40a55
    address!("0x30e7e1989cf774726dd9f4780cab42d5558003e6"), // SAND / 0xe53ec727dbdeb9e2d5456c3be40cff031ab40a55
    address!("0x3da1313ae46132a397d90d95b1424a9a7e3e0fce"), // WETH / CRV
    address!("0x3dd49f67e9d5bc4c5e6634b3f70bfd9dc1b6bd74"), // SAND / WETH
    address!("0x582e3da39948c6339433008703211ad2c13eb2ac"), // WETH / 0xd233d1f6fd11640081abb8db125f722b5dc729dc
    address!("0x5fa9569b0ed6aa01e234468e6a15b77988b950df"), // 0x5732046a883704404f284ce41ffadd5b007fd668 / USDC
    address!("0x6a3d23fa07c455f88d70c29d230467c407a3964b"), // WETH / 0xe17f017475a709de58e976081eb916081ff4c9d5
    address!("0x6d7a7b251a6cac43f1798494d847e6d333f9ffca"), // 0x2c9aceb63181cd08a093d052ec041e191f229692 / USDT
    address!("0x71aa44cbed2ca17077aee7c5087e86a53fc01f6e"), // UNI / CRV
    address!("0x76f4500a099ec231abc9708313ffeff671248ec1"), // 0x691ace5e56507c008d836c8c58ed93546bdc81ff / 0xe17f017475a709de58e976081eb916081ff4c9d5
    address!("0x81a8bd7f2b29cee72aae18da9b4637acf4bc125a"), // LINK / 0xd233d1f6fd11640081abb8db125f722b5dc729dc
    address!("0x93e7dec988e24b08cc1606061b232deeb80ffbf8"), // 0x691ace5e56507c008d836c8c58ed93546bdc81ff / WETH
    address!("0x9bd82673c50acb4a3b883d61e070a3c8d9b08e10"), // DAI / 0xeef9f339514298c6a857efcfc1a762af84438dee
    address!("0xa16a3cbc92d77b720a851d33a91890c4fbfb0299"), // MKR / 0xd233d1f6fd11640081abb8db125f722b5dc729dc
    address!("0xa2107fa5b38d9bbd2c461d6edf11b11a50f6b974"), // LINK / WETH
    address!("0xa478c2975ab1ea89e8196811f51a7b7ade33eb11"), // DAI / WETH
    address!("0xb20bd5d04be54f870d5c0d3ca85d82b34b836405"), // DAI / USDT
    address!("0xb4e16d0168e52d35cacd2c6185b44281ec28c9dc"), // USDC / WETH
    address!("0xc2adda861f89bbb333c90c492cb837741916a225"), // MKR / WETH
    address!("0xcf4236db746dbc1855a4d095aaf58da9b030491e"), // 0x5732046a883704404f284ce41ffadd5b007fd668 / WETH
    address!("0xd3d2e2692501a5c9ca623199d38826e513033a17"), // UNI / WETH
];


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
    WBTC_ADDRESS,
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
