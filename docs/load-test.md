# 부하 테스트와 모니터링

봇 클라이언트(`tests/client/`)가 브라우저와 같은 WebSocket 프로토콜로 접속해 실제 플레이어처럼 방을 만들고 경기합니다. 게임 서버는 `/metrics`로 Prometheus 지표를 내보내고, `infra/`의 Docker Compose가 게임 서버·Prometheus·Grafana·cAdvisor를 함께 띄웁니다. Google Cloud 배포용 Pulumi 프로젝트는 `infra/pulumi/`에 있습니다.

## 봇 클라이언트 (`tests/client/`)

서버와 따로 빌드하는 Rust 프로젝트입니다. proto 코드는 `build.rs`가 `proto/game.proto`에서 만듭니다(서버와 같은 내장 protoc).

```powershell
cd tests\client
cargo run --release -- --count 50                       # Ctrl+C까지 계속 접속
cargo run --release -- --count 50 --duration 300        # 300초 뒤 모두 나가고 종료
```

| 인자 | 기본값 | 설명 |
| --- | --- | --- |
| `--count` | (필수) | 접속할 봇 수 |
| `--duration` | 없음 | 실행 시간(초). 없으면 Ctrl+C까지 접속을 유지 |
| `--url` | `ws://localhost:<PORT 또는 8800>/ws` | 서버 WebSocket 주소 |
| `--mode` | `normal` | 방 모드: `normal`·`augment`·`big-head` |
| `--map` | `supply-yard` | 맵: `supply-yard`·`warehouse` |
| `--match-type` | `team` | `team`(팀 데스매치)·`ffa`(개인전) |
| `--ramp-ms` | 50 | 봇 사이 접속 간격(ms) |
| `--metrics-port` | 9900 | 봇 지표 `/metrics` 포트, 0이면 끔 |

10초마다 접속·방·경기 수, 받은 스냅샷, 킬, Ping 왕복 시간, 서버 오류, 끊김을 한 줄로 출력합니다. 끝날 때(시간 종료 또는 Ctrl+C) 모든 봇이 `LeaveRoom`을 보내고 연결을 닫습니다.

### 행동 트리

