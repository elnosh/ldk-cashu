use axum::{
    routing::{get, post},
    Extension, Router,
};

mod routes;
mod wallet;

#[tokio::main]
async fn main() {
    let ln_cashu_wallet = wallet::LnCashuWallet::new();
    ln_cashu_wallet.start().await.unwrap();

    let state = routes::State {
        wallet: ln_cashu_wallet.clone(),
    };

    let app = Router::new()
        .route("/balance", get(routes::balance))
        .route("/newaddress", get(routes::new_address))
        .route("/sendtoaddress", post(routes::send_to_address))
        .route("/openchannel", post(routes::open_channel))
        .route("/closechannel", post(routes::close_channel))
        .route("/listchannels", get(routes::list_channels))
        .route("/createinvoice", get(routes::receive))
        .route("/payinvoice", post(routes::send))
        .route("/swap", post(routes::swap))
        .route("/receive-ecash", post(routes::receive_ecash))
        .route("/send-ecash", post(routes::send_ecash))
        .layer(Extension(state.clone()));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
