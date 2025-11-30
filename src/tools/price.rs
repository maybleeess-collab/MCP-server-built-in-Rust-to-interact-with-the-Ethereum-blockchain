use super::Tool;
use crate::ethereum::EthereumClient;
use alloy::{
    primitives::{Address, I256},
    providers::Provider,
    sol,
    sol_types::SolCall,
};
use anyhow::Result;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

// Simplified Chainlink Aggregator Interface
sol! {
    #[allow(missing_docs)]
    function latestAnswer() external view returns (int256);
    #[allow(missing_docs)]
    function decimals() external view returns (uint8); // Used for Chainlink + ERC20 decimal fetch
}

// Uniswap V3 Factory Interface
sol! {
    #[allow(missing_docs)]
    function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool);
}

// Uniswap V3 Pool Interface (slot0)
sol! {
    #[allow(missing_docs)]
    function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked);
    #[allow(missing_docs)]
    function token0() external view returns (address);
}

pub struct GetTokenPriceTool;

#[async_trait::async_trait]
impl Tool for GetTokenPriceTool {
    fn name(&self) -> &'static str {
        "get_token_price"
    }

    fn description(&self) -> &'static str {
        "Get the current price of a token in USD or ETH. Uses Chainlink for ETH/USD and Uniswap V3 for others."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_symbol": {
                    "type": "string",
                    "description": "Symbol of the token (e.g., ETH, USDC, UNI)"
                },
                "token_address": {
                    "type": "string",
                    "description": "Address of the token (required for non-standard tokens)"
                }
            },
            "required": ["token_symbol"]
        })
    }

    async fn call(&self, client: &EthereumClient, args: Value) -> Result<Value> {
        let symbol = args["token_symbol"]
            .as_str()
            .unwrap_or("ETH")
            .to_uppercase();
        let token_address_str = args.get("token_address").and_then(|v| v.as_str());

        // 1. ETH Price via Chainlink
        if symbol == "ETH" {
            let eth_price = self.get_eth_price_chainlink(client).await?;
            return Ok(json!({
                "symbol": "ETH",
                "price_usd": eth_price,
                "price_eth": Decimal::ONE,
                "source": "Chainlink Oracle"
            }));
        }

        // 2. Resolve Token Address
        let token_address = if let Some(addr) = token_address_str {
            Address::from_str(addr)?
        } else {
            // Common token mappings
            match symbol.as_str() {
                "USDC" => Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")?,
                "WETH" => Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")?,
                "WBTC" => Address::from_str("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599")?,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unknown token symbol. Please provide token_address."
                    ))
                }
            }
        };

        // 3. Get Price via Uniswap V3 (Token/ETH or Token/USDC)
        // Find pool against WETH.
        let weth_address = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")?;
        let factory_address = Address::from_str("0x1F98431c8aD98523631AE4a59f267346ea31F984")?; // Uniswap V3 Factory

        // Try 0.3% fee tier (3000)
        let fee = 3000;
        let get_pool_data = getPoolCall {
            tokenA: token_address,
            tokenB: weth_address,
            fee,
        }
        .abi_encode();
        let tx_req = alloy::rpc::types::eth::TransactionRequest::default()
            .to(factory_address)
            .input(get_pool_data.into());

        let pool_res = client.provider.call(&tx_req).await?;
        let pool_return = getPoolCall::abi_decode_returns(&pool_res, true)?;
        let pool_address: Address = pool_return.pool;

        if pool_address == Address::ZERO {
            return Err(anyhow::anyhow!(
                "No Uniswap V3 pool found for {}/WETH (0.3%)",
                symbol
            ));
        }

        // Get slot0 (sqrtPriceX96)
        let slot0_data = slot0Call {}.abi_encode();
        let slot0_req = alloy::rpc::types::eth::TransactionRequest::default()
            .to(pool_address)
            .input(slot0_data.into());
        let slot0_res = client.provider.call(&slot0_req).await?;
        let slot0_return = slot0Call::abi_decode_returns(&slot0_res, true)?;
        let sqrt_price_x96 = slot0_return.sqrtPriceX96;

        // Check token0 order to calculate price correctly
        let token0_data = token0Call {}.abi_encode();
        let token0_req = alloy::rpc::types::eth::TransactionRequest::default()
            .to(pool_address)
            .input(token0_data.into());
        let token0_res = client.provider.call(&token0_req).await?;
        let token0_return = token0Call::abi_decode_returns(&token0_res, true)?;
        let token0: Address = token0_return._0;

        // Fetch decimals for token and WETH to adjust the price correctly.
        let token_decimals = self.get_erc20_decimals(client, token_address).await?;
        let weth_decimals = self.get_erc20_decimals(client, weth_address).await?;

        // price1 / price0 = (sqrtPriceX96 / 2^96)^2 * 10^(dec0 - dec1)
        // Where token0/token1 follow the pool order.
        // Avoid overflowing Decimal by dividing down by 2^96 in smaller steps (2^32 * 2^32 * 2^32).
        let sqrt_price = Decimal::from_str(&sqrt_price_x96.to_string())?;
        let q32 = Decimal::from(4_294_967_296u64); // 2^32 fits comfortably
        let sqrt_ratio = sqrt_price / q32 / q32 / q32; // sqrtPriceX96 / 2^96
        let mut price_ratio = sqrt_ratio * sqrt_ratio;

        // Decimal adjustment for differing token decimals
        let decimal_adjust = pow10_decimal(i32::from(token_decimals) - i32::from(weth_decimals))?;
        price_ratio *= decimal_adjust;

        let price_in_eth = if token0 == token_address {
            // token0 = token, token1 = WETH -> price_ratio is WETH per token
            price_ratio
        } else {
            // token0 = WETH, token1 = token -> invert
            Decimal::ONE / price_ratio
        };

        let eth_price_usd = self.get_eth_price_chainlink(client).await?;
        let price_usd = price_in_eth * eth_price_usd;

        Ok(json!({
            "symbol": symbol,
            "price_eth": price_in_eth,
            "price_usd": price_usd,
            "source": "Uniswap V3 (Derived from ETH pair)",
            "pool_fee": fee,
            "pool": pool_address
        }))
    }
}

