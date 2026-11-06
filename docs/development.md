# 개발 가이드

## 준비물

- Rust 1.89 이상(`bevy_ecs` 0.18 요구), Node.js 22.12 이상(`@bufbuild/protoc-gen-es`, Vite 8 요구)
- Microsoft Edge (e2e 테스트용, playwright-core가 `msedge` 채널 사용)

## 빌드와 실행

```powershell
cd client; npm ci; npm run build           # proto 생성 → 에셋 검사 → 타입 검사 → client/dist
cd ..\server; cargo run --release          # http://localhost:8800
```

- macOS·Linux에서는 프로젝트 폴더의 `./run.sh` 하나로 클라이언트 빌드(패키지가 없거나 `package-lock.json`이 바뀌었을 때만 `npm ci`) → 서버 빌드(`cargo build --release`) → 서버 시작까지 합니다. `PORT=8810 ./run.sh`처럼 포트를 줄 수 있고, 그 포트를 이미 다른 프로세스가 쓰고 있으면 알려 주며, 같은 네트워크에서 접속할 주소(`http://<IP>:<포트>`)도 출력합니다.
- 서버가 `client/dist`를 정적 파일로 제공합니다. 경로는 `CLIENT_DIST` 환경 변수로 바꿉니다.
- 포트:
  - `PORT`가 없으면 8800을 쓰고, 사용 중이면 8801~8819 중 첫 빈 포트, 그것도 없으면 OS가 고른 포트를 씁니다. 옮겼을 때는 준비 줄 앞에 `Port 8800 is in use; using 8801. …`을 출력합니다.
  - `PORT`를 지정하면 그 포트만 씁니다. 사용 중이거나 숫자가 아니면 한 줄 오류를 출력하고 종료 코드 1로 끝납니다.
  - 사용 여부는 bind 전에 127.0.0.1·::1로 접속해 먼저 확인합니다. macOS에서는 다른 프로세스가 loopback에만 연 포트에도 0.0.0.0 bind가 성공해서, `localhost`가 그 프로세스로 연결되기 때문입니다.
- 같은 네트워크의 다른 PC는 `http://<서버 IP>:8800`으로 접속합니다.
- 클라이언트 개발: 서버를 켠 채 `cd client; npm run dev` → http://localhost:5173. Vite는 `/ws`를 `PORT`(없으면 8800)로 프록시하므로, 서버가 다른 포트로 열렸으면 같은 `PORT`를 주고 실행합니다.
- `http://localhost:8800/?debug`로 열면 `window.__game`이 노출됩니다.

## 에셋

- 런타임 에셋(glTF·WebP)은 `client/public/art/`에 두고 `client/src/game/art/manifest.json`에 등록합니다. 출처·라이선스는 `client/public/art/LICENSES.json`에 파일별로 기록합니다(AGENTS.md 규칙 7).
- `npm run build`의 `scripts/check-art.mjs`가 빌드를 실패시키는 경우:
  - 등록되지 않은 파일이 있거나, 등록됐는데 파일이 없을 때
  - 라이선스가 허용되지 않을 때(CC0, 또는 표기 문구가 있는 CC-BY만 허용)
  - manifest가 가리키는 파일이 없을 때
  - 예산(`manifest.json`의 `budgets`)을 넘을 때
  - 무기를 하나라도 등록했는데 `WeaponKind` 전체를 채우지 않았을 때
  - 파일 내용이 `generated.json`의 sha256·크기와 다를 때(내용 잠금), GLB에 삼각형 수 기록이 없을 때, manifest가 이름을 대지 않는 파일이 있을 때 (`.DS_Store`는 무시)
- `http://localhost:8800/?art`는 서버에 접속하지 않고 에셋을 불러와 manifest 요약·로드 실패·예산을 보여 줍니다.
- 로더는 실패한 에셋마다 `[art]` 경고 한 줄을 남기고 대체 모형을 씁니다. `console.error`는 남기지 않습니다(`play.mjs`가 콘솔 에러로 실패하기 때문).

