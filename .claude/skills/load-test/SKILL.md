---
name: load-test
description: Run a Sudden Front load test — M bots for N minutes against the game server in Docker, watched by Prometheus/Grafana — and write a Korean report with charts (report.html) and a short summary (report.md) (tick rate, tick time p99, CPU, memory, network, ECS size, bot errors, minute-by-minute trend, comparison with the last run) with an analysis for developers. Use when the user asks for a load test, stress test, 부하 테스트, or how the server holds up with many players. Target is localhost only for now (remote is not deployed).
argument-hint: "<minutes> <bots> [mode] [map] [team|ffa]  e.g. 5분 50명"
---

# Load test

Runs the `tests/client` bots (10 to a room, they play match after match) against the game server of
`infra/docker-compose.yml` for a fixed time, then turns what Prometheus collected into
`tests/reports/<time>-<M>bots-<N>m/report.md`, and you add the analysis. Talk to the user in Korean;
the report is Korean.

Everything goes through one script (Node 22, no dependencies), run from the project root:

```
node .claude/skills/load-test/scripts/loadtest.mjs <up|run|report|status|down> [options]
```

## 1. Read the request

- **Minutes (N)** and **bots (M)** are required. If either is missing, ask once (AskUserQuestion).
  Suggest 5분 · 50명 as the default.
- Optional: mode `normal|augment|big-head`, map `supply-yard|warehouse`, match type `team|ffa`,
  bot ramp `--ramp-ms` (default 50 ms between connections). Defaults: normal, supply-yard, team.
- **Target**: `local` (default). If the user asks for a remote server, say in one line that remote
  is not supported yet because the Pulumi VM (`infra/pulumi`) is not deployed, and offer local.
- Sanity limits: one PC runs both the server and the bots, so above ~300 bots the bot client
  itself may become the bottleneck — say so in the analysis when the host CPU is busy.

## 2. Bring the stack up

```
node .claude/skills/load-test/scripts/loadtest.mjs up
```

- Starts Docker Desktop if it is not running (waits up to 3 min), then
  `docker compose up -d --build`, so the server image is rebuilt from the current code (the first
  build takes several minutes; later ones use the cache). Give it a timeout of 600000 ms.
- Use `--no-build` only when the user says the code has not changed since the last run.
- If it fails on port 8800, a local `cargo run` server is probably running: find it
  (`Get-NetTCPConnection -LocalPort 8800`) and ask before stopping it.
- It returns once Prometheus scrapes the server. Grafana: http://localhost:3000 (dashboard
  "Sudden Front server"), Prometheus: http://localhost:9090.

## 3. Run the test

Run it **in the background** (`run_in_background: true`) — it lasts N minutes plus about a minute:

```
node .claude/skills/load-test/scripts/loadtest.mjs run --count <M> --minutes <N> [--mode ..] [--map ..] [--match-type ..]
```

It builds the bots (`cargo build --release` in `tests/client`), starts them with
`--duration N*60`, prints a status line every 30 s (clients, bots in match, tick Hz, tick p99, CPU,
memory, bot errors) next to the bots' own 10 s summary, waits for the last scrape, and writes into
the run folder (the last two lines name the reports):

- `report.html` — the visual report, self-contained (opens from disk, no network): verdict tiles,
  the analysis, tick time against the 33 ms budget, time-series charts with hover (players, tick
  rate, tick p99, CPU, RSS, network, Ping, room phases, ECS entities; the ramp-up and wind-down
  are shaded), comparison with the last run, run conditions, detail tables folded in `<details>`.
  Drawn by `scripts/report-html.mjs`.
- `report.md` — short: verdict table + analysis + link to `report.html`. The analysis lives here.
- `metrics.json` (numbers and the fine series), `run.json`, `bots.log`.

`render --dir <run folder>` rebuilds both reports from `metrics.json` without Prometheus;
`report --dir` queries Prometheus again (needed for runs whose `metrics.json` has no `series`).

While it runs:
- Tell the user the run started, how long it takes, and the Grafana link; then wait for the
  completion notification. Don't poll in a loop.
- If the user asks how it goes, read the tail of the background output, or run `status`.
- If it must stop early, stop the background task; if `loadtest.exe` is still alive, end it
  (`Stop-Process -Name loadtest`). A stopped run has no report; start a new run.

## 4. Write the analysis

When the run finishes, read `report.md` and `metrics.json` (the `timeline` and `series` hold the
trends; `bots.log` if the bots reported errors). Replace the placeholder between
`<!-- analysis:start -->` and `<!-- analysis:end -->` with a `## 분석` section in Korean, keeping
both markers (a later `report`/`render` keeps what is between them). Then run
`render --dir <run folder>` so `report.html` shows the analysis too.

The charts and tables already show the numbers, so the analysis is not prose: **4–6 bullets, each
one line**, a bold 2–4 word label then the numbers and the conclusion (`**선형 증가**: 50→500명에서
틱 평균 0.24→2.34 ms, CPU 7→71%`). No sentences restating a table. Cover what a developer can act on:

- The verdict in one sentence: did the server keep 30 Hz with M bots, and how much headroom is left
  (tick p99 against the 33 ms budget, CPU against the Docker CPU count in 실행 조건).
- What grew with load and what did not: tick time and CPU per bot, bytes per client, ECS entities
  per match. Point at a trend in 분 단위 추이 (e.g. RSS rising every minute = possible leak; tick
  spikes at match starts).
- Anything wrong: errors, drops, kicked connections, bots that never got into a match, a minute
  with a low tick rate, Prometheus targets down. Quote the numbers.
- Against 이전 실행과 비교, when present: what changed and whether the code commit differs (a
  different bot count makes the comparison weak — say so).
- What to try next (more bots, another mode/map, profiling a system), only if the data suggests it.

Don't invent causes the data doesn't show; say "확인 필요" instead. Mention when the bots ran on the
same PC and the host CPU may have limited them.

## 5. Report back

Answer the user in Korean with: the overall verdict, the 3–5 numbers that matter (tick Hz avg/min,
tick p99, CPU, RSS, errors), the analysis in short, the full paths of `report.html` (the one to
open) and `report.md`, plus the Grafana link of the run (in 참고). Leave the Docker stack running so they can look at Grafana, and
tell them `node .claude/skills/load-test/scripts/loadtest.mjs down` stops it (the Prometheus data
volume stays, so later reports can still be rebuilt with `report --dir <run folder>`).

`tests/reports/` is git-ignored: don't commit reports unless the user asks.
