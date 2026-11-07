//! Prometheus metrics, served at `/metrics`: the hub's tick timing and population, the ECS worlds'
//! size, and the WebSocket traffic. On Linux (the Docker image) the process collector adds CPU
//! time, memory, threads and open files.

use crate::game::EcsStats;
use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounter, IntGauge, IntGaugeVec, Opts, Registry,
    TextEncoder,
};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// How long a tick-rate window lasts before the rate is reported.
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// Tick duration buckets (s), around the 33 ms a 30 Hz tick may take.
const TICK_BUCKETS: [f64; 12] = [
    0.000_1, 0.000_25, 0.000_5, 0.001, 0.002, 0.004, 0.008, 0.016, 0.033, 0.05, 0.1, 0.25,
];

/// What the hub holds at a moment, gathered once a second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HubStats {
    /// Open connections, said hello or not.
    pub clients: usize,
    pub rooms_waiting: usize,
    pub rooms_playing: usize,
    /// Rooms showing a match's results.
    pub rooms_results: usize,
    /// People and bots in the matches being played.
    pub humans_playing: usize,
    pub bots_playing: usize,
    /// The matches' ECS worlds, summed.
    pub ecs: EcsStats,
}

pub struct Metrics {
    registry: Registry,
    ticks: IntCounter,
    tick_seconds: Histogram,
    tick_overruns: IntCounter,
    tick_rate: prometheus::Gauge,
    clients: IntGauge,
    rooms: IntGaugeVec,
    players: IntGaugeVec,
    ecs_entities: IntGauge,
    ecs_archetypes: IntGauge,
    ecs_tables: IntGauge,
    ecs_components: IntGauge,
    pub connections: IntCounter,
    pub frames_in: IntCounter,
    pub bytes_in: IntCounter,
    pub frames_out: IntCounter,
    pub bytes_out: IntCounter,
    pub kicked: IntCounter,
}

static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::new);

/// The server's metrics.
pub fn metrics() -> &'static Metrics {
    &METRICS
}

fn counter(registry: &Registry, name: &str, help: &str) -> IntCounter {
    let metric = IntCounter::new(name, help).expect("valid counter");
    registry
        .register(Box::new(metric.clone()))
        .expect("unique metric");
    metric
}

fn gauge(registry: &Registry, name: &str, help: &str) -> IntGauge {
    let metric = IntGauge::new(name, help).expect("valid gauge");
    registry
        .register(Box::new(metric.clone()))
        .expect("unique metric");
    metric
}

fn gauge_vec(registry: &Registry, name: &str, help: &str, label: &str) -> IntGaugeVec {
    let metric = IntGaugeVec::new(Opts::new(name, help), &[label]).expect("valid gauge");
    registry
        .register(Box::new(metric.clone()))
        .expect("unique metric");
    metric
}

/// A count as a gauge value; nothing the hub counts comes near `i64::MAX`.
fn level(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

impl Metrics {
    fn new() -> Self {
        let registry =
            Registry::new_custom(Some("sudden_front".into()), None).expect("valid metric prefix");
        #[cfg(target_os = "linux")]
        registry
            .register(Box::new(
                prometheus::process_collector::ProcessCollector::for_self(),
            ))
            .expect("process collector");
        let tick_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "tick_duration_seconds",
                "Time one hub tick takes: every room's match and its snapshots.",
            )
            .buckets(TICK_BUCKETS.to_vec()),
        )
        .expect("valid histogram");
        registry
            .register(Box::new(tick_seconds.clone()))
            .expect("unique metric");
        let tick_rate = prometheus::Gauge::new(
            "tick_rate_hertz",
            "Hub ticks run in the last second; 30 when the server keeps up.",
        )
        .expect("valid gauge");
        registry
            .register(Box::new(tick_rate.clone()))
            .expect("unique metric");
        Self {
            ticks: counter(&registry, "ticks_total", "Hub ticks run."),
            tick_seconds,
            tick_overruns: counter(
                &registry,
                "tick_overruns_total",
                "Hub ticks that took longer than a tick's budget.",
            ),
            tick_rate,
            clients: gauge(&registry, "clients", "Open WebSocket connections."),
            rooms: gauge_vec(&registry, "rooms", "Rooms by phase.", "phase"),
            players: gauge_vec(
                &registry,
                "players_in_match",
                "Players in the matches being played, people or bots.",
                "kind",
            ),
            ecs_entities: gauge(
                &registry,
                "ecs_entities",
                "Entities in every match's world.",
            ),
            ecs_archetypes: gauge(
                &registry,
                "ecs_archetypes",
                "Archetypes in every match's world, summed.",
            ),
            ecs_tables: gauge(
                &registry,
                "ecs_tables",
                "Component tables in every match's world, summed.",
            ),
            ecs_components: gauge(
                &registry,
                "ecs_components",
                "Component types registered in every match's world, summed.",
            ),
            connections: counter(
                &registry,
                "connections_total",
                "WebSocket connections opened.",
            ),
            frames_in: counter(
                &registry,
                "ws_frames_in_total",
                "WebSocket frames received.",
            ),
            bytes_in: counter(
                &registry,
                "ws_bytes_in_total",
                "Bytes of WebSocket frames received.",
            ),
            frames_out: counter(&registry, "ws_frames_out_total", "WebSocket frames sent."),
            bytes_out: counter(
                &registry,
                "ws_bytes_out_total",
                "Bytes of WebSocket frames sent.",
            ),
            kicked: counter(
                &registry,
                "clients_kicked_total",
                "Connections closed because their send queue filled up.",
            ),
            registry,
        }
    }

    /// One hub tick that took `took` of a `budget`.
    pub fn record_tick(&self, took: Duration, budget: Duration) {
        self.ticks.inc();
        self.tick_seconds.observe(took.as_secs_f64());
        if took > budget {
            self.tick_overruns.inc();
        }
    }

    pub fn record_tick_rate(&self, hertz: f64) {
        self.tick_rate.set(hertz);
    }

    pub fn record_hub(&self, stats: &HubStats) {
        self.clients.set(level(stats.clients));
        for (phase, count) in [
            ("waiting", stats.rooms_waiting),
            ("playing", stats.rooms_playing),
            ("results", stats.rooms_results),
        ] {
            self.rooms.with_label_values(&[phase]).set(level(count));
        }
        self.players
            .with_label_values(&["human"])
            .set(level(stats.humans_playing));
        self.players
            .with_label_values(&["bot"])
            .set(level(stats.bots_playing));
        self.ecs_entities.set(level(stats.ecs.entities));
        self.ecs_archetypes.set(level(stats.ecs.archetypes));
        self.ecs_tables.set(level(stats.ecs.tables));
        self.ecs_components.set(level(stats.ecs.components));
    }

    /// The Prometheus text exposition of every metric.
    pub fn render(&self) -> String {
        let mut out = Vec::new();
        if let Err(error) = TextEncoder::new().encode(&self.registry.gather(), &mut out) {
            return format!("# metrics could not be encoded: {error}\n");
        }
        String::from_utf8(out).unwrap_or_default()
    }
}