### 에셋 파이프라인 (`tools/art/`)

```powershell
cd tools/art; npm ci
node fetch.mjs                 # sources.json의 팩을 art-src/로 받고 SHA-256 확인 (art-src/는 gitignore)
node build-art.mjs             # recipes/로 client/public/art/, generated.json, LICENSES.json, docs/credits.md 생성
node build-art.mjs --check     # 다시 만들어도 바이트가 같은지 확인
node inspect.mjs <파일.glb>    # 노드·뼈·클립·삼각형·재질·크기 측정 (--json, --summary)
```

- 에셋을 바꿀 때는 런타임 파일이 아니라 레시피(`tools/art/recipes/`)를 고칩니다. `build-art.mjs`가 manifest와 만든 결과가 서로 맞는지도 검사합니다.
- 후보 비교: `node stage-candidates.mjs`로 후보를 `client/public/art/_candidates/`(gitignore, 빌드에서 제외)에 모으면, 개발 서버의 `?art&candidates`가 비교 갤러리를 보여 줍니다.
- 스크린샷: `node gallery-shots.mjs`는 후보 갤러리를 `client/e2e-shots/art-gallery/`에, `--final`은 등록된 에셋을 `client/e2e-shots/art-final/`에 저장합니다.
- 출처·라이선스 목록은 [credits.md](credits.md)(자동 생성)에 있습니다.

## 성능 측정

- `?debug&perf`로 열면 화면 왼쪽 위에 draw call·삼각형·셰이더 프로그램·프레임 시간 오버레이가 뜹니다. 같은 값을 `window.__game.perf()`로 읽을 수 있습니다.
- `node e2e/perf.mjs`는 두 맵에서 봇 경기를 열어 20초 동안 측정하고 `e2e-shots/perf.json`에 저장합니다.
- 기준선(그래픽 개선 전): 2026-11-04, headless Chrome + SwiftShader, 1280×720, 이 Mac(16코어). e2e 스크립트의 기본 브라우저는 Edge이고, 이 측정은 `channel`만 `"chrome"`으로 바꾼 사본으로 했습니다. 프레임 시간은 실행마다 ±30% 흔들립니다.

| 맵 | draw call 평균(최대) | 삼각형 평균 | 셰이더 프로그램 | 평균 / p95 프레임(ms) | 첫 프레임(ms) |
| --- | --- | --- | --- | --- | --- |
| 보급창고 | 190–204 (224) | 약 4,000 | 10 | 38–60 / 88–127 | 2,900–3,150 |
| 웨어하우스 | 180–193 (224) | 약 3,400 | 8–9 | 47–52 / 124–153 | 2,380–2,450 |

- 3단계(glTF 캐릭터) 뒤: 그리기 호출이 보급창고 103(최대 110), 웨어하우스 112(최대 128)로 줄었습니다. 삼각형은 약 5.4만 개, 셰이더 프로그램은 10~11개입니다. 첫 프레임은 그대로이고, SwiftShader 평균 프레임은 스킨드 메시 때문에 20~40% 느려졌습니다. 실제 GPU에서는 사람이 다시 재야 합니다.
- 4단계(1인칭) 뒤: 그리기 호출 보급창고 95(최대 112), 웨어하우스 100(최대 109). 1인칭은 약 12회에서 2~3회가 됐습니다. 삼각형은 약 6만 개, 셰이더 프로그램은 11~12개입니다.
- 5단계(맵) 뒤:
  - 정적 맵 그리기 호출은 74 → 16(보급창고), 91 → 12(웨어하우스)입니다. 경기 전체로는 보급창고 49(최대 69), 웨어하우스 41(최대 55)입니다.
  - 삼각형은 약 8.5만~9.5만 개, 셰이더 프로그램은 14~18개입니다.
  - 첫 프레임은 +2~9%입니다.
