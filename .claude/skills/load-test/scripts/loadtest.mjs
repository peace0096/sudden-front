#!/usr/bin/env node
// Load test runner for the load-test skill: brings up the Docker stack (game server, Prometheus,
// Grafana), runs the tests/client bots for a fixed time, and writes a Markdown report from what
// Prometheus collected (report.md: verdict + analysis, report.html: charts
// and tables). Node 22, no dependencies.
//
//   node loadtest.mjs up [--no-build]                 Docker stack up, wait until scraped
//   node loadtest.mjs run --count 50 --minutes 5      bots for 5 minutes, then the report
//   node loadtest.mjs report --dir <run dir>          query Prometheus again for a past run
//   node loadtest.mjs render --dir <run dir>          report.md + report.html from metrics.json
//   node loadtest.mjs status                          one line of what the server does now
//   node loadtest.mjs down                            Docker stack down (the data volume stays)
//
// Every command takes --target local (the only one for now: the Pulumi VM is not deployed yet).

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { htmlReport } from "./report-html.mjs";

const ROOT = path.resolve(import.meta.dirname, "..", "..", "..", "..");
const COMPOSE = path.join(ROOT, "infra", "docker-compose.yml");
const CLIENT = path.join(ROOT, "tests", "client");
const REPORTS = path.join(ROOT, "tests", "reports");
const BOT_EXE = path.join(CLIENT, "target", "release", process.platform === "win32" ? "loadtest.exe" : "loadtest");
const DOCKER_DESKTOP = "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe";

/** How often the run prints a status line. */
const STATUS_EVERY_S = 30;
/** Seconds after the bots exit before the report queries, so Prometheus has scraped the end. */
const SETTLE_S = 12;
/** The tick budget of a 30 Hz server (s). */
const TICK_BUDGET = 1 / 30;

// ---- arguments

function parseArgs(argv) {
  const [command = "help", ...rest] = argv;
  const opts = {};
  for (let i = 0; i < rest.length; i += 1) {
    const arg = rest[i];
    if (!arg.startsWith("--")) throw new Error(`unexpected argument: ${arg}`);
    const key = arg.slice(2);
    const next = rest[i + 1];
    if (next === undefined || next.startsWith("--")) opts[key] = true;
    else {
      opts[key] = next;
      i += 1;
    }
  }
  return { command, opts };
}

function targetOf(opts) {
  const target = opts.target ?? "local";
  if (target === "local") {
    const serverPort = Number(opts["server-port"] ?? 8800);
    return {
      name: "local",
      prom: opts.prometheus ?? "http://127.0.0.1:9090",
      grafana: opts.grafana ?? "http://localhost:3000",
      server: `http://127.0.0.1:${serverPort}`,
      ws: `ws://127.0.0.1:${serverPort}/ws`,
    };
  }
  if (target === "remote") {
    throw new Error(
      "target 'remote' is not supported yet: the Pulumi VM (infra/pulumi) has not been deployed. Use --target local.",
    );
  }
  throw new Error(`unknown target: ${target} (local or remote)`);
}

// ---- small helpers

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const stamp = (date) => date.toLocaleString("sv-SE").replace("T", " ");
const log = (line) => console.log(`[${stamp(new Date()).slice(11)}] ${line}`);

function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { encoding: "utf8", cwd: ROOT, ...options });
  return { ok: result.status === 0, out: `${result.stdout ?? ""}${result.stderr ?? ""}`.trim(), status: result.status };
}

async function fetchText(url, timeoutMs = 3000) {
  const response = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.text();
}

// ---- Prometheus

async function promGet(target, api, params) {
  const url = new URL(`${target.prom}/api/v1/${api}`);
  for (const [key, value] of Object.entries(params)) url.searchParams.set(key, String(value));
  const response = await fetch(url, { signal: AbortSignal.timeout(15000) });
  const body = await response.json();
  if (body.status !== "success") throw new Error(`Prometheus ${api} ${params.query ?? ""}: ${body.error ?? response.status}`);
  return body.data;
}

/** One number from an instant query (the first series), or null when there is none. */
async function scalar(target, query, time) {
  const data = await promGet(target, "query", time === undefined ? { query } : { query, time });
  const first = data.result[0];
  if (!first) return null;
  const value = Number(first.value[1]);
  return Number.isFinite(value) ? value : null;
}

/** Instant vector: [{ metric, value }]. */
async function vector(target, query, time) {
  const data = await promGet(target, "query", time === undefined ? { query } : { query, time });
  return data.result.map((row) => ({ metric: row.metric, value: Number(row.value[1]) }));
}

/** Range query of a single-series expression: [[unix seconds, value]]. */
async function series(target, query, start, end, step) {
  const data = await promGet(target, "query_range", { query, start, end, step });
  const first = data.result[0];
  if (!first) return [];
  return first.values.map(([t, v]) => [Number(t), Number(v)]).filter(([, v]) => Number.isFinite(v));
}

function describe(points) {
  if (points.length === 0) return null;
  const values = points.map(([, v]) => v);
  const sum = values.reduce((a, b) => a + b, 0);
  return {
    min: Math.min(...values),
    avg: sum / values.length,
    max: Math.max(...values),
    first: values[0],
    last: values[values.length - 1],
  };
}

const within = (points, from, to) => points.filter(([t]) => t >= from && t <= to);

// ---- the Docker stack

function dockerUp() {
  // `docker info --format` exits 0 even when it cannot reach the daemon: look for the version.
  const info = run("docker", ["info", "--format", "{{.ServerVersion}}"]);
  return info.ok && /^\d+\.\d+/.test(info.out);
}

