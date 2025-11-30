use super::Tool;
use crate::ethereum::EthereumClient;
use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    sol,
    sol_types::SolCall,
};
use anyhow::Result;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

sol! {
    #[allow(missing_docs)]
    function balanceOf(address account) external view returns (uint256);
    #[allow(missing_docs)]
    function decimals() external view returns (uint8);
    #[allow(missing_docs)]
    function symbol() external view returns (string);
}

pub struct GetBalanceTool;

#[async_trait::async_trait]
impl Tool for GetBalanceTool {
    fn name(&self) -> &'static str {
        "get_balance"
    }

    fn description(&self) -> &'static str {
        "Get the balance of ETH or an ERC20 token for a specific address"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "The wallet address to check balance for"
                },
                "token_address": {
                    "type": "string",
                    "description": "Optional ERC20 token contract address. If omitted, returns ETH balance."
                }
            },
            "required": ["address"]
        })
    }

    async fn call(&self, client: &EthereumClient, args: Value) -> Result<Value> {
        let address_str = args["address"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing address"))?;
        let address = Address::from_str(address_str)?;

        let token_address_str = args.get("token_address").and_then(|v| v.as_str());

        if let Some(token_addr_str) = token_address_str {
            // ERC20 Balance
            let token_address = Address::from_str(token_addr_str)?;

            let call_data = balanceOfCall { account: address }.abi_encode();

            let tx_req = alloy::rpc::types::eth::TransactionRequest::default()
                .to(token_address)
                .input(call_data.into());

            let result = client.provider.call(&tx_req).await?;
            let balance: U256 = balanceOfCall::abi_decode_returns(&result, true)?._0;

            // Get decimals
            let decimals_data = decimalsCall {}.abi_encode();
            let decimals_req = alloy::rpc::types::eth::TransactionRequest::default()
                .to(token_address)
                .input(decimals_data.into());
            let decimals_res = client.provider.call(&decimals_req).await?;
            let decimals: u8 = decimalsCall::abi_decode_returns(&decimals_res, true)?._0;

            // Get symbol
            let symbol_data = symbolCall {}.abi_encode();
            let symbol_req = alloy::rpc::types::eth::TransactionRequest::default()
                .to(token_address)
                .input(symbol_data.into());
            let symbol_res = client.provider.call(&symbol_req).await?;
            let symbol: String = symbolCall::abi_decode_returns(&symbol_res, true)?._0;

            let formatted = format_units(balance, decimals)?;

            Ok(json!({
                "balance": formatted,
                "raw_balance": balance.to_string(),
                "symbol": symbol,
                "decimals": decimals
            }))
        } else {
            // ETH Balance
            let balance = client.provider.get_balance(address).await?;
            let formatted = format_units(balance, 18)?;

            Ok(json!({
                "balance": formatted,
                "raw_balance": balance.to_string(),
                "symbol": "ETH",
                "decimals": 18
            }))
        }
    }
}

fn format_units(value: U256, decimals: u8) -> Result<String> {
    let s = value.to_string();
    let d = Decimal::from_str(&s)?;
    let scale = pow10_decimal(decimals as i32)?;
    Ok((d / scale).normalize().to_string())
}

fn pow10_decimal(exp: i32) -> Result<Decimal> {
    if exp < 0 {
        let positive = pow10_decimal(-exp)?;
        return Ok(Decimal::ONE / positive);
    }

    let exp_usize = usize::try_from(exp).unwrap_or(0);
    let s = format!("1{}", "0".repeat(exp_usize));
    Ok(Decimal::from_str(&s)?)
}