- 6단계(이펙트) 뒤: 경기 전체 그리기 호출 평균 44(최대 49, 보급창고)·40(최대 45, 웨어하우스). 이펙트는 최대 4회이고, 10초 연사 중에도 지오메트리 수가 그대로입니다. 셰이더 프로그램은 17~23개(예산 30)입니다.

## 프로토콜을 바꿀 때

1. `proto/game.proto` 수정 (지운 필드 번호는 `reserved`)
2. 서버는 `cargo build`가 자동 재생성
3. 클라이언트는 `npm run gen` (`dev`/`build`가 자동 실행)

## 수치 조정 위치

| 바꿀 것 | 위치 |
| --- | --- |
| 무기 스탯, 공용 이동속도·탄퍼짐 계수, 증강모드 대미지 | `server/src/game/weapons.rs` |
| 증강 등급 풀, 확률, 선택/대기 시간, 증강 수치 | `server/src/game/augments.rs` |
| 왕대두모드의 레벨 범위·레벨별 머리 배율(`HEAD_SCALES`)·마이너스 레벨 처치 후 레벨(`COMEBACK_LEVEL`)·왕대두 시간(`KING_TIME`)·왕대두 뒤 레벨(`KING_END_LEVEL`) | `server/src/game/big_head.rs` (최저·최고 레벨과 시간은 `GameConfig`로 전달. `hud.ts`의 Tab 규칙 문구 속 1레벨·0레벨은 사본) |
| 뿅망치 대미지·사거리·간격·이동속도(= 왕대두 이동속도) | `server/src/game/weapons.rs`의 `HAMMER` |
| 틱 레이트, 부활·보호 시간, HP, 플레이어 크기, 목표 킬 후보·기본값(팀전 `KILLS_TO_WIN`, 개인전 `KILLS_TO_WIN_FFA`) | `server/src/game/mod.rs` |
| 개인전 스폰 지점(`ffa_spawns`), 맵 박스·엄폐물, 웨어하우스 컨테이너 계단(`WAREHOUSE_CLIMBS`), 기울어진 컨테이너의 기울기·충돌 조각 길이(`LEAN`, `LEAN_SLICE`, 높이 `CONTAINER_H`는 클라이언트 `world.ts`의 `TILTED_HEIGHT`에 사본) | `server/src/game/map.rs` |
| 사격·근접·방패 판정 상수, 블룸 유지(`BLOOM_HOLD_EXTRA`) | `server/src/game/systems/fire.rs` 상단 |
| 블룸 회복(`BLOOM_DECAY`) | `server/src/game/systems/weapon.rs` 상단 |
| 폭발 피해 감쇠(`SPLASH_FALLOFF`) | `server/src/game/systems/blast.rs` 상단 |
| 미사일 속도·수명·명중 여유 | `server/src/game/systems/missile.rs` 상단 |
| 수류탄 던지기·비행·튕김·퓨즈(`THROW_*`, `GRENADE_*`) | `server/src/game/systems/grenade.rs` 상단 |
| 틱당 명령 예산(`CMDS_PER_TICK` 등), lag compensation 되감기 범위(`MAX_REWIND_TICKS`) | `server/src/game/systems/input.rs`, `systems/history.rs` 상단 |
| 수류탄 개수·폭발 대미지·반경·던진 뒤 다음 무기까지 시간 | `server/src/game/weapons.rs`의 `GRENADE` |
| 봇 반응·조준, 수류탄 거리·확률·조준 비율(`GRENADE_*`) | `server/src/game/systems/bot.rs` 상단 |
| 반동(무기별 킥, 상한, 회복) | `client/src/game/game.ts` |
| 점프 이륙 속도·중력(`JUMP_SPEED`·`GRAVITY`, `GameConfig`로 전달) | `server/src/game/mod.rs` |
| 박스 윗면에 선 것으로 볼 여유(`FOOTING`, 클라이언트 `movement.ts`에 사본) | `server/src/game/physics.rs` |
| 발소리 간격·들리는 거리(`STEP_INTERVAL`, `FOOTSTEP_RANGE`), 마우스 휠 감도(`WHEEL_STEP`, `WHEEL_GAP_MS`), 수류탄 투척 후 발사 입력 유지(`THROW_HOLD`)·부딪힘 소리 기준(`GRENADE_CLINK`) | `client/src/game/game.ts` |
| 기본 마우스 감도(배율 1일 때 픽셀당 라디안 `MOUSE_SENSITIVITY`) | `client/src/game/game.ts` |
| 감도 배율 범위·단위·기본값, 저장 키 | `client/src/settings.ts` |
| 처치 배너의 연속 처치 인정 시간(`KILL_STREAK_WINDOW`, 4초) | `client/src/game/game.ts` |
| 처치 배너 이름(`STREAK_NAMES`)·탄환/해골 그림(`BULLET_SVG`, `SKULL_SVG`)·최대 탄환 수, 등장 애니메이션(`style.css`의 `.kill-banner`, `kb-*`) | `client/src/game/hud.ts`, `client/src/style.css` |
| 왕대두모드 화면: 특수 샷 이름·그림(`SPECIAL_KILLS`, `HAMMER_SVG`, `DOT_SVG`), 레벨 패널·알림(`style.css`의 `.level-panel`, `.big-head-notice`), 금관(`crownTexture`, `character.ts`의 `CROWN_*`), 소리(`audio.ts`의 `squeak`·`crowned`·`reignOver`·`specialShot`) | `client/src/game/hud.ts`, `client/src/style.css`, `client/src/game/textures.ts`, `client/src/game/character.ts`, `client/src/game/audio.ts` |
| 카드 이름·설명 | `client/src/augments.ts` |
| 무기 설명 | `client/src/weapons.ts` |