async function ensureDocker() {
  if (dockerUp()) return;
  if (process.platform === "win32" && fs.existsSync(DOCKER_DESKTOP)) {
    log("Docker is not running: starting Docker Desktop");
    spawn(DOCKER_DESKTOP, [], { detached: true, stdio: "ignore" }).unref();
    for (let i = 0; i < 60; i += 1) {
      await sleep(3000);
      if (dockerUp()) return;
    }
  }
  throw new Error("Docker is not running. Start Docker Desktop (or the Docker daemon) and try again.");
}

async function waitReady(target, timeoutS = 180) {
  const deadline = Date.now() + timeoutS * 1000;
  let last = "";
  while (Date.now() < deadline) {
    try {
      await fetchText(`${target.server}/metrics`);
      const targets = await promGet(target, "targets", { state: "active" });
      const server = targets.activeTargets.filter((t) => t.labels.job === "sudden-front-server");
      if (server.length > 0 && server.every((t) => t.health === "up")) return;
      last = server.length === 0 ? "Prometheus has no server target yet" : `server target ${server[0].health}`;
    } catch (error) {
      last = error.message;
    }
    await sleep(2000);
  }
  throw new Error(`the stack did not become ready in ${timeoutS}s: ${last}`);
}

async function commandUp(opts) {
  const target = targetOf(opts);
  await ensureDocker();
  const args = ["compose", "-f", COMPOSE, "up", "-d"];
  if (!opts["no-build"]) args.push("--build");
  log(`docker ${args.join(" ")}`);
  const result = spawnSync("docker", args, { cwd: ROOT, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error("docker compose up failed. Is another server holding port 8800 (a local cargo run)?");
  }
  await waitReady(target);
  log(`ready: game ${target.server}, Prometheus ${target.prom}, Grafana ${target.grafana}`);
}

function commandDown() {
  const result = spawnSync("docker", ["compose", "-f", COMPOSE, "down"], { cwd: ROOT, stdio: "inherit" });
  if (result.status !== 0) throw new Error("docker compose down failed");
}

// ---- status

async function statusLine(target) {
  const [clients, rate, p99, cpu, rss, inMatch, errors] = await Promise.all([
    scalar(target, "sum(sudden_front_clients)"),
    scalar(target, "min(sudden_front_tick_rate_hertz)"),
    scalar(target, "histogram_quantile(0.99, sum by (le) (rate(sudden_front_tick_duration_seconds_bucket[30s])))"),
    scalar(target, "sum(rate(sudden_front_process_cpu_seconds_total[30s]))"),
    scalar(target, "sum(sudden_front_process_resident_memory_bytes)"),
    scalar(target, "sum(loadtest_bots_in_match)"),
    scalar(target, "sum(loadtest_server_errors_total) + sum(loadtest_disconnects_total)"),
  ]);
  const show = (v, f) => (v === null ? "-" : f(v));
  return [
    `clients ${show(clients, (v) => v.toFixed(0))}`,
    `bots in match ${show(inMatch, (v) => v.toFixed(0))}`,
    `tick ${show(rate, (v) => v.toFixed(1))} Hz`,
    `p99 ${show(p99, (v) => (v * 1000).toFixed(2))} ms`,
    `cpu ${show(cpu, (v) => (v * 100).toFixed(0))}%`,
    `rss ${show(rss, (v) => (v / 2 ** 20).toFixed(1))} MB`,
    `bot errors+drops ${show(errors, (v) => v.toFixed(0))}`,
  ].join(" | ");
}

async function commandStatus(opts) {
  const target = targetOf(opts);
  console.log(await statusLine(target));
}

// ---- run

function gitInfo() {
  const commit = run("git", ["rev-parse", "--short", "HEAD"]).out || "?";
  const subject = run("git", ["log", "-1", "--format=%s"]).out || "";
  const dirty = run("git", ["status", "--porcelain", "--", "server", "proto", "client/src", "tests/client/src"]).out !== "";
  return { commit, subject, dirty };
}

function dockerInfo() {
  const info = run("docker", ["info", "--format", "{{.NCPU}} {{.MemTotal}} {{.OperatingSystem}}"]);
  if (!info.ok) return null;
  const [cpus, mem, ...osName] = info.out.split(" ");
  const image = run("docker", ["image", "inspect", "sudden-front-server:local", "--format", "{{.Created}}"]);
  return { cpus: Number(cpus), memBytes: Number(mem), os: osName.join(" "), imageCreated: image.ok ? image.out : null };
}

async function commandRun(opts) {
  const target = targetOf(opts);
  const count = Number(opts.count);
  const minutes = Number(opts.minutes);
  if (!Number.isInteger(count) || count < 1) throw new Error("--count <bots> is required (a whole number ≥ 1)");
  if (!(minutes > 0)) throw new Error("--minutes <minutes> is required (> 0)");
  const durationS = Math.max(1, Math.round(minutes * 60));
  const bot = {
    count,
    minutes,
    mode: opts.mode ?? "normal",
    map: opts.map ?? "supply-yard",
    matchType: opts["match-type"] ?? "team",
    rampMs: Number(opts["ramp-ms"] ?? 50),
  };

  if (!dockerUp()) throw new Error("Docker is not running: run `up` first");
  await waitReady(target, 30);
  const busy = await scalar(target, "sum(sudden_front_clients)");
  if (busy && busy > 0) log(`warning: the server already has ${busy} connection(s); they count in this run`);

  log("building the bots (cargo build --release)");
  const build = spawnSync("cargo", ["build", "--release"], { cwd: CLIENT, stdio: "inherit" });
  if (build.status !== 0) throw new Error("cargo build --release of tests/client failed");

  const label = `${stamp(new Date()).replace(/[-: ]/g, "").replace(/^(\d{8})(\d{6})$/, "$1-$2")}-${count}bots-${minutes}m`;
  const dir = path.resolve(opts.out ?? path.join(REPORTS, label));
  fs.mkdirSync(dir, { recursive: true });
  const botLog = fs.createWriteStream(path.join(dir, "bots.log"));

  const args = [
    "--count", String(count),
    "--duration", String(durationS),
    "--url", target.ws,
    "--mode", bot.mode,
    "--map", bot.map,
    "--match-type", bot.matchType,
    "--ramp-ms", String(bot.rampMs),
    "--metrics-port", "9900",
  ];
  const meta = {
    target: target.name,
    targetUrls: target,
    bot,
    botArgs: args,
    git: gitInfo(),
    docker: dockerInfo(),
    host: { os: `${os.type()} ${os.release()}`, cpus: os.cpus().length, cpuModel: os.cpus()[0]?.model ?? "?" },
    dir,
  };

  log(`starting ${count} bots for ${minutes} min → ${dir}`);
  const start = Date.now() / 1000;
  const child = spawn(BOT_EXE, args, { cwd: CLIENT, stdio: ["ignore", "pipe", "pipe"] });
  const relay = (chunk) => {
    botLog.write(chunk);
    for (const line of chunk.toString().split(/\r?\n/)) if (line.trim()) log(`[bots] ${line}`);
  };
  child.stdout.on("data", relay);
  child.stderr.on("data", relay);
  const stopChild = () => child.kill();
  process.on("SIGINT", stopChild);
  process.on("SIGTERM", stopChild);

  const alerts = [];
  let downStreak = 0;
  const timer = setInterval(async () => {
    try {
      const up = await scalar(target, 'min(up{job="sudden-front-server"})');
      downStreak = up === 1 ? 0 : downStreak + 1;
      if (downStreak === 3) {
        const at = Math.round(Date.now() / 1000 - start);
        alerts.push(`${at}s: Prometheus could not scrape the game server three times in a row`);
      }
      log(await statusLine(target));
    } catch (error) {
      log(`status failed: ${error.message}`);
    }
  }, STATUS_EVERY_S * 1000);

  const exitCode = await new Promise((resolve) => child.on("exit", (code) => resolve(code ?? 1)));
  const end = Date.now() / 1000;
  clearInterval(timer);
  botLog.end();
  if (exitCode !== 0) alerts.push(`the bot client exited with code ${exitCode}`);
  log(`bots exited (${exitCode}); waiting ${SETTLE_S}s for the last scrape`);
  await sleep(SETTLE_S * 1000);

  const runInfo = { ...meta, start, stop: start + durationS, end, exitCode, alerts };
  fs.writeFileSync(path.join(dir, "run.json"), `${JSON.stringify(runInfo, null, 2)}\n`);
  const { reportFile, htmlFile } = await writeReport(target, runInfo);
  log(`report: ${reportFile}`);
  log(`html: ${htmlFile}`);
  if (exitCode !== 0) process.exitCode = 1;
}

async function commandReport(opts) {
  if (!opts.dir) throw new Error("--dir <run dir> is required");
  const dir = path.resolve(opts.dir);
  const runInfo = JSON.parse(fs.readFileSync(path.join(dir, "run.json"), "utf8"));
  const target = targetOf({ ...opts, target: runInfo.target });
  const { reportFile, htmlFile } = await writeReport(target, { ...runInfo, dir });
  log(`report: ${reportFile}`);
  log(`html: ${htmlFile}`);
}

/** Rebuilds report.md and report.html from metrics.json (no Prometheus), keeping the analysis. */
function commandRender(opts) {
  if (!opts.dir) throw new Error("--dir <run dir> is required");
  const { reportFile, htmlFile } = render(path.resolve(opts.dir));
  log(`report: ${reportFile}`);
  log(`html: ${htmlFile}`);
}

// ---- the report

const fmt = {
  n: (v, digits = 0) => (v === null || v === undefined ? "-" : v.toLocaleString("en-US", { maximumFractionDigits: digits, minimumFractionDigits: digits })),
  ms: (v) => (v === null || v === undefined ? "-" : `${(v * 1000).toFixed(2)} ms`),
  pct: (v, digits = 1) => (v === null || v === undefined ? "-" : `${(v * 100).toFixed(digits)}%`),
  mb: (v) => (v === null || v === undefined ? "-" : `${(v / 2 ** 20).toFixed(1)} MB`),
  kbs: (v) => (v === null || v === undefined ? "-" : `${(v / 1024).toFixed(1)} KB/s`),
  dur: (s) => `${Math.floor(s / 60)}분 ${Math.round(s % 60)}초`,
  clock: (s) => `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(Math.round(s % 60)).padStart(2, "0")}`,
};

const GOOD = "✅ 정상";
const WARN = "⚠️ 주의";
const BAD = "❌ 문제";
const rank = { [GOOD]: 0, [WARN]: 1, [BAD]: 2, "-": 0 };

function grade(value, good, warn, higherIsBetter = false) {
  if (value === null || value === undefined) return "-";
  if (higherIsBetter) return value >= good ? GOOD : value >= warn ? WARN : BAD;
  return value <= good ? GOOD : value <= warn ? WARN : BAD;
}

async function collect(target, run) {
  const { start, stop, end, bot } = run;
  const total = Math.max(1, end - start);
  const step = Math.max(5, Math.round(total / 400));
  const q = (query) => series(target, query, Math.floor(start), Math.ceil(end), step);

  const [humans, botsInMatch] = await Promise.all([
    q('sum(sudden_front_players_in_match{kind="human"})'),
    q("sum(loadtest_bots_in_match)"),
  ]);
  // Steady state: from the first moment (nearly) every bot plays, until the run's stop signal.
  const threshold = Math.max(1, Math.floor(bot.count * 0.9));
  const filled = humans.find(([, v]) => v >= threshold) ?? botsInMatch.find(([, v]) => v >= threshold);
  const steadyStart = Math.min(filled ? filled[0] : start + Math.min(60, (stop - start) / 5), stop - 1);
  const steadyEnd = stop;
  const steadyS = Math.max(5, Math.round(steadyEnd - steadyStart));
  const W = `${steadyS}s`;
  const at = Math.round(steadyEnd);
  const totalW = `${Math.round(total)}s`;
  const atEnd = Math.round(end);

  const [rate, cpu, rss, threads, fds, clients, outBytes, inBytes, outFrames, inFrames, entities, archetypes, tables, components, snapshotsRate, rttSeries, botsHumans, serverBots] =
    await Promise.all([
      q("min(sudden_front_tick_rate_hertz)"),
      q("sum(rate(sudden_front_process_cpu_seconds_total[30s]))"),
      q("sum(sudden_front_process_resident_memory_bytes)"),
      q("sum(sudden_front_process_threads)"),
      q("sum(sudden_front_process_open_fds)"),
      q("sum(sudden_front_clients)"),
      q("sum(rate(sudden_front_ws_bytes_out_total[30s]))"),
      q("sum(rate(sudden_front_ws_bytes_in_total[30s]))"),
      q("sum(rate(sudden_front_ws_frames_out_total[30s]))"),
      q("sum(rate(sudden_front_ws_frames_in_total[30s]))"),
      q("sum(sudden_front_ecs_entities)"),
      q("sum(sudden_front_ecs_archetypes)"),
      q("sum(sudden_front_ecs_tables)"),
      q("sum(sudden_front_ecs_components)"),
      q("sum(rate(loadtest_snapshots_total[30s]))"),
      q("sum(rate(loadtest_rtt_seconds_total[1m])) / sum(rate(loadtest_rtt_count_total[1m]))"),
      Promise.resolve(humans),
      q('sum(sudden_front_players_in_match{kind="bot"})'),
    ]);
  const roomsByPhase = {};
  const roomSeries = {};
  for (const phase of ["waiting", "playing", "results"]) {
    roomSeries[phase] = await q(`sum(sudden_front_rooms{phase="${phase}"})`);
    roomsByPhase[phase] = describe(roomSeries[phase]);
  }
  const tickP99 = await q("histogram_quantile(0.99, sum by (le) (rate(sudden_front_tick_duration_seconds_bucket[30s])))");
  const p99Minutes = await series(
    target,
    "histogram_quantile(0.99, sum by (le) (increase(sudden_front_tick_duration_seconds_bucket[1m])))",
    Math.floor(start) + 60,
    Math.ceil(stop),
    60,
  );

  // A server counter's growth between two moments: its value then and now. `increase()` over the
  // window would miss what grew before the window's first sample (the bots' first connections).
  const valueAt = (metric, t) => scalar(target, `sum(last_over_time(${metric}[1m]))`, t);
  const inc = async (metric, from = steadyStart, to = at) => {
    const [before, after] = await Promise.all([valueAt(metric, Math.floor(from) - 1), valueAt(metric, to)]);
    if (after === null) return null;
    // A restart (the counter went back) leaves only what increase() can tell.
    if (before !== null && after < before) return scalar(target, `sum(increase(${metric}[${Math.round(to - from)}s]))`, to);
    return after - (before ?? 0);
  };
  // The bots' counters start at zero with their process: the largest value seen is the total.
  const botTotal = (metric) => scalar(target, `sum(max_over_time(${metric}[${totalW}]))`, atEnd);
  const quantile = (qv) =>
    scalar(target, `histogram_quantile(${qv}, sum by (le) (increase(sudden_front_tick_duration_seconds_bucket[${W}])))`, at);
  const [p50, p95, p99, tickSum, tickCount, ticks, overruns, kicked, connections] = await Promise.all([
    quantile(0.5),
    quantile(0.95),
    quantile(0.99),
    inc("sudden_front_tick_duration_seconds_sum"),
    inc("sudden_front_tick_duration_seconds_count"),
    inc("sudden_front_ticks_total"),
    inc("sudden_front_tick_overruns_total"),
    inc("sudden_front_clients_kicked_total", start, atEnd),
    inc("sudden_front_connections_total", start, atEnd),
  ]);
  const [errors, drops, connectFailures, started, ended, snapshots, kills, rttSum, rttCount, botBytesIn, botBytesOut] = await Promise.all(
    [
      "loadtest_server_errors_total",
      "loadtest_disconnects_total",
      "loadtest_connect_failures_total",
      "loadtest_matches_started_total",
      "loadtest_matches_ended_total",
      "loadtest_snapshots_total",
      "loadtest_kills_total",
      "loadtest_rtt_seconds_total",
      "loadtest_rtt_count_total",
      "loadtest_bytes_in_total",
      "loadtest_bytes_out_total",
    ].map(botTotal),
  );
  // Snapshots per second while (nearly) every bot plays: the count's growth over that window.
  const [snapBefore, snapAfter] = await Promise.all([
    valueAt("loadtest_snapshots_total", Math.floor(steadyStart)),
    valueAt("loadtest_snapshots_total", at),
  ]);
  const steadySnapshots = snapAfter === null ? null : (snapAfter - (snapBefore ?? 0)) / steadyS;
  const up = await vector(target, "up", atEnd);

  const steady = (points) => describe(within(points, steadyStart, steadyEnd));
  const whole = (points) => describe(points);

  // Minute by minute, from the fine series.
  const timeline = [];
  // Minutes that start before the stop signal; the wind-down after it is not a minute of load.
  for (let m = 0; start + m * 60 < stop - 5; m += 1) {
    const from = start + m * 60;
    const to = Math.min(start + (m + 1) * 60, end);
    const pick = (points) => describe(within(points, from, to));
    const p99m = p99Minutes.find(([t]) => t > from && t <= to + 1);
    timeline.push({
      minute: m,
      clients: pick(clients)?.max ?? null,
      inMatch: pick(botsInMatch)?.max ?? null,
      tickMin: pick(rate)?.min ?? null,
      p99: p99m ? p99m[1] : null,
      cpu: pick(cpu)?.avg ?? null,
      rss: pick(rss)?.max ?? null,
      outBytes: pick(outBytes)?.avg ?? null,
      rtt: pick(rttSeries)?.avg ?? null,
    });
  }

  // The fine series for the HTML charts: [seconds since start, value in the chart's unit].
  const rel = (points, scale = 1) => points.map(([t, v]) => [Math.round(t - start), Number((v * scale).toPrecision(5))]);
  const seriesOut = {
    clients: rel(clients),
    botsInMatch: rel(botsInMatch),
    tickRate: rel(rate),
    tickP99Ms: rel(tickP99, 1000),
    cpuPct: rel(cpu, 100),
    rssMb: rel(rss, 1 / 2 ** 20),
    outKBs: rel(outBytes, 1 / 1024),
    inKBs: rel(inBytes, 1 / 1024),
    rttMs: rel(rttSeries, 1000),
    roomsPlaying: rel(roomSeries.playing),
    roomsResults: rel(roomSeries.results),
    roomsWaiting: rel(roomSeries.waiting),
    entities: rel(entities),
  };

  return {
    window: { start, stop, end, steadyStart, steadyEnd, steadyS, step },
    series: seriesOut,
    tick: {
      rate: steady(rate),
      rateWhole: whole(rate),
      p50,
      p95,
      p99,
      mean: tickCount ? tickSum / tickCount : null,
      ticks,
      overruns,
      overrunRatio: ticks ? (overruns ?? 0) / ticks : null,
    },
    population: {
      clients: whole(clients),
      humansInMatch: steady(botsHumans),
      serverBotsInMatch: steady(serverBots),
      rooms: roomsByPhase,
      connections,
      kicked,
    },
    ecs: { entities: steady(entities), archetypes: steady(archetypes), tables: steady(tables), components: steady(components) },
    process: { cpu: steady(cpu), rss: steady(rss), rssWhole: whole(rss), threads: steady(threads), fds: steady(fds) },
    network: { outBytes: steady(outBytes), inBytes: steady(inBytes), outFrames: steady(outFrames), inFrames: steady(inFrames) },
    bots: {
      inMatch: steady(botsInMatch),
      errors,
      drops,
      connectFailures,
      started,
      ended,
      snapshots,
      snapshotsRate: steadySnapshots,
      kills,
      rtt: rttCount ? rttSum / rttCount : null,
      rttMinutes: whole(rttSeries),
      bytesIn: botBytesIn,
      bytesOut: botBytesOut,
    },
    targets: up.map((row) => ({ job: row.metric.job, instance: row.metric.instance, up: row.value === 1 })),
    timeline,
  };
}

function previousRun(dir) {
  if (!fs.existsSync(REPORTS)) return null;
  const runs = fs
    .readdirSync(REPORTS)
    .map((name) => path.join(REPORTS, name, "metrics.json"))
    .filter((file) => fs.existsSync(file) && path.dirname(file) !== dir)
    .map((file) => JSON.parse(fs.readFileSync(file, "utf8")))
    .filter((m) => m.run?.start && m.run.start < JSON.parse(fs.readFileSync(path.join(dir, "run.json"), "utf8")).start)
    .sort((a, b) => b.run.start - a.run.start);
  return runs[0] ?? null;
}

function verdicts(run, m) {
  const count = run.bot.count;
  const errorsTotal = [m.bots.errors, m.bots.drops, m.bots.connectFailures, m.population.kicked]
    .map((v) => v ?? 0)
    .reduce((a, b) => a + b, 0);
  const rssGrowth = m.process.rss && m.process.rss.first ? (m.process.rss.last - m.process.rss.first) / m.process.rss.first : null;
  const rows = [
    ["서버 틱 레이트(평균)", m.tick.rate ? `${fmt.n(m.tick.rate.avg, 2)} Hz (최저 ${fmt.n(m.tick.rate.min, 0)})` : "-", "≥ 29.5 Hz", grade(m.tick.rate?.avg, 29.5, 28, true)],
    ["틱 시간 p99", fmt.ms(m.tick.p99), "< 16.7 ms(예산의 절반)", grade(m.tick.p99, TICK_BUDGET / 2, TICK_BUDGET)],
    ["예산(33 ms) 초과 틱", m.tick.overrunRatio === null ? "-" : `${fmt.n(m.tick.overruns)}회 (${fmt.pct(m.tick.overrunRatio, 3)})`, "< 0.1%", grade(m.tick.overrunRatio, 0.001, 0.01)],
    ["경기에 들어간 봇(최대)", `${fmt.n(m.bots.inMatch?.max)} / ${count}`, `${count}명 모두`, grade(m.bots.inMatch?.max ?? null, count, Math.floor(count * 0.8), true)],
    ["오류·끊김·접속 실패·강제 종료", fmt.n(errorsTotal), "0", grade(errorsTotal, 0, 0)],
    ["Ping 왕복(평균)", fmt.ms(m.bots.rtt), "< 50 ms", grade(m.bots.rtt, 0.05, 0.1)],
    ["메모리 증가(안정 구간)", rssGrowth === null ? "-" : `${fmt.mb(m.process.rss.first)} → ${fmt.mb(m.process.rss.last)} (${rssGrowth >= 0 ? "+" : ""}${fmt.pct(rssGrowth)})`, "< +20%", grade(rssGrowth, 0.2, 0.5)],
  ];
  if (run.alerts?.length) rows.push(["실행 중 경고", `${run.alerts.length}건`, "0건", BAD]);
  const overall = rows.map((row) => row[3]).reduce((worst, v) => (rank[v] > rank[worst] ? v : worst), GOOD);
  return { rows, overall };
}

/** The pieces both reports are built from: the run conditions, the detail tables, the charts. */
function sections(run, m, previous) {
  const w = m.window;
  const d = run.docker;
  const mma = (dsc, f) => (dsc ? [f(dsc.min), f(dsc.avg), f(dsc.max)] : ["-", "-", "-"]);
  const MMA = ["지표", "최소", "평균", "최대"];
  const rooms = m.population.rooms;
  const perClient = (rate) => (rate && m.population.clients?.max ? rate.avg / m.population.clients.max : null);
  const grafana = `${run.targetUrls.grafana}/d/sudden-front/?from=${Math.floor(w.start * 1000)}&to=${Math.ceil(w.end * 1000)}`;

  const conditions = [
    ["봇", `${run.bot.count}명, ${run.bot.minutes}분`],
    ["방 설정", `${run.bot.mode} · ${run.bot.map} · ${run.bot.matchType}, 봇 접속 간격 ${run.bot.rampMs} ms`],
    ["시간", `${stamp(new Date(w.start * 1000))} ~ ${stamp(new Date(w.end * 1000)).slice(11)} (안정 구간 ${fmt.clock(w.steadyStart - w.start)}~${fmt.clock(w.steadyEnd - w.start)}, 수집 간격 ${w.step}초)`],
    ["서버 코드", `\`${run.git.commit}\` ${run.git.subject}${run.git.dirty ? " **(커밋하지 않은 변경 있음)**" : ""}`],
    ["서버 이미지", d?.imageCreated ? `빌드 ${d.imageCreated.slice(0, 19).replace("T", " ")}` : "-"],
    ["Docker", d ? `${d.os}, CPU ${d.cpus}개, 메모리 ${fmt.mb(d.memBytes)}` : "-"],
    ["호스트", `${run.host.cpuModel} × ${run.host.cpus} (봇 클라이언트도 같은 PC)`],
    ["봇 명령", `\`${run.botArgs.join(" ")}\` → 종료 코드 ${run.exitCode}`],
  ];

  const tables = [
    {
      title: "분 단위 추이",
      head: ["시각", "접속", "경기 중 봇", "틱 최저(Hz)", "틱 p99", "CPU", "RSS", "송신", "Ping"],
      rows: m.timeline.map((row) => [
        fmt.clock(row.minute * 60),
        fmt.n(row.clients),
        fmt.n(row.inMatch),
        fmt.n(row.tickMin, 0),
        fmt.ms(row.p99),
        fmt.pct(row.cpu, 0),
        fmt.mb(row.rss),
        fmt.kbs(row.outBytes),
        fmt.ms(row.rtt),
      ]),
    },
    {
      title: "게임 루프",
      head: MMA,
      rows: [
        ["틱 레이트 (Hz)", ...mma(m.tick.rate, (v) => fmt.n(v, 1))],
        ["CPU (1코어 = 100%)", ...mma(m.process.cpu, (v) => fmt.pct(v, 0))],
        ["틱 시간 평균 / p50 / p95 / p99", fmt.ms(m.tick.mean), `${fmt.ms(m.tick.p50)} / ${fmt.ms(m.tick.p95)}`, fmt.ms(m.tick.p99)],
        ["돌린 틱 / 33 ms 초과", fmt.n(m.tick.ticks), "", `${fmt.n(m.tick.overruns)}회`],
      ],
      note: "분위수는 히스토그램 버킷(0.1·0.25·0.5·1·2·4·8·16·33·50·100·250 ms) 사이를 선형 보간한 추정값입니다.",
    },
    {
      title: "인원과 방",
      head: MMA,
      rows: [
        ["열린 WebSocket", ...mma(m.population.clients, (v) => fmt.n(v))],
        ["경기 중 사람(봇 클라이언트)", ...mma(m.population.humansInMatch, (v) => fmt.n(v))],
        ["경기 중 서버 봇", ...mma(m.population.serverBotsInMatch, (v) => fmt.n(v))],
        ["방: 대기실", ...mma(rooms.waiting, (v) => fmt.n(v))],
        ["방: 진행", ...mma(rooms.playing, (v) => fmt.n(v))],
        ["방: 결과", ...mma(rooms.results, (v) => fmt.n(v))],
      ],
      note: `연 연결 ${fmt.n(m.population.connections)}개, 송신 큐가 차서 끊은 연결 ${fmt.n(m.population.kicked)}개.`,
    },
    {
      title: "ECS (진행 중인 모든 경기 World 합계)",
      head: MMA,
      rows: [
        ["엔티티", ...mma(m.ecs.entities, (v) => fmt.n(v))],
        ["아키타입", ...mma(m.ecs.archetypes, (v) => fmt.n(v))],
        ["테이블", ...mma(m.ecs.tables, (v) => fmt.n(v))],
        ["컴포넌트 종류", ...mma(m.ecs.components, (v) => fmt.n(v))],
      ],
    },
    {
      title: "프로세스와 네트워크",
      head: MMA,
      rows: [
        ["메모리 RSS", ...mma(m.process.rss, fmt.mb)],
        ["스레드", ...mma(m.process.threads, (v) => fmt.n(v))],
        ["열린 파일", ...mma(m.process.fds, (v) => fmt.n(v))],
        ["송신", ...mma(m.network.outBytes, fmt.kbs)],
        ["수신", ...mma(m.network.inBytes, fmt.kbs)],
        ["송신 프레임/초", ...mma(m.network.outFrames, (v) => fmt.n(v))],
        ["수신 프레임/초", ...mma(m.network.inFrames, (v) => fmt.n(v))],
      ],
      note: `접속당 평균 송신 ${fmt.kbs(perClient(m.network.outBytes))}, 수신 ${fmt.kbs(perClient(m.network.inBytes))}. 실행 전체 메모리는 ${fmt.mb(m.process.rssWhole?.first)}에서 시작해 최대 ${fmt.mb(m.process.rssWhole?.max)}.`,
    },
    {
      title: "봇 클라이언트",
      head: ["지표", "값"],
      rows: [
        ["경기 중 봇 (최소 / 평균 / 최대)", m.bots.inMatch ? `${fmt.n(m.bots.inMatch.min)} / ${fmt.n(m.bots.inMatch.avg, 1)} / ${fmt.n(m.bots.inMatch.max)}` : "-"],
        ["경기 시작 / 종료 (봇 기준 누적)", `${fmt.n(m.bots.started)} / ${fmt.n(m.bots.ended)}`],
        ["받은 스냅샷", `${fmt.n(m.bots.snapshots)} (안정 구간 평균 ${fmt.n(m.bots.snapshotsRate)}/초, 정상은 경기 중 봇 × 30)`],
        ["킬", fmt.n(m.bots.kills)],
        ["Ping 왕복 평균 (분 단위 최대)", `${fmt.ms(m.bots.rtt)} (${fmt.ms(m.bots.rttMinutes?.max)})`],
        ["서버 오류 / 서버가 끊음 / 접속 실패", `${fmt.n(m.bots.errors)} / ${fmt.n(m.bots.drops)} / ${fmt.n(m.bots.connectFailures)}`],
        ["보낸 / 받은 바이트", `${fmt.mb(m.bots.bytesOut)} / ${fmt.mb(m.bots.bytesIn)}`],
      ],
    },
  ];

  let comparison = null;
  if (previous) {
    const pm = previous.metrics;
    const pr = previous.run;
    const delta = (label, now, before, f, lowerIsBetter = true) => {
      if (now === null || now === undefined || before === null || before === undefined) return [label, f(before), f(now), "-"];
      const change = before === 0 ? null : (now - before) / before;
      if (change !== null && Math.abs(change) < 0.01) return [label, f(before), f(now), "변화 없음"];
      const word = change === null || Math.abs(change) < 0.01 ? "" : (change < 0) === lowerIsBetter ? " 개선" : " 악화";
      return [label, f(before), f(now), change === null ? "-" : `${change >= 0 ? "▲ +" : "▼ "}${fmt.pct(change)}${word}`];
    };
    comparison = {
      title: "이전 실행과 비교",
      note: `이전: ${stamp(new Date(pr.start * 1000))}, 봇 ${pr.bot.count}명 · ${pr.bot.minutes}분 · ${pr.bot.mode}/${pr.bot.map}, 코드 \`${pr.git.commit}\`${pr.bot.count !== run.bot.count ? " — **봇 수가 달라 직접 비교에 주의**" : ""}`,
      head: ["지표", "이전", "이번", "변화"],
      rows: [
        delta("틱 레이트 평균", m.tick.rate?.avg, pm.tick.rate?.avg, (v) => fmt.n(v, 2), false),
        delta("틱 시간 평균", m.tick.mean, pm.tick.mean, fmt.ms),
        delta("틱 시간 p99", m.tick.p99, pm.tick.p99, fmt.ms),
        delta("CPU 평균", m.process.cpu?.avg, pm.process.cpu?.avg, (v) => fmt.pct(v, 0)),
        delta("RSS 최대", m.process.rss?.max, pm.process.rss?.max, fmt.mb),
        delta("송신 평균", m.network.outBytes?.avg, pm.network.outBytes?.avg, fmt.kbs),
        delta("Ping 평균", m.bots.rtt, pm.bots.rtt, fmt.ms),
      ],
    };
  }

  const down = m.targets.filter((t) => !t.up).map((t) => `${t.job}(${t.instance})`);
  const links = [
    `Grafana(이 실행 구간): ${grafana}`,
    `Prometheus: ${run.targetUrls.prom}`,
    `끝날 때 수집 대상: ${m.targets.length - down.length}/${m.targets.length} 정상${down.length ? `, 응답 없음: ${down.join(", ")}` : ""}`,
    ...(run.alerts ?? []).map((a) => `실행 중 경고: ${a}`),
    "같은 폴더: `report.md`(요약과 분석), `metrics.json`(숫자와 시계열), `run.json`(실행 조건), `bots.log`(봇 콘솔)",
  ];

  // Charts: one measure per chart (no second axis); a chart has one unit.
  const s = m.series;
  const steadyOf = (points) => describe(points.filter(([t]) => t >= w.steadyStart - w.start && t <= w.steadyEnd - w.start));
  const note = (points, f, what = ["평균", "최대"]) => {
    const dsc = steadyOf(points ?? []);
    if (!dsc) return "";
    const pick = { 최소: dsc.min, 평균: dsc.avg, 최대: dsc.max, 처음: dsc.first, 마지막: dsc.last };
    return `안정 구간 ${what.map((k) => `${k} ${f(pick[k])}`).join(" · ")}`;
  };
  const n1 = (v) => fmt.n(v, 1);
  const n0 = (v) => fmt.n(v, 0);
  const charts = s
    ? [
        { id: "players", title: "접속과 경기 중 봇", unit: "명", digits: 0, note: note(s.botsInMatch, n0, ["최소", "평균"]).replace("안정 구간", "경기 중 봇"), series: [{ name: "접속", slot: 1, points: s.clients }, { name: "경기 중 봇", slot: 2, points: s.botsInMatch }] },
        { id: "rate", title: "틱 레이트", unit: "Hz", digits: 1, ref: { y: 30, label: "목표 30 Hz" }, note: note(s.tickRate, n1, ["평균", "최소"]), series: [{ name: "틱 레이트", slot: 1, points: s.tickRate }] },
        { id: "p99", title: "틱 시간 p99 (30초 창)", unit: "ms", digits: 2, note: `${note(s.tickP99Ms, (v) => fmt.n(v, 2))} · 예산 33.3 ms`, series: [{ name: "p99", slot: 1, points: s.tickP99Ms }] },
        { id: "cpu", title: "서버 CPU", unit: "% (1코어 = 100%)", digits: 0, note: note(s.cpuPct, n0), series: [{ name: "CPU", slot: 1, points: s.cpuPct }] },
        { id: "rss", title: "메모리 RSS", unit: "MB", digits: 1, note: note(s.rssMb, n1, ["처음", "마지막"]), series: [{ name: "RSS", slot: 1, points: s.rssMb }] },
        { id: "net", title: "네트워크", unit: "KB/s", digits: 0, note: `${note(s.outKBs, n0, ["평균"]).replace("평균", "송신 평균")} · 수신 평균 ${fmt.n(steadyOf(s.inKBs ?? [])?.avg, 0)}`, series: [{ name: "송신", slot: 1, points: s.outKBs }, { name: "수신", slot: 2, points: s.inKBs }] },
        { id: "rtt", title: "Ping 왕복 (봇 측정, 1분 창)", unit: "ms", digits: 1, note: note(s.rttMs, n1), series: [{ name: "Ping", slot: 1, points: s.rttMs }] },
        { id: "rooms", title: "방 상태", unit: "개", digits: 0, note: note(s.roomsPlaying, n0, ["평균"]).replace("평균", "진행 평균"), series: [{ name: "진행", slot: 1, points: s.roomsPlaying }, { name: "결과", slot: 2, points: s.roomsResults }, { name: "대기실", slot: 3, points: s.roomsWaiting }] },
        { id: "ecs", title: "ECS 엔티티 (모든 경기 합계)", unit: "개", digits: 0, note: note(s.entities, n0, ["평균", "최대"]), series: [{ name: "엔티티", slot: 1, points: s.entities }] },
      ]
    : [];

  const tickBars = [
    { label: "평균", ms: m.tick.mean === null ? null : m.tick.mean * 1000 },
    { label: "p50", ms: m.tick.p50 === null ? null : m.tick.p50 * 1000 },
    { label: "p95", ms: m.tick.p95 === null ? null : m.tick.p95 * 1000 },
    { label: "p99", ms: m.tick.p99 === null ? null : m.tick.p99 * 1000 },
  ];
  return { conditions, tables, comparison, links, charts, tickBars, grafana };
}

const ANALYSIS_RE = /<!-- analysis:start -->[\s\S]*?<!-- analysis:end -->/;
const ANALYSIS_PLACEHOLDER = "<!-- analysis:start -->\n## 분석\n\n_(에이전트가 작성: 4~6줄 불릿, 숫자 먼저)_\n<!-- analysis:end -->";

/** report.md: the verdict table and the analysis only; the charts and details are in report.html. */
function markdown(run, m, verdict, parts, analysis) {
  const w = m.window;
  const table = (head, body) => [`| ${head.join(" | ")} |`, `| ${head.map(() => "---").join(" | ")} |`, ...body.map((r) => `| ${r.join(" | ")} |`)].join("\n");
  return [
    `# 부하 테스트 리포트: 봇 ${run.bot.count}명 · ${run.bot.minutes}분`,
    "",
    `**종합 판정: ${verdict.overall}** · ${stamp(new Date(w.start * 1000))} · ${run.bot.mode} · ${run.bot.map} · ${run.bot.matchType} · 코드 \`${run.git.commit}\``,
    "",
    "📊 차트·상세 표: [report.html](report.html) (브라우저로 열기)",
    "",
    table(["항목", "값", "기준", "판정"], verdict.rows),
    "",
    analysis,
    "",
    `Grafana: ${parts.grafana}`,
    "",
  ].join("\n");
}

/** Writes report.md and report.html from metrics.json, keeping an analysis already in report.md. */
function render(dir) {
  const { run: runOnly, metrics: m } = JSON.parse(fs.readFileSync(path.join(dir, "metrics.json"), "utf8"));
  const run = { ...runOnly, dir };
  const verdict = verdicts(run, m);
  const parts = sections(run, m, previousRun(dir));
  const reportFile = path.join(dir, "report.md");
  let analysis = ANALYSIS_PLACEHOLDER;
  if (fs.existsSync(reportFile)) {
    const old = fs.readFileSync(reportFile, "utf8").match(ANALYSIS_RE);
    if (old && !old[0].includes("_(에이전트가 작성")) analysis = old[0];
  }
  fs.writeFileSync(reportFile, markdown(run, m, verdict, parts, analysis));
  const htmlFile = path.join(dir, "report.html");
  fs.writeFileSync(
    htmlFile,
    htmlReport({ run, m, verdict, budgetMs: TICK_BUDGET * 1000, analysisMd: analysis, ...parts }),
  );
  return { reportFile, htmlFile };
}

async function writeReport(target, run) {
  const metrics = await collect(target, run);
  const { overall } = verdicts(run, metrics);
  const { dir, ...runOnly } = run;
  fs.writeFileSync(path.join(dir, "metrics.json"), `${JSON.stringify({ run: runOnly, overall, metrics }, null, 2)}\n`);
  return render(dir);
}

// ---- main

const HELP = `usage: node loadtest.mjs <command> [options]
  up      [--no-build]                     Docker stack up (game, Prometheus, Grafana), wait until scraped
  run     --count <bots> --minutes <min>   run the bots, then write tests/reports/<run>/report.md
          [--mode normal|augment|big-head] [--map supply-yard|warehouse] [--match-type team|ffa]
          [--ramp-ms 50] [--out <dir>]
  report  --dir <run dir>                  query Prometheus again and rewrite the reports of a past run
  render  --dir <run dir>                  rewrite report.md/report.html from metrics.json (after the analysis)
  status                                   what the server does now
  down                                     Docker stack down
every command: [--target local] [--server-port 8800] [--prometheus URL] [--grafana URL]`;

const { command, opts } = parseArgs(process.argv.slice(2));
const commands = { up: commandUp, run: commandRun, report: commandReport, render: commandRender, status: commandStatus, down: commandDown };
if (!commands[command]) {
  console.log(HELP);
  process.exitCode = command === "help" ? 0 : 1;
} else {
  try {
    await commands[command](opts);
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exitCode = 1;
  }
}
