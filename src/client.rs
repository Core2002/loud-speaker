use actix_web::web::Bytes;
use actix_web_actors::ws;
use futures_util::{SinkExt, StreamExt};
use tokio::select;

pub async fn client_init() {
    let (res, mut ws) = awc::Client::new()
        .ws("ws://127.0.0.1:8080/ws/")
        .connect()
        .await
        .unwrap();

    log::debug!("response: {res:?}");
    log::info!("connected; server will echo messages sent");
    loop {
        select! {
            Some(msg) = ws.next() => {
                match msg {
                    Ok(ws::Frame::Text(txt)) => {
                        // log echoed messages from server
                        log::info!("Server: {txt:?}")
                    }

                    Ok(ws::Frame::Ping(_)) => {
                        // respond to ping probes
                        ws.send(ws::Message::Pong(Bytes::new())).await.unwrap();
                    }

                    _ => {}
                }
            }
            else => break
        }
    }
}