이동 규칙을 바꾸면 `server/src/game/physics.rs`의 `step_move`(와 `systems/input.rs`의 `walk_step`·`consume_inputs`), `client/src/game/movement.ts`를 함께 바꿔야 예측이 어긋나지 않습니다.

## 테스트

### 서버 단위 테스트

```powershell
cd server; cargo test --release
```

테스트는 그 테스트가 검증하는 로직이 있는 파일 맨 아래 `tests` 모듈에 있습니다([server/AGENTS.md](../server/AGENTS.md) 규칙 2). 순수 함수 단위 테스트가 먼저 오고, `Match`를 여러 틱 돌리는 시나리오 테스트가 그 뒤에 옵니다. 시나리오 테스트의 공용 도우미는 `server/src/game/test_support.rs`에 있습니다.

`server/src/hub/`의 테스트(`seats.rs`·`room.rs`·`mod.rs`)는 대기실 칸 배정(입장·팀 변경·봇 양보와 맞바꾸기·방장의 봇 설정·방장 승계·나가기·연결 끊김·시작 조건·기권승 후 대기실 복귀), 준비(방장 외 모두 준비해야 시작, 준비 취소, 방장은 준비 없음, 시작하면 준비 해제), 대기실의 모드·맵 변경(방장만, 같은 값·모르는 값·경기 중 무시, 경기 반영, 왕대두모드 방 만들기·바꾸기와 그 경기의 레벨 설정), 도중 참가(빈 칸에만, 인원이 적은 팀으로, 봇 칸뿐이면 거절, 결과 화면 중 거절, 개인전은 가장 낮은 번호, 증강모드의 카드 처리)와 개인전 방(번호 칸 0~9, 빈칸 우선 착석, 팀 변경 거부, 2명 시작 조건, 기본 20킬, 혼자 남은 사람의 기권승 후 재경기, 11번째 입장 거부)을 검증합니다. `server/src/game/`의 테스트는 각 규칙의 순수 함수(명령 예산·시선·이동 한 걸음, 장전·스코프, 탄 소비·버스트·퍼짐·되감기·첫 명중·피해·특수 샷·처치 보상, 폭발 감쇠, 미사일 비행, 수류탄 투척·튕김, 휴식, 왕대두 시간, 부활, 위치 기록, 선택·대기 시간)와 함께, 시나리오로 게임 규칙을 검증합니다: 이동·충돌, 사격 판정, lag compensation, 무기별 수치, 장전·전환, 부활·보호, 목표 킬, 봇, 증강 카드 배분·교체·선택 시간, 더블 타임 칸 충돌, 증강별 효과(방패, 빙의, 대두, 휴식, 영혼 흡수 등), 미사일 비행·폭발·시야, 테이저 마비, 개인전(누구나 서로 피해, 자기 미사일 면제, 목표 도달 승자, 기권승, 스폰 분산과 중앙 방향, 봇만으로 끝까지 진행), 점프(높이·체공 시간, 상자 위 착지와 가장자리에서 떨어짐, 못 넘는 높이, 머리 위 박스, 공중 재점프 없음, 공중 히트박스), 수류탄(4번 칸·목숨당 1개·없으면 전환 안 됨, 던지면 그 전 칸으로 복귀, 정확히 3초 뒤 놓인 자리에서 폭발, 거리별 피해와 반경 밖 0, 자신·아군 무피해, 벽 엄폐, 수류탄 킬, 벽에 튕기고 바닥에 멈춤, 맵 밖으로 안 나감, 부활 때 보충, 봇 투척), 맵(웨어하우스 컨테이너마다 상자 계단으로 바닥에서 올라감, 기울어진 컨테이너 충돌 조각이 경사를 따라가 낮은 쪽 위로 총알 통과), 왕대두모드(레벨 −5~3과 레벨별 머리 배율, 처치 +1·사망 −1, 마이너스에서 처치하면 1레벨, 다른 모드는 레벨·머리 그대로, 3레벨 즉위와 HP 회복, 뿅망치 외 칸·수류탄·B키 즉시 교체 막힘, 왕대두 무적(총알은 막히고 피해 없음, 수류탄 무피해), 뿅망치 한 방 큐트 샷, 왕대두 이동속도, 도트 샷 조건(가장 작은 머리·동률·더 작은 사람이 있으면 아님·헤드샷만), 정확히 20초 뒤 0레벨과 새 무기, 죽은 뒤 3레벨이 되면 부활 때 즉위, 봇만으로 팀전·개인전이 끝나고 왕대두 봇이 큐트 샷을 냄).

