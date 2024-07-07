use std::path::Path;
use std::process::exit;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use cdk::nuts::MeltQuoteState;
use cdk::wallet::Wallet;
use cdk::{Amount, Bolt11Invoice};
use cdk_redb::WalletRedbDatabase;
use hex_conservative::DisplayHex;
use ldk_node::bitcoin::address::NetworkUnchecked;
use ldk_node::bitcoin::{Address, Network, OutPoint, Txid};
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::{AnchorChannelsConfig, Builder, ChannelDetails, Config, Node, UserChannelId};
use rand::Rng;
use secp256k1::PublicKey;
use serde::Serialize;
use tokio::time::{sleep, timeout};

use crate::error::Error;
use crate::lsp::LspClient;

const DB_PATH: &str = "./walletdb";
const MIN_CHANNEL_OPENING_SAT: u64 = 1_000_000;

#[derive(Clone, Serialize)]
pub struct Balance {
    pub cashu_balance: u64,
    pub lightning_balance: u64,
    pub onchain_balance: u64,
    pub spendable_onchain_balance: u64,
}

#[derive(Clone, Serialize)]
pub struct ChannelInfo {
    channel_id: String,
    counterparty_node_id: PublicKey,
    funding_txo: Option<OutPoint>,
    channel_value_sats: u64,
    unspendable_punishment_reserve: Option<u64>,
    user_channel_id: String,
    outbound_capacity_sat: u64,
    inbound_capacity_sat: u64,
    is_channel_ready: bool,
}

impl From<&ChannelDetails> for ChannelInfo {
    fn from(value: &ChannelDetails) -> Self {
        let channel_id = value.channel_id.0.to_lower_hex_string();
        let user_channel_id = value.user_channel_id.0.to_be_bytes().to_lower_hex_string();

        let outbound_capacity_sat = value.outbound_capacity_msat / 1000;
        let inbound_capacity_sat = value.inbound_capacity_msat / 1000;

        ChannelInfo {
            channel_id,
            counterparty_node_id: value.counterparty_node_id,
            funding_txo: value.funding_txo,
            channel_value_sats: value.channel_value_sats,
            unspendable_punishment_reserve: value.unspendable_punishment_reserve,
            user_channel_id,
            outbound_capacity_sat,
            inbound_capacity_sat,
            is_channel_ready: value.is_channel_ready,
        }
    }
}

#[derive(Clone)]
pub struct LnCashuWallet {
    cashu: Wallet,
    lightning_node: Arc<Node>,
    lsp_client: LspClient,
}

