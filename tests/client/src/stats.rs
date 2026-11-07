//! Counters every bot of a run adds to, printed as a summary line and served to Prometheus.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Stats {
    /// Bots whose WebSocket is open.
    pub connected: AtomicU64,
    /// Bots seated in a room, waiting or playing.
    pub in_room: AtomicU64,
    /// Bots playing a match.
    pub in_match: AtomicU64,
    pub connect_failures: AtomicU64,
    pub disconnects: AtomicU64,
    pub server_errors: AtomicU64,
    pub matches_started: AtomicU64,
    pub matches_ended: AtomicU64,
    pub snapshots: AtomicU64,
    pub kills: AtomicU64,
    pub frames_in: AtomicU64,
    pub bytes_in: AtomicU64,
    pub frames_out: AtomicU64,
    pub bytes_out: AtomicU64,
    /// Round trips of Ping and Pong: how many, their total, and the longest since the last
    /// summary, in microseconds.
    pub rtt_count: AtomicU64,
    pub rtt_total_us: AtomicU64,
    pub rtt_max_us: AtomicU64,
}

fn get(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

pub fn add(counter: &AtomicU64, amount: u64) {
    counter.fetch_add(amount, Ordering::Relaxed);
}

pub fn sub(counter: &AtomicU64, amount: u64) {
    // Saturating: a gauge never goes below zero, even if a decrement races ahead.
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_sub(amount))
    });
}

impl Stats {
    pub fn record_rtt(&self, micros: u64) {
        add(&self.rtt_count, 1);
        add(&self.rtt_total_us, micros);
        self.rtt_max_us.fetch_max(micros, Ordering::Relaxed);
    }

    /// One line for the console: the gauges and the mean round trip so far.
    pub fn summary(&self) -> String {
        let rtt = get(&self.rtt_total_us)
            .checked_div(get(&self.rtt_count))
            .unwrap_or(0);
        format!(
            "connected {} | in room {} | in match {} | matches {} started, {} ended | snapshots {} | kills {} | rtt avg {:.1} ms, max {:.1} ms | errors {} | dropped {}",
            get(&self.connected),
            get(&self.in_room),
            get(&self.in_match),
            get(&self.matches_started),
            get(&self.matches_ended),
            get(&self.snapshots),
            get(&self.kills),
            rtt as f64 / 1000.0,
            self.rtt_max_us.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            get(&self.server_errors),
            get(&self.disconnects),
        )
    }

    /// The Prometheus text exposition of every counter, under the `loadtest_` prefix.
    pub fn exposition(&self) -> String {
        let gauges = [
            (
                "loadtest_bots_connected",
                "Bots whose WebSocket is open.",
                &self.connected,
            ),
            (
                "loadtest_bots_in_room",
                "Bots seated in a room.",
                &self.in_room,
            ),
            (
                "loadtest_bots_in_match",
                "Bots playing a match.",
                &self.in_match,
            ),
        ];
        let counters = [
            (
                "loadtest_connect_failures_total",
                "Connections that failed to open.",
                &self.connect_failures,
            ),
            (
                "loadtest_disconnects_total",
                "Connections the server closed before the bot left.",
                &self.disconnects,
            ),
            (
                "loadtest_server_errors_total",
                "ErrorMessage frames received.",
                &self.server_errors,
            ),
            (
                "loadtest_matches_started_total",
                "MatchStart messages received.",
                &self.matches_started,
            ),
            (
                "loadtest_matches_ended_total",
                "MatchEnd messages received.",
                &self.matches_ended,
            ),
            (
                "loadtest_snapshots_total",
                "Snapshots received.",
                &self.snapshots,
            ),
            (
                "loadtest_frames_in_total",
                "WebSocket frames received.",
                &self.frames_in,
            ),
            (
                "loadtest_bytes_in_total",
                "Bytes of WebSocket frames received.",
                &self.bytes_in,
            ),
            (
                "loadtest_frames_out_total",
                "WebSocket frames sent.",
                &self.frames_out,
            ),
            (
                "loadtest_bytes_out_total",
                "Bytes of WebSocket frames sent.",
                &self.bytes_out,
            ),
            (
                "loadtest_rtt_count_total",
                "Ping round trips measured.",
                &self.rtt_count,
            ),
        ];
        let mut out = String::new();
        for (name, help, value) in gauges {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {}",
                get(value)
            );
        }
        for (name, help, value) in counters {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} counter\n{name} {}",
                get(value)
            );
        }
        let _ = writeln!(
            out,
            "# HELP loadtest_rtt_seconds_total Sum of the measured Ping round trips.\n# TYPE loadtest_rtt_seconds_total counter\nloadtest_rtt_seconds_total {}",
            get(&self.rtt_total_us) as f64 / 1e6
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gauge_never_drops_below_zero() {
        let stats = Stats::default();
        add(&stats.connected, 1);
        sub(&stats.connected, 3);
        assert_eq!(get(&stats.connected), 0);
    }

    #[test]
    fn the_summary_averages_the_round_trips_and_then_forgets_the_longest() {
        let stats = Stats::default();
        stats.record_rtt(10_000);
        stats.record_rtt(30_000);
        let line = stats.summary();
        assert!(line.contains("rtt avg 20.0 ms, max 30.0 ms"), "{line}");
        assert!(stats.summary().contains("max 0.0 ms"));
    }

    #[test]
    fn the_exposition_names_every_metric_with_its_type_and_value() {
        let stats = Stats::default();
        add(&stats.snapshots, 42);
        let text = stats.exposition();
        assert!(
            text.contains("# TYPE loadtest_snapshots_total counter\nloadtest_snapshots_total 42\n")
        );
        assert!(text.contains("# TYPE loadtest_bots_connected gauge\n"));
    }
}
