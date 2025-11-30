use super::Tool;
use crate::ethereum::EthereumClient;
use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::eth::TransactionRequest,
    sol,
    sol_types::SolCall,
};
use anyhow::Result;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

// Uniswap V3 QuoterV2 Interface
sol! {
    #[allow(missing_docs)]
    struct QuoteExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint24 fee;
        uint160 sqrtPriceLimitX96;
    }

    #[allow(missing_docs)]
    function quoteExactInputSingle(QuoteExactInputSingleParams memory params)
        external
        returns (
            uint256 amountOut,
            uint160 sqrtPriceX96After,
            uint32 initializedTicksCrossed,
            uint256 gasEstimate
        );
}

// Uniswap V3 SwapRouter Interface
sol! {
    #[allow(missing_docs)]
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    #[allow(missing_docs)]
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);
}

pub struct SwapTokensTool;

#[async_trait::async_trait]
impl Tool for SwapTokensTool {
    fn name(&self) -> &'static str {
        "swap_tokens"
    }

    fn description(&self) -> &'static str {
        "Simulate a token swap on Uniswap V3 and construct the transaction."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from_token": {
                    "type": "string",
                    "description": "Address of the token to sell"
                },
                "to_token": {
                    "type": "string",
                    "description": "Address of the token to buy"
                },
                "amount": {
                    "type": "string",
                    "description": "Amount of from_token to sell (in base units)"
                },
                "fee": {
                    "type": "integer",
                    "description": "Pool fee tier (e.g., 500, 3000, 10000). Default 3000."
                },
                "slippage_tolerance": {
                    "type": "number",
                    "description": "Slippage tolerance in percentage (e.g., 0.5 for 0.5%). Default 0.5."
                }
            },
            "required": ["from_token", "to_token", "amount"]
        })
    }

    async fn call(&self, client: &EthereumClient, args: Value) -> Result<Value> {
        let from_token = Address::from_str(
            args["from_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing from_token"))?,
        )?;
        let to_token = Address::from_str(
            args["to_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing to_token"))?,
        )?;
        let amount_in = U256::from_str(
            args["amount"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing amount"))?,
        )?;
        let fee = (args.get("fee").and_then(|v| v.as_u64()).unwrap_or(3000) as u32) & 0xFFFFFF; // clamp to uint24
        let slippage_percent = args
            .get("slippage_tolerance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);

        // Uniswap V3 QuoterV2 Address (Mainnet)
        let quoter_address = Address::from_str("0x61fFE0149A332c47d847296F720a48855e9cb754")?;
        // Uniswap V3 SwapRouter Address (Mainnet)
        let router_address = Address::from_str("0xE592427A0AEce92De3Edee1F18E0157C05861564")?;

        // 1. Simulate via Quoter to get estimated output
        let quote_call_data = quoteExactInputSingleCall {
            params: QuoteExactInputSingleParams {
                tokenIn: from_token,
                tokenOut: to_token,
                amountIn: amount_in,
                fee,
                sqrtPriceLimitX96: U256::ZERO,
            },
        }
        .abi_encode();

        let tx_req = TransactionRequest::default()
            .to(quoter_address)
            .input(quote_call_data.into());

        let result = client.provider.call(&tx_req).await?;
        let mut decode_error: Option<String> = None;
        let (amount_out, gas_estimate_quote) =
            match quoteExactInputSingleCall::abi_decode_returns(&result, true) {
                Ok(v) => (v.amountOut, v.gasEstimate),
                Err(e) => {
                    decode_error = Some(e.to_string());
                    (U256::ZERO, U256::ZERO)
                }
            };

        // 2. Calculate Minimum Output with Slippage
        let amount_out_decimal = Decimal::from_str(&amount_out.to_string())?;
        let slippage_decimal =
            Decimal::from_f64(slippage_percent).unwrap_or(Decimal::ZERO) / Decimal::from(100);
        let amount_out_min_decimal = amount_out_decimal * (Decimal::ONE - slippage_decimal);
        let amount_out_min_str = amount_out_min_decimal.floor().to_string();
        let amount_out_min = U256::from_str(&amount_out_min_str)?;

        // 3. Construct Real Transaction for Router
        let router_params = ExactInputSingleParams {
            tokenIn: from_token,
            tokenOut: to_token,
            fee,
            recipient: client.signer_address, // Send to self
            deadline: U256::MAX,              // No deadline for simulation
            amountIn: amount_in,
            amountOutMinimum: amount_out_min,
            sqrtPriceLimitX96: U256::ZERO,
        };

        let router_call_data = exactInputSingleCall {
            params: router_params,
        }
        .abi_encode();
        let router_call_hex = hex::encode(&router_call_data);

        // 4. Simulate the router transaction via eth_call (read-only)
        let router_sim_tx = TransactionRequest::default()
            .to(router_address)
            .from(client.signer_address)
            .input(router_call_data.clone().into());
        let router_simulation = match client.provider.call(&router_sim_tx).await {
            Ok(data) => {
                // If it succeeds, decode the returned amountOut.
                match exactInputSingleCall::abi_decode_returns(&data, true) {
                    Ok(sim_amount_out) => json!({
                        "status": "ok",
                        "simulated_amount_out": sim_amount_out.amountOut.to_string()
                    }),
                    Err(_) => json!({"status": "ok", "message": "call succeeded"}),
                }
            }
            Err(e) => json!({"status": "error", "message": e.to_string()}),
        };

        Ok(json!({
            "estimated_output": amount_out.to_string(),
            "minimum_output": amount_out_min.to_string(),
            "gas_estimate_simulation": gas_estimate_quote.to_string(),
            "transaction": {
                "to": router_address.to_string(),
                "data": format!("0x{}", router_call_hex),
                "value": "0", // Assuming ERC20 swap. If ETH, need to handle value.
                "description": "Uniswap V3 SwapRouter.exactInputSingle"
            },
            "router_call_simulation": router_simulation,
            "simulation_note": "Gas estimate is from Quoter. Router eth_call included; actual execution still depends on approvals/balance."
            , "quoter_decode_error": decode_error
        }))
    }
}
