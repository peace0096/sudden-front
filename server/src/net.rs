//! WebSocket transport: binary protobuf frames <-> the hub thread.

use crate::hub::Inbound;
use crate::pb;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use tokio::sync::mpsc;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Largest client frame we accept; a full input batch is far below this.
const MAX_FRAME_BYTES: usize = 16 * 1024;

pub async fn ws_handler(ws: WebSocketUpgrade, State(hub): State<Sender<Inbound>>) -> Response {
    ws.max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| client_session(socket, hub))
}

async fn client_session(socket: WebSocket, hub: Sender<Inbound>) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(512);

    if hub.send(Inbound::Connect { id, tx: out_tx }).is_err() {
        return;
    }

    let mut writer = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Some(Ok(Message::Binary(data))) => {
                    // Malformed frames are dropped; they can't affect other players.
                    if let Ok(msg) = pb::ClientMessage::decode(data.as_ref()) {
                        if hub.send(Inbound::Message { id, msg }).is_err() {
                            break;
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            // The hub closes the channel when it kicks a client that can't keep up.
            _ = &mut writer => break,
        }
    }

    writer.abort();
    let _ = hub.send(Inbound::Disconnect { id });
}
