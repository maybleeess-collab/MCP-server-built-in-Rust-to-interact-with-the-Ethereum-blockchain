pub mod balance;
pub mod price;
pub mod swap;

use crate::ethereum::EthereumClient;
use serde_json::Value;

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> Value;
    async fn call(&self, client: &EthereumClient, args: Value) -> anyhow::Result<Value>;
}
