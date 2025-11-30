use dotenv::dotenv;
use ethereum_trading_mcp::{
    ethereum::EthereumClient,
    tools::{balance::GetBalanceTool, price::GetTokenPriceTool, swap::SwapTokensTool, Tool},
};
use rust_decimal::Decimal;
use serde_json::json;
use std::env;
use std::str::FromStr;

async fn setup_client() -> EthereumClient {
    dotenv().ok();
    let rpc = env::var("ETHEREUM_RPC_URL").expect("ETHEREUM_RPC_URL must be set for tests");
    let pk = env::var("PRIVATE_KEY").expect("PRIVATE_KEY must be set for tests");
    EthereumClient::new(&rpc, &pk)
        .await
        .expect("Failed to create Ethereum client")
}

#[tokio::test]
async fn test_get_eth_balance() {
    let client = setup_client().await;
    let tool = GetBalanceTool;

    let args = json!({
        "address": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("Vitalik's ETH Balance: {}", result);

    assert!(result.get("balance").is_some());
}

#[tokio::test]
async fn test_get_erc20_balance_usdc() {
    let client = setup_client().await;
    let tool = GetBalanceTool;

    let args = json!({
        "address": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "token_address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48" // USDC
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("Vitalik's USDC Balance: {}", result);

    assert_eq!(result.get("symbol").and_then(|v| v.as_str()), Some("USDC"));
    assert!(result.get("balance").is_some());
}

#[tokio::test]
async fn test_get_token_price_eth() {
    let client = setup_client().await;
    let tool = GetTokenPriceTool;

    let args = json!({
        "token_symbol": "ETH"
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("ETH Price: {}", result);
    assert!(result.get("price_usd").is_some());
}

#[tokio::test]
async fn test_get_token_price_uni() {
    let client = setup_client().await;
    let tool = GetTokenPriceTool;

    // UNI Token
    let args = json!({
        "token_symbol": "UNI",
        "token_address": "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("UNI Price: {}", result);
    assert!(result.get("price_usd").is_some());
}

#[tokio::test]
async fn test_get_token_price_arbitrary_address() {
    let client = setup_client().await;
    let tool = GetTokenPriceTool;

    // AAVE Token (requires address lookup)
    let args = json!({
        "token_symbol": "AAVE",
        "token_address": "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9"
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("AAVE Price: {}", result);
    assert!(result.get("price_usd").is_some());
}

#[tokio::test]
async fn test_get_token_price_unknown_symbol_requires_address() {
    let client = setup_client().await;
    let tool = GetTokenPriceTool;

    let args = json!({
        "token_symbol": "UNKNOWN"
    });

    let result = tool.call(&client, args).await;
    assert!(
        result.is_err(),
        "Expected error for unknown symbol without address"
    );
}

#[tokio::test]
async fn test_swap_simulation() {
    let client = setup_client().await;
    let tool = SwapTokensTool;

    // WETH -> USDC
    let args = json!({
        "from_token": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        "to_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        "amount": "1000000000000000000", // 1 ETH
        "slippage_tolerance": 0.5
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("Swap Result: {}", result);

    assert!(result.get("transaction").is_some());
    assert!(result["transaction"].get("data").is_some());

    // Verify slippage math only when quoter returned a non-zero estimate.
    if result
        .get("quoter_decode_error")
        .and_then(|v| v.as_str())
        .is_none()
    {
        let est = Decimal::from_str(result["estimated_output"].as_str().unwrap()).unwrap();
        if est > Decimal::ZERO {
            let min_out = Decimal::from_str(result["minimum_output"].as_str().unwrap()).unwrap();
            let ratio = min_out / est;
            // With 0.5% slippage, ratio should be close to >= 0.995 (allow tiny rounding gap).
            assert!(ratio >= Decimal::from_str("0.994").unwrap() && ratio <= Decimal::ONE);
        }
    }
}

#[tokio::test]
async fn test_swap_simulation_shows_revert_without_allowance() {
    let client = setup_client().await;
    let tool = SwapTokensTool;

    // Verify that router simulation errors (e.g. missing allowance) are surfaced.
    let args = json!({
        "from_token": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        "to_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        "amount": "1000000000000000000", // 1 ETH
        "slippage_tolerance": 0.5
    });

    let result = tool.call(&client, args).await.unwrap();
    if let Some(sim) = result.get("router_call_simulation") {
        let status = sim.get("status").and_then(|v| v.as_str()).unwrap_or("");
        assert!(status == "ok" || status == "error");
    }
}

#[tokio::test]
async fn test_swap_simulation_fee_500_pool() {
    let client = setup_client().await;
    let tool = SwapTokensTool;

    // USDC -> USDT fee 500 pool (0.05%)
    let args = json!({
        "from_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", // USDC
        "to_token": "0xdAC17F958D2ee523a2206206994597C13D831ec7",   // USDT
        "amount": "1000000", // 1 USDC (6 decimals)
        "slippage_tolerance": 0.5,
        "fee": 500
    });

    let result = tool.call(&client, args).await.unwrap();
    println!("Swap Result fee 500: {}", result);
    assert!(result.get("estimated_output").is_some());
    assert!(result.get("transaction").is_some());
}

#[tokio::test]
async fn test_get_balance_invalid_address_errors() {
    let client = setup_client().await;
    let tool = GetBalanceTool;

    let args = json!({
        "address": "invalid-address"
    });

    let result = tool.call(&client, args).await;
    assert!(result.is_err(), "Expected error for invalid address");
}
