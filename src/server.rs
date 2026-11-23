use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::{self, Sender};
use futures::{future, pin_mut, SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

use loud_speaker::{proto, AudioBus};

/// Connected clients and the queue feeding each of their sockets.
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Sender<Message>>>>;

/// How much freshly captured audio is batched into one WebSocket frame.
const PUMP_INTERVAL: Duration = Duration::from_millis(20);

/// Share the microphone stream from `bus` with every connected client.
///
/// Blocks forever; the process is expected to exit on Ctrl+C.
pub async fn server_init(bus: Arc<Mutex<AudioBus>>, bind_addr: &str, sample_rate: u32) {
    let listener = TcpListener::bind(bind_addr)
        .await
        .unwrap_or_else(|err| panic!("无法监听 {bind_addr}: {err}"));
    println!(
        "服务端已启动: 监听 {}（麦克风 {} Hz，单声道）",
        bind_addr, sample_rate
    );

    let peers: PeerMap = Arc::new(Mutex::new(HashMap::new()));
    tokio::spawn(pump_audio(bus, peers.clone()));

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let peers = peers.clone();
                tokio::spawn(handle_connection(stream, peers, addr, sample_rate));
            }
            Err(err) => eprintln!("接受连接失败: {err}"),
        }
    }
}

/// Periodically drain the mic ring and broadcast it to every client.
///
/// Draining happens even with no clients so a newly connected client never
/// starts with a backlog of stale audio.
async fn pump_audio(bus: Arc<Mutex<AudioBus>>, peers: PeerMap) {
    let mut ticker = tokio::time::interval(PUMP_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        let samples = match bus.lock() {
            Ok(mut bus) => bus.drain(usize::MAX),
            Err(_) => return,
        };
        if samples.is_empty() {
            continue;
        }

        let mut peers = peers.lock().unwrap();
        if peers.is_empty() {
            continue;
        }

        let message = Message::Binary(proto::encode_samples(&samples));
        // A full queue means the peer cannot keep up: skip this frame for it
        // rather than letting real-time audio pile up in memory. A closed
        // queue means the connection is gone, so drop it from the map.
        peers.retain(|_, sender| match sender.try_send(message.clone()) {
            Ok(()) => true,
            Err(err) => !err.is_disconnected(),
        });
    }
}

async fn handle_connection(
    tcp_stream: TcpStream,
    peers: PeerMap,
    addr: SocketAddr,
    sample_rate: u32,
) {
    let ws_stream = match tokio_tungstenite::accept_async(tcp_stream).await {
        Ok(ws_stream) => ws_stream,
        Err(err) => {
            eprintln!("{addr} WebSocket 握手失败: {err}");
            return;
        }
    };
    println!("客户端已连接: {addr}");

    let (mut outgoing, mut incoming) = ws_stream.split();

    // The hello must reach the client before any audio, so queue it before the
    // peer becomes visible to the audio pump. The queue is bounded: if a client
    // stalls, old audio is dropped instead of buffered forever.
    let (mut tx, mut rx) = mpsc::channel(8);
    if tx
        .try_send(Message::Text(proto::hello(sample_rate, 1)))
        .is_err()
    {
        return;
    }
    peers.lock().unwrap().insert(addr, tx);

    let send_audio = async {
        while let Some(message) = rx.next().await {
            if let Err(err) = outgoing.send(message).await {
                eprintln!("向 {addr} 发送失败: {err}");
                break;
            }
        }
    };
    // Clients only receive audio; consume their stream to notice disconnects.
    let watch_client = async {
        while let Some(message) = incoming.next().await {
            match message {
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => {}
            }
        }
    };

    pin_mut!(send_audio, watch_client);
    future::select(send_audio, watch_client).await;

    peers.lock().unwrap().remove(&addr);
    println!("客户端已断开: {addr}");
}
