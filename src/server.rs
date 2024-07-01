use futures::channel::mpsc::{self, UnboundedSender};
use futures::{future, pin_mut, StreamExt, TryStreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type PerrMap = Arc<Mutex<HashMap<SocketAddr, UnboundedSender<Message>>>>;

const SERVER_ADDR: &str = "127.0.0.1:8080";

pub async fn server_init(buffer: Arc<Mutex<Vec<f32>>>) {
    let listener = TcpListener::bind(SERVER_ADDR).await.unwrap();

    let peers: PerrMap = Arc::new(Mutex::new(HashMap::new()));
    while let Ok((stream, peer_addr)) = listener.accept().await {
        let peers = Arc::clone(&peers);
        tokio::spawn(handle_connection(stream, peers, peer_addr));
    }
}

async fn handle_connection(tcp_stream: TcpStream, peer_map: PerrMap, addr: SocketAddr) {
    let ws_stream = tokio_tungstenite::accept_async(tcp_stream)
        .await
        .expect("Error during websocket handshake");

    println!("New connection from: {}", addr);

    let (tx, rx) = mpsc::unbounded();
    peer_map.lock().unwrap().insert(addr, tx);

    let (outgoing, incoming) = ws_stream.split();

    let broadcast_incoming = incoming.try_for_each(|msg| {
        println!(
            "Received a message from {}: {}",
            addr,
            msg.to_text().unwrap()
        );
        let peers = peer_map.lock().unwrap();

        // We want to broadcast the message to everyone except ourselves.
        let broadcast_recipients = peers
            .iter()
            .filter(|(peer_addr, _)| peer_addr != &&addr)
            .map(|(_, ws_sink)| ws_sink);

        for recp in broadcast_recipients {
            recp.unbounded_send(msg.clone()).unwrap();
        }

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", &addr);
    peer_map.lock().unwrap().remove(&addr);
}