impl LnCashuWallet {
    pub fn new() -> Self {
        let seed = rand::thread_rng().gen::<[u8; 32]>();
        let path = Path::new(DB_PATH);
        let cashu_db = Arc::new(WalletRedbDatabase::new(path).unwrap());

        // hard-coding this now. should get from info endpoint
        let trusted_peer = PublicKey::from_str(
            "032ae843e4d7d177f151d021ac8044b0636ec72b1ce3ffcde5c04748db2517ab03",
        )
        .unwrap();

        let anchor_channel_config = AnchorChannelsConfig {
            trusted_peers_no_reserve: vec![trusted_peer],
            ..Default::default()
        };

        let config = Config {
            trusted_peers_0conf: vec![trusted_peer],
            anchor_channels_config: Some(anchor_channel_config),
            ..Default::default()
        };

        let mut builder = Builder::from_config(config);

        //let mut builder = Builder::new();
        builder.set_network(Network::Signet);
        builder.set_esplora_server("https://mutinynet.com/api".to_string());
        builder.set_gossip_source_p2p();
        builder.set_log_level(ldk_node::LogLevel::Trace);
        builder.set_log_dir_path("./logs".to_string());
        builder.set_storage_dir_path("./ldk-storage".to_string());

        let node = Arc::new(builder.build().unwrap());

        LnCashuWallet {
            cashu: Wallet::new(
                "https://cashu.mutinynet.com",
                cdk::nuts::CurrencyUnit::Sat,
                cashu_db,
                &seed,
            ),
            lightning_node: node,
            lsp_client: LspClient::default(),
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        self.lightning_node.start().unwrap();

        // hard-coding this now. should get from info endpoint
        let trusted_peer = PublicKey::from_str(
            "032ae843e4d7d177f151d021ac8044b0636ec72b1ce3ffcde5c04748db2517ab03",
        )
        .unwrap();
        let address = SocketAddress::from_str("45.79.201.241:9735").unwrap();

        let _ = self
            .lightning_node
            .connect(trusted_peer, address, true)
            .unwrap();

        // TODO: start thread for checking events from ldk

        Ok(())
    }

    pub async fn balance(&self) -> Result<Balance, Error> {
        let cashu_balance = self.cashu.total_balance().await?;
        let ln_node_balance = self.lightning_node.list_balances();

        Ok(Balance {
            cashu_balance: cashu_balance.into(),
            lightning_balance: ln_node_balance.total_lightning_balance_sats,
            onchain_balance: ln_node_balance.total_onchain_balance_sats,
            spendable_onchain_balance: ln_node_balance.spendable_onchain_balance_sats,
        })
    }

    // dependending on liquidity receive through cashu or lightning node
    pub async fn receive(self, amt: u64) -> Result<Bolt11Invoice, Error> {
        // if enough inbound, get invoice from lightning node
        if self.inbound_for_amount(amt) {
            let invoice = self
                .lightning_node
                .bolt11_payment()
                .receive(amt * 1000, "", 3600)?;

            return Ok(invoice);
        } else {
            // if no inbound liquidity, get invoice from cashu wallet
            let mint_quote = self.cashu.mint_quote(Amount::from(amt)).await.unwrap();
            let invoice = Bolt11Invoice::from_str(&mint_quote.request).unwrap();

            // spawn thread for couple of minutes that will constantly check invoice
            // if invoice does not get paid in the window
            // add endpoint that will try to mint unclaimed quotes
            tokio::spawn(timeout(Duration::from_secs(180), async move {
                loop {
                    let quote_status = self.cashu.mint_quote_state(&mint_quote.id).await.unwrap();

                    // TODO: use state field instead of paid
                    if quote_status.paid.unwrap() {
                        // try mint
                        let amount = self
                            .cashu
                            .mint(&mint_quote.id, cdk::amount::SplitTarget::None, None)
                            .await
                            .unwrap();

                        println!("minted {amount} sats!");
                        return;
                    }
                    sleep(Duration::from_secs(10)).await;
                }
            }));

            return Ok(invoice);
        }
    }

    pub async fn receive_ecash(&self, token: String) -> Result<u64, Error> {
        let amount = self
            .cashu
            .receive(
                token.as_str(),
                &cdk::amount::SplitTarget::None,
                &vec![],
                &vec![],
            )
            .await?;

        Ok(amount.into())
    }

    pub async fn pay_invoice(&self, invoice: Bolt11Invoice) -> Result<String, Error> {
        // TODO: amountless invoices

        let invoice_amount = invoice.amount_milli_satoshis().unwrap() / 1000;
        let balance = self.balance().await?;

        // try to pay invoice from cashu wallet first
        if balance.cashu_balance > invoice_amount {
            let melt_quote = self.cashu.melt_quote(invoice.to_string(), None).await?;
            let melt = self
                .cashu
                .melt(melt_quote.id.as_str(), cdk::amount::SplitTarget::None)
                .await?;

            // if mint could not pay invoice, try lightning node
            match melt.state {
                MeltQuoteState::Paid => return Ok(melt.preimage.unwrap()),
                _ => {}
            }
        }

        if balance.lightning_balance > invoice_amount {
            let payment = self.lightning_node.bolt11_payment().send(&invoice)?;
            return Ok(payment.0.to_lower_hex_string());
        }

        return Err(Error::InsufficientFunds);
    }

    pub async fn send_ecash(&self, amount_sats: u64) -> Result<String, Error> {
        // TODO: why this send method returns a string instead of a Token
        let token = self
            .cashu
            .send(
                Amount::from(amount_sats),
                None,
                None,
                &cdk::amount::SplitTarget::None,
            )
            .await?;

        Ok(token)
    }

    // swap (from cashu to ln node via jit channel or regular invoice if enough liquidity)
    pub async fn swap(&self, target_amount_sats: u64) -> Result<(), Error> {
        // TODO: have some config that sets the minimum amount for a channel opening
        // to avoid opening small channels

        let balance = self.balance().await?;
        if balance.cashu_balance < target_amount_sats {
            return Err(Error::InsufficientFunds);
        }

        let invoice = if self.inbound_for_amount(target_amount_sats) {
            let invoice = self.lightning_node.bolt11_payment().receive(
                target_amount_sats * 1000,
                "",
                3600,
            )?;
            invoice
        } else {
            // if amount wanting to be swapped is above the minimum target for channel openings
            // then create invoice that when payed will create a JIT channel from the lsp

            if target_amount_sats > MIN_CHANNEL_OPENING_SAT {
                let target_amount_msat = target_amount_sats * 1000;

                let fee_response = self
                    .lsp_client
                    .lsp_fee(target_amount_msat, self.lightning_node.node_id())
                    .await?;

                // create invoice for amount minus lsp fees
                let node_invoice = self.lightning_node.bolt11_payment().receive(
                    target_amount_msat - fee_response.fee_amount_msat,
                    "",
                    3600,
                )?;

                let wrapped_lsp_invoice = self
                    .lsp_client
                    .get_lsp_wrapped_invoice(fee_response.id, node_invoice)
                    .await?;

                wrapped_lsp_invoice
            } else {
                if target_amount_sats < MIN_CHANNEL_OPENING_SAT {
                    return Err(Error::AmountTooLowForChannel);
                } else {
                    return Err(Error::InsufficientInboundForSwap);
                }
            }
        };

        // try melt from cashu wallet
        let melt_quote = self.cashu.melt_quote(invoice.to_string(), None).await?;
        let melt = self
            .cashu
            .melt(melt_quote.id.as_str(), cdk::amount::SplitTarget::None)
            .await?;

        match melt.state {
            MeltQuoteState::Paid => return Ok(()),
            _ => return Err(Error::MintCouldNotPayInvoice),
        }
    }

    fn inbound_for_amount(&self, amount_sat: u64) -> bool {
        let channels = self.lightning_node.list_channels();
        for channel in channels.iter() {
            if (channel.inbound_capacity_msat / 1000) > amount_sat {
                return true;
            }
        }
        false
    }

    pub fn new_address(&self) -> Result<Address, Error> {
        let address = self.lightning_node.onchain_payment().new_address()?;
        Ok(address)
    }

    pub fn open_channel(
        &self,
        amount_sats: u64,
        node_pubkey: Option<PublicKey>,
        node_address: Option<SocketAddress>,
    ) -> Result<String, Error> {
        // if pubkey or node address not specified, open channel to faucet

        let node_pubkey = node_pubkey.unwrap_or(
            PublicKey::from_str(
                "02465ed5be53d04fde66c9418ff14a5f2267723810176c9212b722e542dc1afb1b",
            )
            .unwrap(),
        );

        let node_address =
            node_address.unwrap_or(SocketAddress::from_str("45.79.52.207:9735").unwrap());

        let channel_id = self.lightning_node.connect_open_channel(
            node_pubkey,
            node_address,
            amount_sats,
            None,
            None,
            true,
        )?;
        let channel_id = channel_id.0.to_be_bytes().to_lower_hex_string();

        Ok(channel_id)
    }

    pub fn close_channel(&self, channel_id: UserChannelId) -> Result<(), Error> {
        let channels = self.lightning_node.list_channels();
        match channels
            .iter()
            .find(|channel| channel.user_channel_id == channel_id)
        {
            Some(channel) => {
                let close = self
                    .lightning_node
                    .close_channel(&channel_id, channel.counterparty_node_id);

                if close.is_err() {
                    // if cooperative close fails, try force close
                    self.lightning_node
                        .force_close_channel(&channel_id, channel.counterparty_node_id)?;
                }

                Ok(())
            }
            None => return Err(Error::ChannelNotExist),
        }
    }

    // list of channels
    pub fn list_channels(&self) -> Result<Vec<ChannelInfo>, Error> {
        let channels = self.lightning_node.list_channels();
        let channels = channels
            .iter()
            .map(|channel| ChannelInfo::from(channel))
            .collect();
        Ok(channels)
    }

    pub fn send_to_address(
        &self,
        address: &Address<NetworkUnchecked>,
        amount_sat: u64,
    ) -> Result<Txid, Error> {
        let address = address.clone().require_network(Network::Signet).unwrap();
        let txid = self
            .lightning_node
            .onchain_payment()
            .send_to_address(&address, amount_sat)?;

        Ok(txid)
    }

    // TODO: mint unclaimed quotes

    // ask faucet to open channel to node
}