### 서버 포맷·린트

```powershell
cd server; cargo fmt; cargo clippy --all-targets -- -D warnings
```

`Cargo.toml`의 `[lints]`가 `unsafe` 코드(`forbid`)와 넘칠 수 있는 정수 연산(`clippy::arithmetic_side_effects`)을 막고, clippy는 경고 0이어야 통과입니다. 규칙은 [server/AGENTS.md](../server/AGENTS.md)에 있습니다. Claude Code에서는 Stop 훅(`.claude/hooks/server-check.sh`)이 작업을 마칠 때 서버 소스가 마지막 검사 뒤로 바뀌었으면 두 명령을 돌리고, fmt가 파일을 고쳤거나 clippy가 실패하면 종료를 한 번 막습니다. 훅을 새로 받았거나 고쳤으면 Claude Code의 `/hooks` 메뉴를 한 번 열거나 다시 시작해야 적용됩니다.

### 브라우저 e2e (Playwright)

서버를 release로 켠 상태에서 실행합니다. headless Edge + SwiftShader로 렌더링하며, 두 번째 인자 폴더에 스크린샷을 남깁니다(기본 `e2e-shots`, gitignore). 첫 인자를 생략하면 `client/e2e/server.mjs`가 `PORT`(없으면 8800)로 만든 주소를 씁니다.

