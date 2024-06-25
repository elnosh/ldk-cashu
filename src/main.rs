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

    //ln_cashu_wallet.lightning_node.start().unwrap();

    let state = routes::State {
        wallet: ln_cashu_wallet.clone(),
    };

    let app = Router::new()
        .route("/balance", get(routes::balance))
        .route("/newaddress", get(routes::getnewaddress))
        .route("/openchannel", post(routes::open_channel))
        .route("/createinvoice", get(routes::receive))
        .layer(Extension(state.clone()));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}