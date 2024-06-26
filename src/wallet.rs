use std::error::Error;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use cdk::wallet::Wallet;
use cdk::{Amount, Bolt11Invoice, UncheckedUrl};
use cdk_redb::RedbWalletDatabase;
use ldk_node::bitcoin::{Address, Network};
use ldk_node::lightning::ln::channelmanager::PaymentId;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::{Builder, Node, UserChannelId};
use rand::Rng;
use secp256k1::PublicKey;
use serde::Serialize;
use tokio::time::{sleep, timeout};

const DB_PATH: &str = "./walletdb";
const MIN_CHANNEL_OPENING_SAT: u64 = 5_000_000;

#[derive(Clone)]
pub struct LnCashuWallet {
    default_cashu_mint: UncheckedUrl,
    cashu: Wallet,
    lightning_node: Arc<Node>,
}

impl LnCashuWallet {
    pub fn new() -> Self {
        let seed = rand::thread_rng().gen::<[u8; 32]>();
        let path = Path::new(DB_PATH);
        let cashu_db = Arc::new(RedbWalletDatabase::new(path).unwrap());

        let mut builder = Builder::new();
        builder.set_network(Network::Signet);
        builder.set_esplora_server("https://mutinynet.com/api".to_string());
        builder.set_gossip_source_p2p();

        let lsp_addres = SocketAddress::from_str("45.33.17.66:39735").unwrap();
        let lsp_pubkey = PublicKey::from_str(
            "02c1745d21aab28234955666078778519ae55dc2a82ef0e7268340fd3893362b63",
        )
        .unwrap();
        builder.set_liquidity_source_lsps2(lsp_addres, lsp_pubkey, None);

        let node = builder.build().unwrap();

        LnCashuWallet {
            default_cashu_mint: UncheckedUrl::from_str("https://cashu.mutinynet.com").unwrap(),
            cashu: Wallet::new(cashu_db, &seed, vec![]),
            lightning_node: Arc::new(node),
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        self.lightning_node.start().unwrap();

        // TODO: start thread for checking events from ldk

        Ok(())
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

    // dependending on liquidity receive through cashu or lightning node
    pub async fn receive(self, amt: u64) -> Result<Bolt11Invoice, Box<dyn Error>> {
        let inbound = self.inbound_liquidity();

        // if no channels or no inbound liquidity, get invoice from cashu wallet
        if inbound < amt {
            let mint_quote = self
                .cashu
                .mint_quote(
                    self.default_cashu_mint.clone(),
                    cdk::nuts::CurrencyUnit::Sat,
                    Amount::from(amt),
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
                        .mint_quote_status(self.default_cashu_mint.clone(), &mint_quote.id)
                        .await
                        .unwrap();

                    if quote_status.paid {
                        // try mint
                        let amount = self
                            .cashu
                            .mint(
                                self.default_cashu_mint,
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

    pub async fn receive_ecash(&self, token: String) -> Result<u64, Box<dyn Error>> {
        let amount = self
            .cashu
            .receive(token.as_str(), &cdk::amount::SplitTarget::None, None)
            .await?;

        Ok(amount.into())
    }

    pub async fn pay_invoice(&self, invoice: Bolt11Invoice) -> Result<String, Box<dyn Error>> {
        // handle amountless invoices later
        let invoice_amount = invoice.amount_milli_satoshis().unwrap() / 1000;
        let balance = self.balance().await;

        if balance.cashu_balance > invoice_amount {
            let melt_quote = self
                .cashu
                .melt_quote(
                    self.default_cashu_mint.clone(),
                    cdk::nuts::CurrencyUnit::Sat,
                    invoice.to_string(),
                    None,
                )
                .await?;

            let melt = self
                .cashu
                .melt(
                    &self.default_cashu_mint,
                    melt_quote.id.as_str(),
                    cdk::amount::SplitTarget::None,
                )
                .await?;

            return Ok(melt.preimage.unwrap());
        }

        let payment = self.lightning_node.bolt11_payment().send(&invoice)?;
        // this is probably wrong
        Ok(payment.to_string())
    }

    pub async fn send_ecash(&self, amount_sats: u64) -> Result<String, Box<dyn Error>> {
        // TODO: why this send method returns a string instead of a Token
        let token = self
            .cashu
            .send(
                &self.default_cashu_mint,
                cdk::nuts::CurrencyUnit::Sat,
                Amount::from(amount_sats),
                None,
                None,
                &cdk::amount::SplitTarget::None,
            )
            .await?;

        Ok(token)
    }

    // swap (from cashu to ln node via jit channel or regular invoice if enough liquidity)
    pub fn swap(&self, target_amount_sats: u64) -> Result<(), Box<dyn Error>> {
        // TODO: have some config that sets the minimum amount for a channel opening
        // to avoid opening small channels

        let inbound = self.inbound_liquidity();
        if inbound < target_amount_sats && target_amount_sats < MIN_CHANNEL_OPENING_SAT {
            // throw proper error
            return Err("no inbound to make swap".into());
        }

        // TODO: finish this

        Ok(())
    }

    fn inbound_liquidity(&self) -> u64 {
        let channels = self.lightning_node.list_channels();
        let mut inbound: u64 = 0;
        for channel in channels.iter() {
            let inbound_sat = channel.inbound_capacity_msat / 1000;
            inbound += inbound_sat;
        }
        inbound
    }

    pub fn new_address(&self) -> Result<Address, Box<dyn Error>> {
        let address = self.lightning_node.onchain_payment().new_address()?;
        Ok(address)
    }

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

        let channel_id = self.lightning_node.connect_open_channel(
            node_pubkey,
            node_address,
            amount_sats,
            None,
            None,
            true,
        )?;

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
