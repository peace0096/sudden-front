# 개발 가이드

## 준비물

- Rust (stable), Node.js 20+
- Microsoft Edge (e2e 테스트용, playwright-core가 `msedge` 채널 사용)

## 빌드와 실행

```powershell
cd client; npm install; npm run build      # proto 생성 → 타입 검사 → client/dist
cd ..\server; cargo run --release          # http://localhost:8080
```

- 서버가 `client/dist`를 정적 파일로 제공합니다. 경로는 `CLIENT_DIST`, 포트는 `PORT` 환경 변수로 바꿉니다.
- 같은 네트워크의 다른 PC는 `http://<서버 IP>:8080`으로 접속합니다.
- 클라이언트 개발: 서버를 켠 채 `cd client; npm run dev` → http://localhost:5173
- `http://localhost:8080/?debug`로 열면 `window.__game`이 노출됩니다.

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

이동 규칙을 바꾸면 `server/src/game/systems.rs`의 `step_move`와 `client/src/game/movement.ts`를 함께 바꿔야 예측이 어긋나지 않습니다.

## 테스트

### 서버 단위 테스트

```powershell
cd server; cargo test --release
```

`server/src/game/tests.rs`의 85개 테스트가 규칙을 검증합니다: 이동·충돌, 사격 판정, lag compensation, 무기별 수치, 장전·전환, 부활·보호, 목표 킬, 봇, 증강 카드 배분·교체·선택 시간, 더블 타임 칸 충돌, 증강별 효과(방패, 빙의, 대두, 휴식, 영혼 흡수 등), 미사일 비행·폭발·시야, 테이저 마비.

### 브라우저 e2e (Playwright)

서버를 release로 켠 상태에서 실행합니다. headless Edge + SwiftShader로 렌더링하며, 두 번째 인자 폴더에 스크린샷을 남깁니다(기본 `e2e-shots`, gitignore).

```powershell
cd client
node e2e/play.mjs "http://localhost:8080/?debug" e2e-shots
```

| 스크립트 | 검증 내용 |
| --- | --- |
| play.mjs | 브라우저 2개로 로그인 → 방 생성/참가 → 전투, 예측 오차 |
| full-match.mjs | 목표 킬 도달 → 결과 → 대기실 복귀 → 재시작 |
| kill-target.mjs | 방장의 목표 킬 선택, 비방장 불가, 실제 경기 반영 |
| augment.mjs | 모드 선택, 카드 3장, 카드별 교체, 선택 시간, HP, 대기 중 이동 불가, 무기 모델·투과·마비 화면, 실제 샷건 6펠릿 |
| recoil.mjs | 첫 발 정확도, 연사 시 반동 누적과 흔들림, 회복 |
| name-tags.mjs | 벽 뒤 이름표 숨김 |
| wall-rub.mjs | 벽에 비비며 이동 시 끼임 없음 |
| crouch-keys.mjs | Ctrl/C 앉기 |

Windows에서 서버를 띄우고 테스트하는 패턴:

```powershell
$s = Start-Process ..\server\target\release\sudden-front-server.exe -PassThru -WindowStyle Hidden
node e2e/augment.mjs "http://localhost:8080/?debug" e2e-shots
Stop-Process $s.Id
```

이전 서버 프로세스가 남아 있으면 exe가 잠겨 빌드가 실패하거나 옛 바이너리로 테스트하게 되므로 먼저 종료합니다.
