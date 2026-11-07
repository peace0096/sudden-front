mod game;
mod hub;
mod metrics;
mod net;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/game.v1.rs"));
}

use axum::{routing::get, Router};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tower_http::services::{ServeDir, ServeFile};

/// Where the server listens unless `PORT` says otherwise.
const DEFAULT_PORT: u16 = 8800;
/// How many ports from the default are tried before the system is left to choose one.
const PORT_TRIES: u16 = 20;
/// How long a probe waits for something on a port to answer.
const PROBE_TIMEOUT: Duration = Duration::from_millis(200);

#[tokio::main]
async fn main() {
    let raw = std::env::var_os("PORT").map(|value| value.to_string_lossy().into_owned());
    let requested = parse_port(raw.as_deref()).unwrap_or_else(|message| fail(&message));
    let listener = listen(requested)
        .await
        .unwrap_or_else(|message| fail(&message));
    let port = listener.local_addr().expect("listener address").port();
    if requested.is_none() && port != DEFAULT_PORT {
        println!(
            "Port {DEFAULT_PORT} is in use; using {port}. Run the dev proxy and e2e with PORT={port}."
        );
    }

    // Every room's ECS world is owned by this one thread, ticking at 30 Hz.
    let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("game-hub".into())
        .spawn(move || hub::run(inbound_rx))
        .expect("spawn game thread");

    let dist = std::env::var("CLIENT_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../client/dist"));

    println!("Sudden Front server: http://localhost:{port}");
    axum::serve(listener, app(&dist, inbound_tx))
        .await
        .expect("server error");
}

/// The WebSocket, the metrics, then the client's files from `dist`. A path that matches no file gets the page,
/// except under /art: a missing art file is a 404, which the client reports as a failed asset
/// instead of trying to read index.html as a model.
fn app(dist: &Path, hub: Sender<hub::Inbound>) -> Router {
    let static_files = ServeDir::new(dist).fallback(ServeFile::new(dist.join("index.html")));
    Router::new()
        .route("/ws", get(net::ws_handler))
        .route("/metrics", get(serve_metrics))
        .nest_service("/art", ServeDir::new(dist.join("art")))
        .fallback_service(static_files)
        .with_state(hub)
}

/// The metrics in Prometheus' text format, for a scrape.
async fn serve_metrics() -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        metrics::metrics().render(),
    )
}

/// Prints a one-line error and exits, without a panic's backtrace.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

/// The port `PORT` asks for, if it is set to anything.
fn parse_port(raw: Option<&str>) -> Result<Option<u16>, String> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(text) => text
            .parse()
            .map(Some)
            .map_err(|_| format!("PORT={text} is not a port number (0 to 65535).")),
    }
}

/// Opens the port the server answers on. With `PORT` set (`requested`) it is exactly that port,
/// or an error: the port was chosen, and scripts may rely on it. Otherwise it is the first free
/// port from the default on.
async fn listen(requested: Option<u16>) -> Result<TcpListener, String> {
    match requested {
        Some(port) => bind(port).await.map_err(|error| match error.kind() {
            io::ErrorKind::AddrInUse => {
                format!("Port {port} is in use. Stop what uses it, or set PORT to a free port.")
            }
            _ => format!("Cannot listen on port {port}: {error}."),
        }),
        None => first_free(DEFAULT_PORT, PORT_TRIES)
            .await
            .map_err(|error| format!("Cannot listen on any port: {error}.")),
    }
}

/// The first free port of `tries` from `first` on, or else one the system picks.
async fn first_free(first: u16, tries: u16) -> io::Result<TcpListener> {
    for port in first..first.saturating_add(tries) {
        match bind(port).await {
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => continue,
            result => return result,
        }
    }
    bind(0).await
}

/// Binds `port` on every IPv4 address, unless something already answers on this machine's loopback.
async fn bind(port: u16) -> io::Result<TcpListener> {
    if port != 0 && is_busy(port).await {
        return Err(io::ErrorKind::AddrInUse.into());
    }
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await
}

