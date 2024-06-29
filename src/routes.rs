use std::{collections::HashMap, str::FromStr, u64};

use axum::{
    extract::{self, Query},
    http::StatusCode,
    Extension, Json,
};
use cdk::Bolt11Invoice;
use ldk_node::{
    lightning::{ln::msgs::SocketAddress, util::ser::Writeable},
    UserChannelId,
};
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
pub struct InvoiceRequest {
    invoice: String,
}

pub async fn send(
    Extension(state): Extension<State>,
    extract::Json(payload): extract::Json<InvoiceRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let invoice = Bolt11Invoice::from_str(payload.invoice.as_str()).unwrap();
    let payment = state.wallet.pay_invoice(invoice).await.unwrap();
    //let payment = hex::encode(payment.0);
    Ok(Json(json!(payment)))
}

pub async fn swap(
    Extension(state): Extension<State>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let amount_to_swap = match params.get("amount") {
        Some(amount_param) => {
            let amount: u64 = amount_param
                .parse()
                .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!("invalid amount"))))?;

            amount
        }
        None => return Err((StatusCode::BAD_REQUEST, Json(json!("amount not specified")))),
    };

    let _ = state.wallet.swap(amount_to_swap).await.unwrap();

    Ok(Json(json!("swap successful")))
}

#[derive(Deserialize)]
pub struct ReceiveEcash {
    ecash: String,
}

pub async fn receive_ecash(
    Extension(state): Extension<State>,
    extract::Json(payload): extract::Json<ReceiveEcash>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    println!("ecash: {}", payload.ecash);
    let amount = state.wallet.receive_ecash(payload.ecash).await.unwrap();
    Ok(Json(json!(format!("received {} ecash", amount))))
}

pub async fn send_ecash(
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
    let ecash_token = state.wallet.send_ecash(amount).await.unwrap();

    Ok(Json(json!(ecash_token)))
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

#[derive(Deserialize)]
pub struct CloseChannel {
    channel_id: String,
}

pub async fn close_channel(
    Extension(state): Extension<State>,
    extract::Json(payload): extract::Json<CloseChannel>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    //let channel_id = UserChannelId::from(payload.channel_id).unwrap();
    // let _ = state.wallet.close_channel(channel_id).unwrap();
    // Ok(Json(json!("channel closed")))
    todo!()
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
