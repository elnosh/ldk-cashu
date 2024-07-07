use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    /// Insufficient Funds
    #[error("insufficient funds")]
    InsufficientFunds,
    /// Insufficient inbound for swap
    #[error("insufficient inbound liquidity to make swap")]
    InsufficientInboundForSwap,
    /// Amount too low for channel
    #[error("amount too low to create channel")]
    AmountTooLowForChannel,
    /// Mint could not pay invoice
    #[error("mint could not pay invoice for swap")]
    MintCouldNotPayInvoice,
    /// Channel does not exist
    #[error("channel does not exist")]
    ChannelNotExist,
    /// LDK error
    #[error(transparent)]
    NodeStart(#[from] ldk_node::NodeError),
    /// CDK error
    #[error(transparent)]
    CdkError(#[from] cdk::wallet::error::Error),
    /// Reqwest error
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
}
