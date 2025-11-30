use anyhow::Context;
use std::env;

#[derive(Clone)]
pub struct Config {
    pub rpc_url: String,
    pub private_key: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let rpc_url = env::var("ETHEREUM_RPC_URL").context("ETHEREUM_RPC_URL must be set")?;
        let private_key = env::var("PRIVATE_KEY").context("PRIVATE_KEY must be set")?;

        Ok(Self {
            rpc_url,
            private_key,
        })
    }
}
