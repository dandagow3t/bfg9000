pub mod client;
mod fragment;
mod handshake;
mod recv;
mod tls_connector;
mod websocket;

pub use fastwebsockets::OpCode;

pub async fn connect(
    url: &str,
) -> Result<self::client::Online, Box<dyn std::error::Error + Send + Sync>> {
    self::client::Offline::new().connect(url).await
}