/// Counts ticks over a window and gives their rate once the window is over: the server's "frame
/// rate", which drops below the tick rate when ticks take too long.
pub struct TickMeter {
    since: Instant,
    ticks: u32,
}

impl TickMeter {
    pub fn new(now: Instant) -> Self {
        Self {
            since: now,
            ticks: 0,
        }
    }

    /// Counts a tick at `now`; once `RATE_WINDOW` has passed, the ticks a second over it, and a
    /// new window starts.
    pub fn tick(&mut self, now: Instant) -> Option<f64> {
        self.ticks = self.ticks.saturating_add(1);
        let elapsed = now.saturating_duration_since(self.since);
        if elapsed < RATE_WINDOW {
            return None;
        }
        let rate = f64::from(self.ticks) / elapsed.as_secs_f64();
        *self = Self::new(now);
        Some(rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tick_rate_is_given_once_a_second_from_the_ticks_in_it() {
        let start = Instant::now();
        let mut meter = TickMeter::new(start);
        let step = Duration::from_millis(50);
        let mut rates = Vec::new();
        let mut now = start;
        for _ in 0..40 {
            now += step;
            rates.extend(meter.tick(now));
        }
        // 20 ticks a second over two seconds.
        assert_eq!(rates.len(), 2);
        for rate in rates {
            assert!((rate - 20.0).abs() < 1e-6, "{rate}");
        }
    }

    #[test]
    fn a_stalled_hub_shows_a_low_tick_rate() {
        let start = Instant::now();
        let mut meter = TickMeter::new(start);
        let rate = meter.tick(start + Duration::from_secs(4));
        assert_eq!(rate, Some(0.25));
    }

    #[test]
    fn a_slow_tick_counts_as_an_overrun_and_lands_in_the_histogram() {
        let metrics = Metrics::new();
        let budget = Duration::from_millis(33);
        metrics.record_tick(Duration::from_millis(5), budget);
        metrics.record_tick(Duration::from_millis(40), budget);
        assert_eq!(metrics.ticks.get(), 2);
        assert_eq!(metrics.tick_overruns.get(), 1);
        assert_eq!(metrics.tick_seconds.get_sample_count(), 2);
    }

    #[test]
    fn the_exposition_carries_the_hub_and_ecs_numbers_under_the_server_prefix() {
        let metrics = Metrics::new();
        metrics.record_hub(&HubStats {
            clients: 3,
            rooms_playing: 1,
            humans_playing: 2,
            bots_playing: 8,
            ecs: EcsStats {
                entities: 10,
                archetypes: 4,
                tables: 3,
                components: 20,
            },
            ..Default::default()
        });
        let text = metrics.render();
        for line in [
            "sudden_front_clients 3",
            "sudden_front_rooms{phase=\"playing\"} 1",
            "sudden_front_rooms{phase=\"waiting\"} 0",
            "sudden_front_players_in_match{kind=\"bot\"} 8",
            "sudden_front_ecs_entities 10",
            "sudden_front_ecs_archetypes 4",
        ] {
            assert!(text.contains(line), "no {line} in\n{text}");
        }
    }
}
