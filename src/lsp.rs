use std::str::FromStr;

use ldk_node::lightning_invoice::Bolt11Invoice;
use reqwest::Client;
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::error::Error;

const LSP_URL: &str = "https://mutinynet-flow.lnolymp.us";

#[derive(Clone)]
pub struct LspClient {
    pub client: Client,
    pub url: String,
}

#[derive(Serialize, Deserialize)]
pub struct LspFeeRequest {
    pub amount_msat: u64,
    pub pubkey: String,
}

#[derive(Serialize, Deserialize)]
pub struct LspFeeResponse {
    pub fee_amount_msat: u64,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct LspProposalRequest {
    pub bolt11: String,
    pub fee_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct LspProposalResponse {
    pub jit_bolt11: String,
}

impl Default for LspClient {
    fn default() -> Self {
        LspClient {
            client: Client::new(),
            url: LSP_URL.to_string(),
        }
    }
}

impl LspClient {
    pub async fn lsp_fee(&self, amount: u64, pubkey: PublicKey) -> Result<LspFeeResponse, Error> {
        let fee_request = LspFeeRequest {
            amount_msat: amount,
            pubkey: pubkey.to_string(),
        };

        let fee_response = self
            .client
            .post(format!("{}{}", self.url, "/api/v1/fee"))
            .json(&fee_request)
            .send()
            .await?
            .json::<LspFeeResponse>()
            .await?;

        Ok(fee_response)
    }

    pub async fn get_lsp_wrapped_invoice(
        &self,
        id: String,
        invoice: Bolt11Invoice,
    ) -> Result<Bolt11Invoice, Error> {
        let proposal = LspProposalRequest {
            bolt11: invoice.to_string(),
            fee_id: id,
        };

        let proposal_response = self
            .client
            .post(format!("{}{}", self.url, "/api/v1/proposal"))
            .json(&proposal)
            .send()
            .await?
            .json::<LspProposalResponse>()
            .await?;

        let wrapped_invoice = Bolt11Invoice::from_str(&proposal_response.jit_bolt11).unwrap();

        Ok(wrapped_invoice)
    }
}
