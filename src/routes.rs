use std::{collections::HashMap, str::FromStr, u64};

use axum::{
    extract::{self, Query},
    http::StatusCode,
    Extension, Json,
};
use cdk::Bolt11Invoice;
use hex_conservative::FromHex;
use ldk_node::{bitcoin::Address, lightning::ln::msgs::SocketAddress, UserChannelId};
use secp256k1::PublicKey;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::Error;
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
            let amount: u64 = amount_param.parse().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid amount"})),
                )
            })?;

            amount
        }
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "amount not specified"})),
            ))
        }
    };

    let invoice = state.wallet.receive(amount).await.map_err(handle_err)?;
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
    let invoice = Bolt11Invoice::from_str(payload.invoice.as_str()).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid invoice"})),
        )
    })?;

    let payment = state
        .wallet
        .pay_invoice(invoice)
        .await
        .map_err(handle_err)?;
    Ok(Json(json!(payment)))
}

pub async fn swap(
    Extension(state): Extension<State>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let amount_to_swap = match params.get("amount") {
        Some(amount_param) => {
            let amount: u64 = amount_param.parse().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid amount"})),
                )
            })?;
            amount
        }
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "amount not specified"})),
            ))
        }
    };

    let _ = state
        .wallet
        .swap(amount_to_swap)
        .await
        .map_err(handle_err)?;

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
    let amount = state
        .wallet
        .receive_ecash(payload.ecash)
        .await
        .map_err(handle_err)?;
    Ok(Json(json!(format!("received {} ecash", amount))))
}

pub async fn send_ecash(
    Extension(state): Extension<State>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let amount = match params.get("amount") {
        Some(amount_param) => {
            let amount: u64 = amount_param.parse().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid amount"})),
                )
            })?;

            amount
        }
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "amount not specified"})),
            ))
        }
    };
    let ecash_token = state.wallet.send_ecash(amount).await.map_err(handle_err)?;

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
            let pubkey = PublicKey::from_str(pubkey.as_str()).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid public key"})),
                )
            })?;
            Some(pubkey)
        }
        None => None,
    };

    let node_address = match payload.node_address {
        Some(address) => {
            let address = SocketAddress::from_str(address.as_str()).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid address"})),
                )
            })?;
            Some(address)
        }
        None => None,
    };

    let channel_id = state
        .wallet
        .open_channel(payload.amount_sat, node_pubkey, node_address)
        .map_err(handle_err)?;

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
    let channel_id: [u8; 16] = FromHex::from_hex(&payload.channel_id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid channel id"})),
        )
    })?;
    let channel_id = UserChannelId::from(ldk_node::UserChannelId(u128::from_be_bytes(channel_id)));

    let _ = state.wallet.close_channel(channel_id).unwrap();
    Ok(Json(json!("channel closed")))
}

pub async fn list_channels(
    Extension(state): Extension<State>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let channels = state.wallet.list_channels().map_err(handle_err)?;
    Ok(Json(json!(channels)))
}

pub async fn balance(
    Extension(state): Extension<State>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let balance = state.wallet.balance().await.map_err(handle_err)?;
    Ok(Json(json!(balance)))
}

pub async fn new_address(
    Extension(state): Extension<State>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let address = state.wallet.new_address().map_err(handle_err)?;
    Ok(Json(json!(address.to_string())))
}

#[derive(Deserialize)]
pub struct SendToAddress {
    address: String,
    amount_sat: u64,
}

pub async fn send_to_address(
    Extension(state): Extension<State>,
    extract::Json(payload): extract::Json<SendToAddress>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let address = Address::from_str(&payload.address).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid address"})),
        )
    })?;

    let txid = state
        .wallet
        .send_to_address(&address, payload.amount_sat)
        .map_err(handle_err)?;

    Ok(Json(json!(txid)))
}

fn handle_err(err: Error) -> (StatusCode, Json<Value>) {
    let err = json!({
        "error": format!("{err}"),
    });
    (StatusCode::INTERNAL_SERVER_ERROR, Json(err))
}
