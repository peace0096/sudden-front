//! One bot's connection: a WebSocket to the server, the behaviour tree ticked at the server's tick
//! rate, and a Ping every two seconds like the browser's.

use crate::pb::{self, client_message::Payload as Out, server_message::Payload as In};
use crate::stats::{add, sub, Stats};
use crate::tree::Mind;
use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

/// The tree runs once a server tick.
const TICK: Duration = Duration::from_micros(33_333);
const PING_EVERY: Duration = Duration::from_secs(2);
/// How long a bot may take to leave once told to stop.
const LEAVE_GRACE: Duration = Duration::from_secs(3);

/// Plays until the run stops, then leaves and hangs up.
pub async fn run(url: String, mut mind: Mind, stats: Arc<Stats>, mut stop: watch::Receiver<bool>) {
    let name = mind.agent().name.clone();
    let socket = match tokio_tungstenite::connect_async(url.as_str()).await {
        Ok((socket, _)) => socket,
        Err(error) => {
            add(&stats.connect_failures, 1);
            eprintln!("{name}: cannot connect to {url}: {error}");
            return;
        }
    };
    add(&stats.connected, 1);
    let (mut sink, mut stream) = socket.split();
    let started = Instant::now();
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_tick = Instant::now();
    let mut next_ping = Instant::now();
    let mut stopping_since: Option<Instant> = None;
    let mut seated = false;
    let mut playing = false;
    let mut server_closed = false;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f64();
                last_tick = now;
                if *stop.borrow() && stopping_since.is_none() {
                    stopping_since = Some(now);
                    mind.agent().shutdown = true;
                }
                let finished = mind.tick(dt);
                let agent = mind.agent();
                if agent.my_id.is_some() && now >= next_ping {
                    next_ping = now.checked_add(PING_EVERY).unwrap_or(now);
                    agent.outbox.push(Out::Ping(pb::Ping {
                        client_time: started.elapsed().as_secs_f64() * 1000.0,
                    }));
                }
                let outgoing = std::mem::take(&mut agent.outbox);
                let mut failed = false;
                for payload in outgoing {
                    let bytes = pb::ClientMessage { payload: Some(payload) }.encode_to_vec();
                    add(&stats.frames_out, 1);
                    add(&stats.bytes_out, bytes.len() as u64);
                    if sink.send(Message::Binary(bytes.into())).await.is_err() {
                        failed = true;
                        break;
                    }
                }
                track(&stats.in_room, &mut seated, agent.in_room());
                track(&stats.in_match, &mut playing, agent.in_match());
                if failed {
                    server_closed = true;
                    break;
                }
                if finished || stopping_since.is_some_and(|since| since.elapsed() > LEAVE_GRACE) {
                    break;
                }
            }
            frame = stream.next() => match frame {
                Some(Ok(Message::Binary(data))) => {
                    add(&stats.frames_in, 1);
                    add(&stats.bytes_in, data.len() as u64);
                    let Ok(message) = pb::ServerMessage::decode(data.as_ref()) else {
                        continue;
                    };
                    if let Some(In::Pong(pong)) = &message.payload {
                        let now = started.elapsed().as_secs_f64() * 1000.0;
                        stats.record_rtt(((now - pong.client_time).max(0.0) * 1000.0) as u64);
                    }
                    let agent = mind.agent();
                    agent.absorb(message);
                    let heard = std::mem::take(&mut agent.heard);
                    add(&stats.snapshots, heard.snapshots);
                    add(&stats.kills, heard.kills);
                    add(&stats.server_errors, heard.errors.len() as u64);
                    for error in heard.errors {
                        eprintln!("{name}: server: {error}");
                    }
                    if heard.match_started {
                        add(&stats.matches_started, 1);
                    }
                    if heard.match_ended {
                        add(&stats.matches_ended, 1);
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    server_closed = true;
                    break;
                }
                Some(Ok(_)) => {}
            },
            _ = stop.changed(), if stopping_since.is_none() => {
                // Noticed on the next tick; this only wakes the loop.
            }
        }
    }

    if server_closed && stopping_since.is_none() {
        add(&stats.disconnects, 1);
        eprintln!("{name}: the server closed the connection");
    } else {
        let _ = sink.send(Message::Close(None)).await;
    }
    track(&stats.in_room, &mut seated, false);
    track(&stats.in_match, &mut playing, false);
    sub(&stats.connected, 1);
}

/// Keeps a run-wide gauge in step with one bot's state.
fn track(gauge: &std::sync::atomic::AtomicU64, was: &mut bool, now: bool) {
    match (*was, now) {
        (false, true) => add(gauge, 1),
        (true, false) => sub(gauge, 1),
        _ => {}
    }
    *was = now;
}
