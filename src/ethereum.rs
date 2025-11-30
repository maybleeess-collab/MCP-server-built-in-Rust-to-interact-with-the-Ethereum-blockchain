use alloy::{
    network::EthereumWallet, primitives::Address, providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
};
use anyhow::Result;
use std::str::FromStr;
use url::Url;

#[derive(Clone)]
pub struct EthereumClient {
    pub provider: alloy::providers::RootProvider<
        alloy::transports::http::Http<alloy::transports::http::Client>,
    >,
    pub wallet: EthereumWallet,
    pub signer_address: Address,
}

impl EthereumClient {
    pub async fn new(rpc_url: &str, private_key: &str) -> Result<Self> {
        let signer = PrivateKeySigner::from_str(private_key)?;
        let signer_address = signer.address();
        let wallet = EthereumWallet::from(signer);

        let url = Url::parse(rpc_url)?;
        let provider = ProviderBuilder::new().on_http(url);

        Ok(Self {
            provider,
            wallet,
            signer_address,
        })
    }
}