impl GetTokenPriceTool {
    async fn get_eth_price_chainlink(&self, client: &EthereumClient) -> Result<Decimal> {
        let price_feed_address = Address::from_str("0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419")?; // Mainnet ETH/USD

        let call_data = latestAnswerCall {}.abi_encode();
        let tx_req = alloy::rpc::types::eth::TransactionRequest::default()
            .to(price_feed_address)
            .input(call_data.into());

        let result = client.provider.call(&tx_req).await?;
        let price_raw: I256 = latestAnswerCall::abi_decode_returns(&result, true)?._0;

        let decimals_data = decimalsCall {}.abi_encode();
        let decimals_req = alloy::rpc::types::eth::TransactionRequest::default()
            .to(price_feed_address)
            .input(decimals_data.into());
        let decimals_res = client.provider.call(&decimals_req).await?;
        let decimals: u8 = decimalsCall::abi_decode_returns(&decimals_res, true)?._0;

        let price_decimal = Decimal::from_str(&price_raw.to_string())?;
        let scale = pow10_decimal(decimals as i32)?;
        let price_usd = price_decimal / scale;

        Ok(price_usd)
    }

    async fn get_erc20_decimals(&self, client: &EthereumClient, token: Address) -> Result<u8> {
        let decimals_data = decimalsCall {}.abi_encode();
        let decimals_req = alloy::rpc::types::eth::TransactionRequest::default()
            .to(token)
            .input(decimals_data.into());
        let decimals_res = client.provider.call(&decimals_req).await?;
        let decimals: u8 = decimalsCall::abi_decode_returns(&decimals_res, true)?._0;
        Ok(decimals)
    }
}

fn pow10_decimal(exp: i32) -> Result<Decimal> {
    if exp == 0 {
        return Ok(Decimal::ONE);
    }
    if exp < 0 {
        let positive = pow10_decimal(-exp)?;
        return Ok(Decimal::ONE / positive);
    }

    let exp_usize = usize::try_from(exp).unwrap_or(0);
    let s = format!("1{}", "0".repeat(exp_usize));
    Ok(Decimal::from_str(&s)?)
}
