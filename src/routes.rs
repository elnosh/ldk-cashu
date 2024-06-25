use std::{collections::HashMap, str::FromStr, u64};

use axum::{
    extract::{self, Query},
    http::StatusCode,
    Extension, Json,
};
use ldk_node::lightning::{ln::msgs::SocketAddress, util::ser::Writeable};
use secp256k1::PublicKey;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::wallet::LnCashuWallet;

#[derive(Clone)]
pub struct State {
    pub wallet: LnCashuWallet,
}

pub async fn receive(
    Extension(state): Extension<State>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let amount = match params.get("amount") {
        Some(amount_param) => {
            let amount: u64 = amount_param
                .parse()
                .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!("invalid amount"))))?;

            amount
        }
        None => return Err((StatusCode::BAD_REQUEST, Json(json!("amount not specified")))),
    };

    let invoice = state.wallet.receive(amount).await.unwrap();
    Ok(Json(json!(invoice)))
}

#[derive(Deserialize)]
pub struct OpenChannel {
    amount_sat: u64,
    node_pubkey: Option<String>,
    node_address: Option<String>,
}

pub async fn open_channel(
    Extension(state): Extension<State>,
    extract::Json(payload): extract::Json<OpenChannel>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_pubkey = match payload.node_pubkey {
        Some(pubkey) => {
            let pubkey = PublicKey::from_str(pubkey.as_str()).unwrap();
            Some(pubkey)
        }
        None => None,
    };

    let node_address = match payload.node_address {
        Some(address) => {
            let address = SocketAddress::from_str(address.as_str()).unwrap();
            Some(address)
        }
        None => None,
    };

    let channel_id = state
        .wallet
        .open_channel(payload.amount_sat, node_pubkey, node_address)
        .unwrap();

    let channel_id = hex::encode(channel_id.encode());

    Ok(Json(json!(channel_id)))
}

pub async fn balance(
    Extension(state): Extension<State>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let balance = state.wallet.balance().await;
    Ok(Json(json!(balance)))
}

pub async fn getnewaddress(
    Extension(state): Extension<State>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let address = state.wallet.new_address().unwrap();
    Ok(Json(json!(address)))
}