```powershell
cd client
node e2e/play.mjs "http://localhost:8800/?debug" e2e-shots
```

| 스크립트 | 검증 내용 |
| --- | --- |
| play.mjs | 브라우저 2개로 로그인 → 방 생성/참가 → 손님이 준비하기 전에는 방장의 시작 버튼이 꺼져 있고 준비하면 방장 화면에 READY → 전투, 예측 오차, Esc 화면 감도 2배가 같은 마우스 이동의 회전을 2배로 만들고 기본값 버튼이 되돌리는지, 경기 중 이탈한 자리 비움 |
| full-match.mjs | 게임 중 B키 주무기 선택, 목표 킬 도달 → 결과 → 대기실 복귀 → 재시작, 다음 경기에서 고른 저격소총으로 스코프·볼트·자동 장전 |
| kill-target.mjs | 방장의 목표 킬·게임 모드·맵·칸별 봇 설정(입장 시 봇 양보 포함)이 방 목록과 손님 화면에 반영, 비방장은 선택 상자 없음, 실제 경기 반영(목표 킬, 증강모드, 웨어하우스, 봇 수), 한 팀이 비면 기권승 후 대기실 복귀, 목록 갱신 중 방 만들기 폼 유지 |
| kill-banner.mjs | 처치 배너: 내 처치는 KILL(탄환 1개), 4초 안 연속 처치는 DOUBLE·TRIPLE(헤드샷이면 해골·HEADSHOT 띠)·QUADRA·PENTA·HEXA KILL, 내 사망 뒤 첫 헤드샷은 해골만 있는 HEADSHOT, 4초가 지나면 다시 KILL, 2.4초 뒤 저절로 숨김. 배너마다 처치한 적의 닉네임이 팀 색으로 나오는지. 게임의 킬 이벤트 처리(`Game.onKill`)로 처치를 넣음 |
| grenade.mjs | 4번 칸 수류탄: 무기 칸 표시 `×1`, 4키로 꺼냄(서버 확인), 탄약 칸 `1 / —`, 클릭으로 던지면 소총으로 즉시 복귀하고 `×0`(줄 그음), 서버와 화면에 날아가는 수류탄, 던진 지 약 3초 뒤 놓인 자리에서 폭발 이펙트, 다 던진 뒤 4키 무시, 콘솔 에러 0건 |
| ffa.mjs | 개인전 방: 목록·헤더의 "개인전", 10칸 한 블록(번호로만 된 칸 이름, 팀 변경 없음, 기본 20킬), 혼자 시작 거부, 손님의 첫 빈칸 착석, 방장의 봇 칸 설정이 손님에게 반영. 웨어하우스·봇 2명·10킬 경기: 모두 팀 없음·서로 적, ME/RIVAL 점수 막대와 스냅샷 일치, Tab 순위 한 목록, 중립 킬 피드, 승자만 정확히 10킬이고 승자만 VICTORY. 대기실 복귀 후 두 번째 경기에서 손님이 나가면 방장 기권승 |
| augment.mjs | 모드 선택, 카드 3장, 카드별 교체, 선택 시간, HP, 대기 중 이동 불가, 무기 모델·투과·마비 화면, 실제 샷건 6펠릿 |
| big-head.mjs | 왕대두모드 방(방 만들기 폼에서 선택, 헤더 표시), `GameConfig`의 레벨 −5~3·20초, 모두 0레벨·보통 머리로 시작, 레벨 패널(0레벨, 9칸 게이지), 도트 샷·큐트 샷 배너(그림·연속 처치 띠)와 킬 피드 태그(`Game.onKill`로 넣음), 실제 봇 경기에서 레벨이 바뀌고 모든 머리가 레벨 배율(서버 값과 모델), 내 레벨 패널 일치. 150초 안에 왕대두가 나오면 3레벨·2.6배 머리·뿅망치·금관·알림과 서버가 알린 큐트 샷까지 확인(시간 안에 안 나오면 SKIP), 콘솔 에러 0건 |
| recoil.mjs | 첫 발 정확도, 연사 시 반동 누적과 흔들림, 회복 |
| name-tags.mjs | 벽 뒤 이름표 숨김 |
| wall-rub.mjs | 벽에 비비며 이동 시 끼임 없음 |
| crouch-keys.mjs | Ctrl/C 앉기, 봇 없이 만든 방에 봇 없음 |
| art.mjs | `?art` 미리보기: 에셋 로드 실패·콘솔 에러·`[art]` 경고 0건, 서버 미접속. 캐릭터 벤치로 히트박스 일치(목 높이 서기 1.50·앉기 1.00, pitch ±1.45에서 목 위치, 머리 크기별 머리 꼭대기, 발 높이), 무기별 총구, 캐릭터당 그리기 호출 확인. 무기 자세 카드에 손 클로즈업 두 개를 더 그리고, 왼손 주먹과 받침점 거리 3cm 이내, 왼손 손바닥이 쥐는 방향(핸드가드는 위, 권총 손잡이는 오른쪽)을 향하는지, 나이프 칼끝이 주먹보다 앞·위인지, 수류탄 신관과 뿅망치 머리가 주먹 위로 나오는지, 나이프 손목 꺾임이 20° 이하인지 확인(나이프·수류탄·뿅망치는 조준 방향 검사에서 뺌). 왕대두 상태(2.6배 머리, 뿅망치, 왕관)는 머리 꼭대기 높이, 왕관이 머리 위에 얹히고 이름표가 그 위에 있는지, 왕관이 그리기 호출을 하나만 더하는지(다른 상태에는 왕관 없음) 확인. 1인칭 벤치(무기별 대기·반동·재장전·볼트/펌프·칼질·뿅망치 내려치기·수류탄 던지기, 빨강/파랑, 나이프는 개인전 중립 소매 포함)로 각 주먹과 손잡이·받침점의 거리 1.5cm 이내, 받침손 손바닥이 위를 향하는지, 나이프 손목 꺾임 20° 이하(다른 무기의 손목 꺾임은 1·3인칭 INFO로 출력), 대기 중 총구와 무기를 쥔 손(한손 무기의 오른손, 아킴보의 두 손, 두손 총의 받침손)이 화면 안, 그리기 호출 5 이하, 팔 삼각형 예산 확인. 맵 벤치(모든 BoxKind·PropKind)로 키트 인스턴스와 충돌 박스의 1cm 이내 일치, 컨테이너 색, 텍스처 붙은 소품 판(셔터·광고판·창·환기창)이 원래 텍스처와 상관 0.8 이상, 정적 맵 그리기 호출 35 이하 확인. 이펙트 벤치로 모든 레시피 동시 재생 시 그리기 호출 예산, 풀 상한, 데칼이 박스 면(6방향)에 붙는지 확인 |
| perf.mjs | 맵별 봇 경기에서 draw call·삼각형·셰이더 프로그램·프레임 시간 측정(`e2e-shots/perf.json`). manifest 예산으로 판정: 경기 전체 평균 100·최대 130 이하, 정적 맵(`"world"` 그룹) 35 이하. 10초 연사 구간에서 이펙트 그리기 호출 예산과 지오메트리 수 고정(발사마다 할당 없음) 확인 |

Windows에서 서버를 띄우고 테스트하는 패턴:

```powershell
$s = Start-Process ..\server\target\release\sudden-front-server.exe -PassThru -WindowStyle Hidden
node e2e/augment.mjs "http://localhost:8800/?debug" e2e-shots
Stop-Process $s.Id
```

이전 서버 프로세스가 남아 있으면 exe가 잠겨 빌드가 실패하거나 옛 바이너리로 테스트하게 되므로 먼저 종료합니다.
