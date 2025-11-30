# Ethereum Trading MCP Server

An MCP server built in Rust to interact with the Ethereum blockchain.

## Features

- **`get_balance`**: Query ETH and ERC20 token balances with proper decimal formatting.
- **`get_token_price`**: Get current token price in USD. Supports ETH via Chainlink and arbitrary tokens via Uniswap V3 (decimal-correct).
- **`swap_tokens`**: Simulate Uniswap V3 swaps, calculate minimum output with slippage, construct router calldata, and perform a read-only `eth_call` on the router.

## Prerequisites

- Rust (latest stable)
- An Ethereum RPC URL (e.g., from Alchemy or Infura)
- A private key (for signing transactions/simulations)

## Setup

1.  Clone the repository.
2.  Copy `.env.example` to `.env`:
    ```bash
    cp .env.example .env
    ```
3.  Edit `.env` and set your `ETHEREUM_RPC_URL` and `PRIVATE_KEY`.

## Usage

### Building

```bash
cargo build --release
```

### Running

The server uses Stdio for MCP communication.

```bash
cargo run
```

### Example MCP Tool Calls

#### `get_balance`
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "get_balance",
    "arguments": {
      "address": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      "token_address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
    }
  },
  "id": 1
}
```

#### `get_token_price` (Arbitrary Token)
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "get_token_price",
    "arguments": {
      "token_symbol": "UNI",
      "token_address": "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"
    }
  },
  "id": 2
}
```

#### `swap_tokens` (Simulation & Construction)
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "swap_tokens",
    "arguments": {
      "from_token": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", 
      "to_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
      "amount": "1000000000000000000",
      "slippage_tolerance": 0.5
    }
  },
  "id": 3
}
```

## Design Decisions

- **Client choice**: Alloy is used for its performance, strong typing, and lack of heavy codegen requirements.
- **Async/runtime**: Tokio + async everywhere to match RPC-bound workloads; tools are `Send + Sync` for concurrent handling.
- **Precision**: `rust_decimal` is used end-to-end for price/balance math to avoid float drift; Q96 is a fixed decimal constant to prevent overflow.
- **Uniswap V3**: Selected to cover the majority of mainnet liquidity and fee tiers efficiently.
- **Swap simulation**: Two-step process: QuoterV2 `eth_call` for amounts/gas, followed by a router `eth_call` with real calldata to surface approval/funding issues.
- **Calldata exposure**: `swap_tokens` returns router calldata so agents can sign/broadcast; simulation is read-only.
- **Decimals handling**: ERC20 `decimals()` fetched for price/balance; 10^decimals built with decimal-safe helper to avoid overflow.
- **MCP shape**: Hybrid response (`content` for strict MCP, `data` for structured consumption) to maximize compatibility and programmatic usability.
- **Error transparency**: Router simulation errors are bubbled back in `router_call_simulation` instead of being swallowed.
- **Scope**: Focused on Uniswap V3 and ERC20-to-ERC20 swaps.

## Limitations

- **Gas Estimation**: The returned gas estimate is for the simulation (Quoter) and may differ from the actual swap transaction gas.
- **Price Impact**: The price tool calculates spot price from `slot0` and does not account for price impact of large trades (though `swap_tokens` simulation does).
- **Approvals/Funding**: Router `eth_call` may revert if the signer lacks allowances or balance; this is surfaced in the response but not auto-resolved.
- **Coverage**: Only Uniswap V3 path is implemented (no V2), and swaps assume ERC20->ERC20 (ETH wrapping/unwrapping not included).

## Error Response Examples

### Router simulation revert (missing allowance/balance)
```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [{ "type": "text", "text": "{...}" }],
    "data": {
      "estimated_output": "123",
      "minimum_output": "122",
      "transaction": { "to": "0xE5924...", "data": "0x...", "value": "0" },
      "router_call_simulation": {
        "status": "error",
        "message": "execution reverted: TransferHelper: TRANSFER_FROM_FAILED"
      }
    }
  },
  "id": 1
}
```

### Unknown token symbol without address
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32603,
    "message": "Tool execution failed: Unknown token symbol. Please provide token_address."
  },
  "id": 2
}
```
