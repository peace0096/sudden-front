//! Sudden Front load test: `--count` bots connect like browser players, gather ten to a room, play
//! match after match, and leave when the run ends.
//!
//! cargo run --release -- --count 50                   until Ctrl+C
//! cargo run --release -- --count 50 --duration 300    for five minutes

mod agent;
mod bot;
mod combat;
mod plan;
mod rng;
mod stats;
mod tree;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/game.v1.rs"));
}

use agent::{Agent, RoomSettings};
use clap::{Parser, ValueEnum};
use rng::Rng;
use stats::Stats;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

/// How often the console summary prints.
const SUMMARY_EVERY: Duration = Duration::from_secs(10);
/// How long the run waits for its bots to leave once it stops.
const WIND_DOWN: Duration = Duration::from_secs(5);

#[derive(Parser, Debug)]
#[command(about = "Bots that play Sudden Front over WebSocket, for load tests")]
struct Args {
    /// How many bots connect. Every ten share a room; the first of each ten makes it.
    #[arg(long)]
    count: usize,
    /// The server's WebSocket. Default: ws://localhost:<PORT or 8800>/ws.
    #[arg(long)]
    url: Option<String>,
    /// Seconds the run lasts; without it the bots stay until Ctrl+C.
    #[arg(long)]
    duration: Option<u64>,
    /// The game mode of the rooms the bots make.
    #[arg(long, value_enum, default_value_t = Mode::Normal)]
    mode: Mode,
    /// The map of the rooms the bots make.
    #[arg(long, value_enum, default_value_t = Map::SupplyYard)]
    map: Map,
    /// Team deathmatch or free-for-all rooms.
    #[arg(long, value_enum, default_value_t = MatchType::Team)]
    match_type: MatchType,
    /// Milliseconds between two bots connecting.
    #[arg(long, default_value_t = 50)]
    ramp_ms: u64,
    /// Port for Prometheus to scrape the bots' own counters at /metrics; 0 turns it off.
    #[arg(long, default_value_t = 9900)]
    metrics_port: u16,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Normal,
    Augment,
    BigHead,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Map {
    SupplyYard,
    Warehouse,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MatchType {
    Team,
    Ffa,
}

impl Args {
    fn settings(&self, run: u32) -> RoomSettings {
        RoomSettings {
            run,
            mode: match self.mode {
                Mode::Normal => pb::GameMode::Normal,
                Mode::Augment => pb::GameMode::Augment,
                Mode::BigHead => pb::GameMode::BigHead,
            },
            map: match self.map {
                Map::SupplyYard => pb::MapId::SupplyYard,
                Map::Warehouse => pb::MapId::Warehouse,
            },
            match_type: match self.match_type {
                MatchType::Team => pb::MatchType::TeamDeathmatch,
                MatchType::Ffa => pb::MatchType::FreeForAll,
            },
        }
    }

    fn url(&self) -> String {
        self.url.clone().unwrap_or_else(|| {
            let port = std::env::var("PORT").unwrap_or_else(|_| "8800".into());
            format!("ws://localhost:{}/ws", port.trim())
        })
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if args.count == 0 {
        eprintln!("--count must be at least 1.");
        std::process::exit(2);
    }
    let url = args.url();
    let mut rng = Rng::from_clock(std::process::id().into());
    let run = rng.next_u64() as u32;
    let settings = args.settings(run);
    let stats = Arc::new(Stats::default());
    let (stop_tx, stop_rx) = watch::channel(false);

    println!(
        "Load test {:04x}: {} bots -> {url} ({:?}, {:?}, {:?}), {}",
        run & 0xffff,
        args.count,
        args.mode,
        args.map,
        args.match_type,
        args.duration
            .map_or("until Ctrl+C".to_string(), |s| format!("{s} s")),
    );
    if args.metrics_port != 0 {
        tokio::spawn(serve_metrics(args.metrics_port, stats.clone()));
    }
    tokio::spawn(print_summaries(stats.clone()));

    let launcher = {
        let (stats, stop_rx, url) = (stats.clone(), stop_rx.clone(), url.clone());
        let (count, ramp) = (args.count, Duration::from_millis(args.ramp_ms));
        tokio::spawn(async move {
            let mut bots = Vec::with_capacity(count);
            for index in 0..count {
                if *stop_rx.borrow() {
                    break;
                }
                let name = format!("bot_{}", rng.index(100_000));
                let agent = Agent::new(
                    name,
                    plan::place(index, count),
                    settings,
                    Rng::new(rng.next_u64()),
                );
                bots.push(tokio::spawn(bot::run(
                    url.clone(),
                    tree::Mind::new(agent),
                    stats.clone(),
                    stop_rx.clone(),
                )));
                tokio::time::sleep(ramp).await;
            }
            bots
        })
    };

    let reason = match args.duration {
        Some(seconds) => tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(seconds)) => "duration reached",
            _ = tokio::signal::ctrl_c() => "Ctrl+C",
        },
        None => {
            let _ = tokio::signal::ctrl_c().await;
            "Ctrl+C"
        }
    };
    println!("Stopping ({reason}): the bots leave their rooms and disconnect.");
    let _ = stop_tx.send(true);
    let bots = launcher.await.unwrap_or_default();
    let all_left = async {
        for bot in bots {
            let _ = bot.await;
        }
    };
    if tokio::time::timeout(WIND_DOWN, all_left).await.is_err() {
        eprintln!("Some bots did not leave in time.");
    }
    println!("{}", stats.summary());
}

async fn print_summaries(stats: Arc<Stats>) {
    let mut ticker = tokio::time::interval(SUMMARY_EVERY);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        println!("{}", stats.summary());
    }
}

/// A minimal HTTP endpoint for Prometheus: any request gets the counters.
async fn serve_metrics(port: u16, stats: Arc<Stats>) {
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Metrics are off: cannot listen on port {port}: {error}");
            return;
        }
    };
    println!("Bot metrics: http://localhost:{port}/metrics");
    loop {
        let Ok((mut socket, _)) = listener.accept().await else {
            continue;
        };
        let body = stats.exposition();
        tokio::spawn(async move {
            // The request itself does not matter; read what came so the client sees a clean reply.
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
    }
}