/// Whether something listens on `port` of the loopback. Binding alone is not a good enough test:
/// on macOS a bind to every address can succeed beside a listener on just 127.0.0.1 or ::1, and
/// then `localhost` would reach that other process. Connecting finds it.
async fn is_busy(port: u16) -> bool {
    for ip in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        let connect = TcpStream::connect(SocketAddr::new(ip, port));
        match tokio::time::timeout(PROBE_TIMEOUT, connect).await {
            // Something answered, or took the connection and never replied: the port is taken.
            Ok(Ok(_)) | Err(_) => return true,
            // Refused, or no such address on this machine: nothing listens there.
            Ok(Err(_)) => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    const LOCALHOST_V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

    /// A listener on a port the system picked, on `ip` only. `None` if this machine has no such address.
    async fn hold(ip: IpAddr) -> Option<(TcpListener, u16)> {
        let listener = TcpListener::bind((ip, 0)).await.ok()?;
        let port = listener.local_addr().ok()?.port();
        Some((listener, port))
    }

    async fn free_port() -> u16 {
        let (held, port) = hold(LOCALHOST).await.unwrap();
        drop(held);
        port
    }

    fn port_of(listener: &TcpListener) -> u16 {
        listener.local_addr().unwrap().port()
    }

    #[test]
    fn port_asks_are_read_strictly_but_empty_means_none() {
        assert_eq!(parse_port(None), Ok(None));
        assert_eq!(parse_port(Some("")), Ok(None));
        assert_eq!(parse_port(Some("  ")), Ok(None));
        assert_eq!(parse_port(Some("8801")), Ok(Some(8801)));
        assert_eq!(parse_port(Some(" 8801 ")), Ok(Some(8801)));
        assert_eq!(parse_port(Some("0")), Ok(Some(0)));
        for bad in ["abc", "70000", "-1", "88 01"] {
            let message = parse_port(Some(bad)).unwrap_err();
            assert!(message.contains(bad), "{message}");
        }
    }

    #[tokio::test]
    async fn the_default_range_skips_a_port_that_is_held() {
        let (_held, port) = hold(IpAddr::V4(Ipv4Addr::UNSPECIFIED)).await.unwrap();
        let listener = first_free(port, 3).await.unwrap();
        assert_ne!(port_of(&listener), port);
    }

    #[tokio::test]
    async fn the_default_range_takes_a_free_port_first() {
        let port = free_port().await;
        let listener = first_free(port, 3).await.unwrap();
        assert_eq!(port_of(&listener), port);
    }

    #[tokio::test]
    async fn a_port_held_on_loopback_alone_counts_as_taken() {
        // The case a plain bind to every address misses on macOS.
        for ip in [LOCALHOST, LOCALHOST_V6] {
            let Some((_held, port)) = hold(ip).await else {
                continue;
            };
            assert!(is_busy(port).await, "{ip} was not seen as busy");
            let listener = first_free(port, 3).await.unwrap();
            assert_ne!(port_of(&listener), port);
        }
    }

    #[tokio::test]
    async fn a_free_port_is_not_busy() {
        assert!(!is_busy(free_port().await).await);
    }

    #[tokio::test]
    async fn the_system_picks_when_every_port_in_the_range_is_taken() {
        let (_first, port) = hold(LOCALHOST).await.unwrap();
        let listener = first_free(port, 1).await.unwrap();
        assert_ne!(port_of(&listener), port);
        assert_ne!(port_of(&listener), 0);
    }

    #[tokio::test]
    async fn an_explicit_port_that_is_busy_is_an_error() {
        let (_held, port) = hold(LOCALHOST).await.unwrap();
        let message = listen(Some(port)).await.unwrap_err();
        assert!(
            message.contains(&port.to_string()) && message.contains("in use"),
            "{message}"
        );
        assert!(!message.contains('\n'), "one line, please: {message}");
    }

    #[tokio::test]
    async fn an_explicit_free_port_is_bound_exactly() {
        let port = free_port().await;
        let listener = listen(Some(port)).await.unwrap();
        assert_eq!(port_of(&listener), port);
    }

    /// The status code and body of a plain HTTP/1.1 GET.
    fn fetch(port: u16, path: &str) -> (u16, String) {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect((LOCALHOST, port)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response.split(' ').nth(1).unwrap().parse().unwrap();
        let body = response.split_once("\r\n\r\n").map_or("", |(_, body)| body);
        (status, body.to_owned())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_missing_art_file_is_a_404_while_other_unknown_paths_get_the_page() {
        let dist = std::env::temp_dir().join(format!("sudden-front-art-{}", std::process::id()));
        std::fs::create_dir_all(dist.join("art/weapons")).unwrap();
        std::fs::write(dist.join("index.html"), "page").unwrap();
        std::fs::write(dist.join("art/weapons/rifle.glb"), "glTF").unwrap();
        let (hub, _inbound) = std::sync::mpsc::channel();
        let router = app(&dist, hub);
        let listener = TcpListener::bind((LOCALHOST, 0)).await.unwrap();
        let port = port_of(&listener);
        tokio::spawn(async move { axum::serve(listener, router).await });

        let answers = tokio::task::spawn_blocking(move || {
            [
                "/art/weapons/rifle.glb?v=1a2b",
                "/art/weapons/missing.glb",
                "/art/",
                "/somewhere/else",
            ]
            .map(|path| fetch(port, path))
        })
        .await
        .unwrap();
        std::fs::remove_dir_all(&dist).unwrap();

        assert_eq!(answers[0], (200, "glTF".to_owned()));
        assert_eq!(answers[1].0, 404);
        assert_eq!(answers[2].0, 404);
        assert_eq!(answers[3], (200, "page".to_owned()));
    }
}
