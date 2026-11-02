# 구조

## 디렉터리

```
proto/game.proto            패킷 정의의 단일 원본 (Rust·TS 코드 모두 여기서 생성)
server/
  build.rs                  prost-build로 game.proto → game.v1.rs
  src/main.rs               axum 라우터: /ws 와 client/dist 정적 파일
  src/net.rs                WebSocket 세션 ↔ 허브 스레드 (mpsc 채널)
  src/hub.rs                로비, 방, 방장 권한, 30Hz 틱 루프
  src/game/
    mod.rs                  Match: World + Schedule, 스냅샷 생성, 증강 적용, 대기/선택 단계
    components.rs           ECS 컴포넌트와 리소스
    systems.rs              틱마다 도는 규칙 시스템 (이동, 무기, 사격, 미사일, 부활, 기록)
    weapons.rs              무기 스탯 표, 모드별 대미지, 증강에 따른 무기 교체
    augments.rs             증강 등급 풀, 카드 배분, 더블 타임 추첨, 증강 상수
    bot.rs                  봇 AI
    map.rs                  맵 데이터 (보급창고, 웨어하우스)
    tests.rs                규칙 단위 테스트 (85개)
client/
  src/main.ts               진입점, 화면 전환
  src/net.ts                WebSocket 연결, encode/decode
  src/lobby.ts              로그인, 방 목록, 대기실 (모드·맵·목표 킬 선택)
  src/weapons.ts            무기 설명 문구
  src/augments.ts           증강 카드 이름·설명·등급 (규칙은 서버에 있음)
  src/game/
    game.ts                 게임 루프, 입력, 예측·보정, 반동, 스냅샷 반영, 증강 UI 연결
    movement.ts             이동/충돌 (서버 step_move와 동일해야 함)
    world.ts                맵 렌더링
    character.ts            원격 플레이어 모델 (머리 크기, 방패, 마비 스파크, 무기 모델)
    viewmodel.ts            1인칭 무기 모델
    effects.ts              총구 화염, 탄흔, 폭발, 미사일 꼬리
    hud.ts                  HUD, 점수판, 증강 카드 화면, 무기 슬롯, 마비 오버레이
    audio.ts                WebAudio 절차 사운드
    textures.ts             캔버스 절차 텍스처
  src/gen/game_pb.ts        npm run gen 으로 생성 (gitignore)
  e2e/*.mjs                 Playwright 브라우저 테스트
```

## 서버 스레드 모델

- tokio 런타임이 WebSocket 연결을 받고, 연결마다 `net::client_session`이 돕니다. 받은 바이너리 프레임은 `ClientMessage`로 디코딩해 `std::sync::mpsc`로 허브 스레드에 넘깁니다. 16KB를 넘는 프레임과 디코딩 실패 프레임은 버립니다.
- `game-hub` 스레드 하나가 모든 방을 소유합니다. 30Hz로 돌며 받은 메시지를 처리하고 진행 중인 방마다 `Match`를 한 틱씩 진행한 뒤 스냅샷을 보냅니다. 클라이언트별 송신 큐(512)가 가득 차면 그 클라이언트를 끊습니다.
- 방 상태: 대기실(Lobby) → 진행(InGame) → 결과(8초) → 대기실 복귀. 최대 10명, 팀당 5명.

## 방마다 하나의 ECS World

`Match`는 bevy_ecs `World`와 순서가 고정된(chained) `Schedule`을 가집니다.

```
bot_think → consume_inputs → update_weapons → fire_weapons → update_perks
          → respawn_players → record_history → advance_tick
```

| 시스템 | 역할 |
| --- | --- |
| bot_think | 봇이 사람과 같은 `InputCmd` 큐에 입력을 넣음 |
| consume_inputs | 틱당 최대 2개(누적 8개) 명령을 1/60초 단위로 적용: 이동, 시점, 앉기, 무기 슬롯 전환. 마비 중이면 시점만 적용 |
| update_weapons | 장전, 꺼내기(0.35초), 볼트/펌프 사이클, 블룸 감소 |
| fire_weapons | 사격 판정: 히트스캔/근접/산탄/테이저, lag compensation, 미사일 발사와 비행·폭발(`fly_missiles`), 대미지·킬 이벤트 |
| update_perks | 증강의 지속 효과: 때로는 휴식이(앉아 있을 때 회복) |
| respawn_players | 사망 3초 후 재투입, 3초 보호, 증강에 맞는 무기 지급 |
| record_history | lag compensation용 위치 기록 (최대 15틱 되감기) |
| advance_tick | 틱 번호 증가, 경기 시계 |

경기 시작 대기(증강 선택 15초 + 대기 5초)는 `Match::tick`에서 스케줄을 돌리지 않고 `freeze_tick`만 진행합니다.

## 네트워크 동기화

### 클라이언트 → 서버
- 클라이언트는 60Hz 고정 스텝으로 `InputCmd`(seq, 이동키, yaw/pitch, 사격, 장전, 스코프, 슬롯, 앉기, render_tick)를 만들어 `InputBatch`로 보냅니다.
- 사격 명령에는 반동이 반영된 조준각(`aimYaw`/`aimPitch`)이 실립니다. 서버는 한 틱 안의 첫 번째 사격 명령의 조준각을 씁니다(마지막 것을 쓰면 첫 발이 위로 튀는 문제가 있었음).
- 연사 쿨다운은 `COOLDOWN_SLACK = 0.002`초의 여유를 둬서 60Hz 명령과 30Hz 틱 사이의 반올림 오차로 발사가 밀리지 않게 합니다.

### 이동 예측과 보정
- 클라이언트는 입력을 즉시 자기 위치에 적용(예측)하고, 스냅샷의 `last_input_seq` 이후 명령을 다시 적용(replay)해 서버 위치에 맞춥니다.
- `client/src/game/movement.ts`는 서버 `systems.rs`의 `step_move`와 같은 규칙이어야 합니다. 벽에 비비는 경우 축별로 미끄러지게 처리합니다.

### Lag compensation
- 클라이언트는 원격 플레이어를 보간해서 그리며, 지금 보고 있는 서버 틱을 `render_tick`으로 보냅니다.
- 서버는 사격 시 표적들을 그 틱 위치(`History`)로 되돌려 판정합니다. 되감기는 최대 15틱(0.5초)입니다.
- 미사일은 실제로 날아가는 투사체라 lag compensation 대상이 아닙니다.

### 스냅샷
- 매 틱 모든 플레이어 상태와 그 틱의 이벤트(사격, 대미지, 킬, 장전, 부활, 폭발)를 보냅니다.
- 수신자별 필드: 탄약, 장전 남은 시간, 선택 무기, 주무기/보조무기, 제시된 증강 카드와 교체 가능 여부.
- 공용 필드: 점수, 대기/선택 남은 시간, 경기 시계, 비행 중인 미사일.

## 클라이언트 렌더링

- three.js 씬 하나에 맵, 원격 캐릭터, 1인칭 뷰모델(별도 카메라 레이어), 이펙트를 그립니다.
- 이름표는 벽에 가려지면 숨깁니다(레이캐스트).
- 투과(Wallhack) 증강은 적 캐릭터를 depthTest 없는 단색 패스로 한 번 더 그립니다.
- `?debug` 쿼리로 열면 `window.__game`이 노출되어 e2e 테스트가 내부 상태를 읽습니다.
