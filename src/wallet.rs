use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use cdk::wallet::Wallet;
use cdk::{Amount, Bolt11Invoice, UncheckedUrl};
use cdk_redb::RedbWalletDatabase;
use ldk_node::bitcoin::{Address, Network};
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::{Builder, Node, UserChannelId};
use rand::Rng;
use secp256k1::PublicKey;
use serde::Serialize;
use tokio::time::{sleep, timeout};

const DB_PATH: &str = "./walletdb";

#[derive(Clone)]
pub struct LnCashuWallet {
    cashu: Wallet,
    default_cashu_mint: String,
    lightning_node: Arc<Node>,
}

impl LnCashuWallet {
    pub fn new() -> Self {
        let seed = rand::thread_rng().gen::<[u8; 32]>();
        let cashu_db = Arc::new(RedbWalletDatabase::new(DB_PATH).unwrap());

        let mut builder = Builder::new();
        builder.set_network(Network::Signet);
        builder.set_esplora_server("https://mutinynet.com/api".to_string());
        builder.set_gossip_source_p2p();
        // TODO: set lsp

        let node = builder.build().unwrap();

        LnCashuWallet {
            cashu: Wallet::new(cashu_db, &seed, vec![]),
            default_cashu_mint: "https://cashu.mutinynet.com".to_string(),
            lightning_node: Arc::new(node),
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        self.lightning_node.start().unwrap();

        // TODO: start thread for checking events from ldk

        Ok(())
    }

    // dependending on liquidity receive through cashu or self-custodial
    pub async fn receive(self, amt: u64) -> Result<Bolt11Invoice, Box<dyn Error>> {
        let channels = self.lightning_node.list_channels();

        let mut inbound: u64 = 0;
        for channel in channels.iter() {
            let inbound_sat = channel.inbound_capacity_msat / 1000;
            inbound += inbound_sat;
        }

        // if no channels or no inbound liquidity, get invoice from cashu wallet
        if channels.is_empty() || inbound < amt {
            let mint_quote = self
                .cashu
                .mint_quote(
                    UncheckedUrl::from_str(&self.default_cashu_mint).unwrap(),
                    Amount::from(amt),
                    cdk::nuts::CurrencyUnit::Sat,
                )
                .await
                .unwrap();

            let invoice = Bolt11Invoice::from_str(&mint_quote.request).unwrap();

            // spawn thread for couple of minutes that will constantly check invoice
            // if invoice does not get paid in the window
            // add endpoint that will try to mint unclaimed quotes
            tokio::spawn(timeout(Duration::from_secs(180), async move {
                loop {
                    let quote_status = self
                        .cashu
                        .mint_quote_status(
                            UncheckedUrl::from_str(&self.default_cashu_mint).unwrap(),
                            &mint_quote.id,
                        )
                        .await
                        .unwrap();

                    if quote_status.paid {
                        // try mint
                        let amount = self
                            .cashu
                            .mint(
                                UncheckedUrl::from(&self.default_cashu_mint),
                                &mint_quote.id,
                                cdk::amount::SplitTarget::None,
                                None,
                            )
                            .await
                            .unwrap();

                        println!("minted {amount} sats!");
                        return;
                    }

                    sleep(Duration::from_secs(10)).await;
                }
            }));

            return Ok(invoice);
        } else {
            // if enough inbound, get invoice from lightning node
            let invoice = self
                .lightning_node
                .bolt11_payment()
                .receive(amt * 1000, "", 3600)
                .unwrap();
            return Ok(invoice);
        }
    }

    pub async fn balance(&self) -> Balance {
        let cashu_balance = self
            .cashu
            .unit_balance(cdk::nuts::CurrencyUnit::Sat)
            .await
            .unwrap();

        let lightning_balance = self
            .lightning_node
            .list_balances()
            .total_lightning_balance_sats;

        Balance {
            cashu_balance: cashu_balance.into(),
            lightning_balance,
        }
    }

    // receive ecash

    // pay invoice
    // send ecash

    // swap (from cashu to ln node via jit channel or regular invoice if enough liquidity)

    // get new address
    pub fn new_address(&self) -> Result<Address, Box<dyn Error>> {
        let address = self.lightning_node.onchain_payment().new_address()?;
        Ok(address)
    }

    // open channel
    pub fn open_channel(
        &self,
        amount_sats: u64,
        node_pubkey: Option<PublicKey>,
        node_address: Option<SocketAddress>,
    ) -> Result<UserChannelId, Box<dyn Error>> {
        // if pubkey or node address not specified, open channel to faucet
        let node_pubkey = node_pubkey.unwrap_or(
            PublicKey::from_str(
                "02465ed5be53d04fde66c9418ff14a5f2267723810176c9212b722e542dc1afb1b",
            )
            .unwrap(),
        );

        let node_address =
            node_address.unwrap_or(SocketAddress::from_str("45.79.52.207:9735").unwrap());

        let channel_id = self
            .lightning_node
            .connect_open_channel(node_pubkey, node_address, amount_sats, None, None, true)
            .unwrap();

        Ok(channel_id)
    }

    // close channel

    // ask faucet to open channel to node
}

#[derive(Clone, Copy, Serialize)]
pub struct Balance {
    pub cashu_balance: u64,
    pub lightning_balance: u64,
}