봇마다 [bonsai-bt](https://crates.io/crates/bonsai-bt) 행동 트리(`src/tree.rs`)를 서버 틱과 같은 30Hz로 돌립니다. 잎 노드(`src/agent.rs`의 `Act`)는 네트워크를 직접 다루지 않고, 받은 메시지로 갱신한 칠판(`Agent`)을 읽어 보낼 메시지를 `outbox`에 넣습니다. 연결 루프(`src/bot.rs`)가 매 틱 그것을 보냅니다.

1. **Hello**: 닉네임 `bot_<0~99999 난수>`로 접속, `Welcome`을 받으면 성공
2. **방 만들기/찾기**: 시작 순서대로 10명씩 한 그룹(`src/plan.rs`)이고, 그룹의 첫 봇이 리더입니다. 리더는 `loadtest-<실행 ID>-<그룹>` 이름으로 방을 만들고, 나머지는 방 목록에서 그 이름을 찾아 들어갑니다. 혼자인 리더(봇 1명 또는 마지막 그룹이 1명)는 "빈 자리는 봇으로 채우기"로 방을 만들어 서버 봇과 싸웁니다.
3. **준비와 시작**: 리더가 아닌 봇은 주무기를 무작위로 고르고(`SelectWeapon`) 준비합니다. 리더는 그룹 전원이 들어와 준비하면 시작하고, 30초가 지나면 들어온 인원으로 시작합니다.
4. **전투**: `MatchEnd`까지 싸웁니다. 증강모드면 첫 카드를 고릅니다.
5. **결과 대기**: 방이 대기실로 돌아오면 3으로 돌아가 다음 경기를 합니다.
6. **종료**: 실행이 끝나면 어느 단계에 있든 방을 나가고 연결을 닫습니다.

전투 로직(`src/combat/`)은 서버 봇(`server/src/game/systems/bot.rs`)의 행동을 클라이언트 쪽에 따로 옮긴 것입니다. 시야 46m·반응 0.45~0.95초·초기 조준 오차와 수렴·좌우 이동하며 점사·순찰 경로(1m 격자 BFS)·끼면 경로 재탐색이 같습니다. 서버 봇과 달리 스냅샷에 보이는 정보만 쓰고, 예비 탄약이 사람과 같으며(탄이 다 떨어지면 2번 권총, 3번 나이프로 바꿈), 수류탄은 던지지 않습니다. 순찰 목표는 서버의 차선 대신 격자의 무작위 빈칸입니다. 결정 하나를 `InputCmd` 두 개(seq 연속)로 보내 브라우저와 같은 초당 60명령을 채웁니다. 서버 봇의 행동을 바꿔도 이쪽은 자동으로 따라가지 않습니다.

### 검사

```powershell
cd tests\client; cargo fmt; cargo clippy --all-targets -- -D warnings; cargo test --release
```

서버와 같은 lint(`unsafe` 금지, `clippy::arithmetic_side_effects`)를 씁니다. 테스트는 각 파일 맨 아래에 있고, 행동 트리 전체를 서버 메시지 각본으로 돌리는 테스트가 `tree.rs`에 있습니다.

## 서버 지표 (`/metrics`)

`server/src/metrics.rs`가 `sudden_front_` 접두사로 내보냅니다. 허브 스레드가 틱마다 시간을 재고, 1초마다 틱 수와 허브 상태를 모읍니다.

| 지표 | 종류 | 내용 |
| --- | --- | --- |
| `sudden_front_tick_rate_hertz` | gauge | 최근 1초의 틱 수(서버 FPS). 따라가면 30 |
| `sudden_front_tick_duration_seconds` | histogram | 틱 하나(모든 방의 `Match` 진행과 스냅샷 인코딩)에 걸린 시간 |
| `sudden_front_tick_overruns_total` | counter | 33ms 예산을 넘긴 틱 |
| `sudden_front_ticks_total` | counter | 돌린 틱 |
| `sudden_front_clients` | gauge | 열린 WebSocket |
| `sudden_front_rooms{phase}` | gauge | `waiting`·`playing`·`results` 방 수 |
| `sudden_front_players_in_match{kind}` | gauge | 진행 중인 경기의 `human`·`bot` 수 |
| `sudden_front_ecs_entities`·`_archetypes`·`_tables`·`_components` | gauge | 진행 중인 모든 `Match` World의 엔티티·아키타입·테이블·컴포넌트 종류 합계(`Match::ecs_stats`) |
| `sudden_front_ws_frames_in_total`·`_out_total`, `sudden_front_ws_bytes_in_total`·`_out_total` | counter | WebSocket 프레임 수와 바이트 |
| `sudden_front_connections_total`, `sudden_front_clients_kicked_total` | counter | 연 연결, 송신 큐가 차서 끊은 연결 |
| `sudden_front_process_*` | 여러 개 | CPU 시간·RSS·스레드·열린 파일. Linux(Docker 이미지)에서만 나옴 |

봇 클라이언트는 `--metrics-port`(기본 9900)에 `loadtest_` 접두사 지표(봇 수, 스냅샷·킬·오류·끊김, 보낸·받은 프레임과 바이트, Ping 왕복 합계)를 냅니다.

## Docker Compose (`infra/`)

```powershell
docker compose -f infra/docker-compose.yml up -d --build
cd tests\client; cargo run --release -- --count 50 --duration 600
docker compose -f infra/docker-compose.yml down
```

| 서비스 | 주소 | 내용 |
| --- | --- | --- |
| `server` | http://localhost:8800 | `infra/docker/server.Dockerfile`: 클라이언트 빌드(`npm ci`·`npm run build`) → 서버 release 빌드 → `debian:bookworm-slim`에 바이너리와 `client/dist` |
| `prometheus` | http://localhost:9090 | 5초마다 수집. `server`(compose DNS로 모든 복제본), 호스트의 봇(`host.docker.internal:9900`), cAdvisor |
| `grafana` | http://localhost:3000 | 익명 보기 허용, 관리자 `admin`/`admin`(`GRAFANA_USER`·`GRAFANA_PASSWORD`로 변경). "Sudden Front server" 대시보드가 홈 |
| `cadvisor` | (내부) | 컨테이너별 CPU·메모리·네트워크. Docker Desktop(containerd 이미지 저장소)에서는 컨테이너를 알아보지 못해 비어 있음([history.md](history.md) 알려진 이슈) |

- 포트는 `SERVER_PORT`·`PROMETHEUS_PORT`·`GRAFANA_PORT`로 바꿉니다. 서버 여러 대는 `--scale server=N`이지만 호스트 포트 8800이 겹치므로 `SERVER_PORT`를 비우거나 포트 범위로 바꿔야 합니다.
- 대시보드 JSON(`infra/grafana/dashboards/sudden-front.json`)은 Grafana에서 고친 뒤 JSON으로 내보내 이 파일에 덮어씁니다. 행: 개요, 게임 루프(FPS·틱 시간 p50/p95/p99·초과), 인원, ECS, 프로세스·네트워크, 컨테이너(cAdvisor), 부하 테스트 봇.
- `.dockerignore`가 `node_modules`·`target`·`tests`·`infra/pulumi` 등을 빌드 컨텍스트에서 뺍니다.

## Google Cloud (`infra/pulumi/`)

TypeScript Pulumi 프로젝트입니다. Artifact Registry 저장소, 이미지 읽기 권한을 가진 서비스 계정, 고정 IP, 방화벽(게임 8800은 전체, Grafana 3000·Prometheus 9090은 `adminCidr`만), Debian 12 VM 한 대를 만듭니다. VM 시작 스크립트가 Docker를 설치하고, 메타데이터로 받은 `infra/`의 compose·Prometheus·Grafana 파일을 쓴 뒤, 게임 서버 이미지를 Artifact Registry에서 받아 `docker compose up -d --no-build`로 띄웁니다. 방을 한 프로세스 메모리에 두고 WebSocket을 오래 유지하는 서버라 자동으로 인스턴스를 늘리는 서비스 대신 VM을 씁니다.

```powershell
cd infra\pulumi; npm ci
pulumi stack init dev
pulumi config set gcp:project <프로젝트 ID>
pulumi config set adminCidr <내 IP>/32
pulumi config set --secret grafanaPassword <비밀번호>
pulumi config set imageTag v1
pulumi up                       # 저장소 주소는 출력 imageRepository
docker build -f infra/docker/server.Dockerfile -t <imageRepository>/server:v1 .
docker push <imageRepository>/server:v1
```

- 지역 기본값은 `asia-northeast3`(서울)·`asia-northeast3-a`, 기계는 `machineType`(기본 `e2-standard-2`)입니다.
- 처음 `pulumi up`은 이미지를 올리기 전이라 VM이 이미지를 받지 못합니다. 이미지를 올린 뒤 VM을 다시 시작하면(시작 스크립트는 부팅마다 돔) 뜹니다.
- `npm run check`(`tsc --noEmit`)로 타입만 검사합니다. 실제 배포(`pulumi up`)는 아직 하지 않았습니다.
- Grafana 비밀번호는 VM 메타데이터로 전달되어 프로젝트의 메타데이터 읽기 권한이 있으면 보입니다.
