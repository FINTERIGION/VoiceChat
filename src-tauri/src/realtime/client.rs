use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::events::ClientEvent;

pub type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
pub type WsSource = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

const MODEL: &str = "qwen-audio-3.0-realtime-flash";

pub fn realtime_url(workspace_id: Option<&str>, region: Option<&str>) -> String {
    format!(
        "wss://{}/api-ws/v1/realtime?model={MODEL}",
        crate::dashscope::host(workspace_id, region)
    )
}

pub async fn connect(url: &str, api_key: &str) -> Result<(WsSink, WsSource), String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let auth: HeaderValue = format!("Bearer {api_key}")
        .parse()
        .map_err(|_| "invalid API key header value".to_string())?;
    request.headers_mut().insert("Authorization", auth);

    let (ws, _response) = connect_async(request).await.map_err(|e| e.to_string())?;
    Ok(ws.split())
}

pub async fn send_event(sink: &mut WsSink, event: &ClientEvent) -> Result<(), String> {
    let text = serde_json::to_string(event).map_err(|e| e.to_string())?;
    sink.send(Message::text(text)).await.map_err(|e| e.to_string())
}
