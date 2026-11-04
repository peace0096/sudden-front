# 개발 가이드

## 준비물

- Rust 1.89 이상(`bevy_ecs` 0.18 요구), Node.js 22.12 이상(`@bufbuild/protoc-gen-es`, Vite 8 요구)
- Microsoft Edge (e2e 테스트용, playwright-core가 `msedge` 채널 사용)

## 빌드와 실행

```powershell
cd client; npm install; npm run build      # proto 생성 → 타입 검사 → client/dist
cd ..\server; cargo run --release          # http://localhost:8800
```

- 서버가 `client/dist`를 정적 파일로 제공합니다. 경로는 `CLIENT_DIST` 환경 변수로 바꿉니다.
- 포트:
  - `PORT`가 없으면 8800을 쓰고, 사용 중이면 8801~8819 중 첫 빈 포트, 그것도 없으면 OS가 고른 포트를 씁니다. 옮겼을 때는 준비 줄 앞에 `Port 8800 is in use; using 8801. …`을 출력합니다.
  - `PORT`를 지정하면 그 포트만 씁니다. 사용 중이거나 숫자가 아니면 한 줄 오류를 출력하고 종료 코드 1로 끝납니다.
  - 사용 여부는 bind 전에 127.0.0.1·::1로 접속해 먼저 확인합니다. macOS에서는 다른 프로세스가 loopback에만 연 포트에도 0.0.0.0 bind가 성공해서, `localhost`가 그 프로세스로 연결되기 때문입니다.
- 같은 네트워크의 다른 PC는 `http://<서버 IP>:8800`으로 접속합니다.
- 클라이언트 개발: 서버를 켠 채 `cd client; npm run dev` → http://localhost:5173. Vite는 `/ws`를 `PORT`(없으면 8800)로 프록시하므로, 서버가 다른 포트로 열렸으면 같은 `PORT`를 주고 실행합니다.
- `http://localhost:8800/?debug`로 열면 `window.__game`이 노출됩니다.

## 프로토콜을 바꿀 때

1. `proto/game.proto` 수정 (지운 필드 번호는 `reserved`)
2. 서버는 `cargo build`가 자동 재생성
3. 클라이언트는 `npm run gen` (`dev`/`build`가 자동 실행)

## 수치 조정 위치

| 바꿀 것 | 위치 |
| --- | --- |
| 무기 스탯, 공용 이동속도·탄퍼짐 계수, 증강모드 대미지 | `server/src/game/weapons.rs` |
| 증강 등급 풀, 확률, 선택/대기 시간, 증강 수치 | `server/src/game/augments.rs` |
| 틱 레이트, 부활·보호 시간, HP, 플레이어 크기, 목표 킬 후보 | `server/src/game/mod.rs` |
| 사격·미사일·방패 판정 상수 | `server/src/game/systems.rs` 상단 |
| 봇 반응·조준 | `server/src/game/bot.rs` 상단 |
| 반동(무기별 킥, 상한, 회복) | `client/src/game/game.ts` |
| 카드 이름·설명 | `client/src/augments.ts` |
| 무기 설명 | `client/src/weapons.ts` |

이동 규칙을 바꾸면 `server/src/game/physics.rs`의 `step_move`(와 `systems.rs` `consume_inputs`의 이동 부분), `client/src/game/movement.ts`를 함께 바꿔야 예측이 어긋나지 않습니다.

## 테스트

### 서버 단위 테스트

```powershell
cd server; cargo test --release
```

`server/src/hub.rs`의 23개 테스트는 대기실 칸 배정(입장·팀 변경·봇 양보와 맞바꾸기·방장의 봇 설정·방장 승계·나가기·연결 끊김·시작 조건·기권승 후 대기실 복귀)을 검증합니다. `server/src/game/tests.rs`의 91개 테스트(1개는 디버그 빌드 전용)가 게임 규칙을 검증합니다: 이동·충돌, 사격 판정, lag compensation, 무기별 수치, 장전·전환, 부활·보호, 목표 킬, 봇, 증강 카드 배분·교체·선택 시간, 더블 타임 칸 충돌, 증강별 효과(방패, 빙의, 대두, 휴식, 영혼 흡수 등), 미사일 비행·폭발·시야, 테이저 마비.

### 브라우저 e2e (Playwright)

서버를 release로 켠 상태에서 실행합니다. headless Edge + SwiftShader로 렌더링하며, 두 번째 인자 폴더에 스크린샷을 남깁니다(기본 `e2e-shots`, gitignore). 첫 인자를 생략하면 `client/e2e/server.mjs`가 `PORT`(없으면 8800)로 만든 주소를 씁니다.

```powershell
cd client
node e2e/play.mjs "http://localhost:8800/?debug" e2e-shots
```

| 스크립트 | 검증 내용 |
| --- | --- |
| play.mjs | 브라우저 2개로 로그인 → 방 생성/참가 → 전투, 예측 오차, 경기 중 이탈한 자리 비움 |
| full-match.mjs | 게임 중 B키 주무기 선택, 목표 킬 도달 → 결과 → 대기실 복귀 → 재시작, 다음 경기에서 고른 저격소총으로 스코프·볼트·자동 장전 |
| kill-target.mjs | 방장의 목표 킬·칸별 봇 설정(입장 시 봇 양보 포함), 비방장 불가, 실제 경기 반영(목표 킬, 봇 수), 한 팀이 비면 기권승 후 대기실 복귀, 목록 갱신 중 방 만들기 폼 유지 |
| augment.mjs | 모드 선택, 카드 3장, 카드별 교체, 선택 시간, HP, 대기 중 이동 불가, 무기 모델·투과·마비 화면, 실제 샷건 6펠릿 |
| recoil.mjs | 첫 발 정확도, 연사 시 반동 누적과 흔들림, 회복 |
| name-tags.mjs | 벽 뒤 이름표 숨김 |
| wall-rub.mjs | 벽에 비비며 이동 시 끼임 없음 |
| crouch-keys.mjs | Ctrl/C 앉기, 봇 없이 만든 방에 봇 없음 |

Windows에서 서버를 띄우고 테스트하는 패턴:

```powershell
$s = Start-Process ..\server\target\release\sudden-front-server.exe -PassThru -WindowStyle Hidden
node e2e/augment.mjs "http://localhost:8800/?debug" e2e-shots
Stop-Process $s.Id
```

이전 서버 프로세스가 남아 있으면 exe가 잠겨 빌드가 실패하거나 옛 바이너리로 테스트하게 되므로 먼저 종료합니다.
