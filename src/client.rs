use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use loud_speaker::{proto, AudioBus};

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
/// The server streams continuously (silence included), so a long gap means the
/// connection is dead even if the OS has not noticed yet.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect to a server, feed received audio into `bus`, and reconnect forever.
///
/// `out_rate` / `out_channels` describe the local speaker; the bus is rebuilt
/// from the server's sample rate whenever a new session announces itself.
pub async fn client_init(bus: Arc<Mutex<AudioBus>>, url: &str, out_rate: u32, out_channels: usize) {
    loop {
        match connect_async(url).await {
            Ok((ws_stream, _)) => {
                println!("已连接服务端: {url}");
                receive_audio(ws_stream, &bus, out_rate, out_channels).await;
                println!("与服务端断开，{} 秒后重连...", RECONNECT_DELAY.as_secs());
            }
            Err(err) => {
                eprintln!(
                    "连接 {url} 失败: {err}，{} 秒后重试...",
                    RECONNECT_DELAY.as_secs()
                );
            }
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn receive_audio(
    mut ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    bus: &Arc<Mutex<AudioBus>>,
    out_rate: u32,
    out_channels: usize,
) {
    loop {
        let message = match tokio::time::timeout(READ_TIMEOUT, ws_stream.next()).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(_) => {
                eprintln!("{} 秒未收到音频，断开重连...", READ_TIMEOUT.as_secs());
                break;
            }
        };

        match message {
            Ok(Message::Text(text)) => match proto::parse_hello(&text) {
                Some((sample_rate, channels)) => {
                    println!("服务端音频格式: {sample_rate} Hz / {channels} 声道");
                    if let Ok(mut bus) = bus.lock() {
                        // Start a fresh session; this also discards audio that
                        // was queued before a reconnect.
                        *bus = AudioBus::from_rates(sample_rate, out_rate, out_channels);
                    }
                }
                None => eprintln!("忽略未知控制消息: {text}"),
            },
            Ok(Message::Binary(bytes)) => {
                let samples = proto::decode_samples(&bytes);
                if samples.is_empty() {
                    continue;
                }
                if let Ok(mut bus) = bus.lock() {
                    bus.push_mono(&samples);
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(err) => {
                eprintln!("接收音频失败: {err}");
                break;
            }
        }
    }
}
