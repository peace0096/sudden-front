# 구조

## 디렉터리

```
proto/game.proto            패킷 정의의 단일 원본 (Rust·TS 코드 모두 여기서 생성)
server/
  build.rs                  prost-build로 game.proto → game.v1.rs
  src/main.rs               axum 라우터: /ws, /art (없는 파일은 404), client/dist 정적 파일, 포트 선택 (기본 8800, 사용 중이면 빈 포트)
  src/net.rs                WebSocket 세션 ↔ 허브 스레드 (mpsc 채널)
  src/hub.rs                로비, 방(팀별 5칸: 사람·봇·빈칸, 개인전은 같은 10칸을 번호 0~9로 씀), 방장 권한, 30Hz 틱 루프, 칸 배정·준비·도중 참가 단위 테스트 (43개)
  src/game/
    mod.rs                  Match: World + Schedule, 스냅샷 생성, 증강 적용, 대기/선택 단계
    components.rs           ECS 컴포넌트와 리소스
    systems.rs              틱마다 도는 규칙 시스템 (이동, 무기, 사격, 미사일, 수류탄, 부활, 기록)
    physics.rs              이동 충돌(step_move), 레이·시야 판정
    weapons.rs              무기 스탯 표, 모드별 대미지, 증강에 따른 무기 교체
    augments.rs             증강 등급 풀, 카드 배분, 더블 타임 추첨, 증강 상수
    bot.rs                  봇 AI
    nav.rs                  봇용 보행 격자와 BFS 경로 탐색
    map.rs                  맵 데이터 (보급창고, 웨어하우스), 팀 스폰과 개인전 스폰(`ffa_spawns`)
    tests.rs                규칙 단위 테스트 (109개, 1개는 디버그 빌드 전용)
client/
  src/main.ts               진입점, 화면 전환, 에셋 프리로드(에셋이 준비되기 전 경기가 시작되면 "에셋 불러오는 중…" 오버레이로 대기), ?art 미리보기 분기
  src/net.ts                WebSocket 연결, encode/decode
  src/lobby.ts              로그인, 방 목록·방 생성 (대전 방식·모드·맵·봇 채우기 초기값), 대기실 (팀 변경, 방장 외 플레이어의 준비 버튼과 READY 표시, 방장의 목표 킬·칸별 봇 설정과 모두 준비해야 켜지는 시작 버튼. 개인전은 10칸 한 블록, 팀 변경 없음). 로비가 떠 있으면 방 목록 패널만 다시 그림
  src/settings.ts           개인 설정: 마우스 감도 배율(0.1~5, 0.05 단위, 기본 1)을 localStorage에 저장·읽기, 로비 방 목록과 게임 Esc 화면이 함께 쓰는 감도 슬라이더(화면에 있는 슬라이더끼리 값이 맞춰짐)
  src/weapons.ts            무기 설명 문구, 게임 중 B키 무기 선택 카드
  src/augments.ts           증강 카드 이름·설명·등급 (규칙은 서버에 있음)
  src/game/
    game.ts                 게임 루프, 입력, 예측·보정, 반동, 스냅샷 반영, 증강 UI 연결, 아군 판단 `isAlly()`(개인전은 늘 거짓)
    movement.ts             이동/충돌 (서버 step_move와 동일해야 함)
    world.ts                맵 렌더링: 하늘·조명·바닥·맵을 "world" 그룹으로 반환. 컨테이너(4색·기울어진 것)·상자·드럼통·가로등은 키트 모델을 InstancedMesh로 충돌 박스에 맞춤(1cm 이내), 벽돌·콘크리트·지붕은 병합 박스와 코드 텍스처, 트러스·바닥선은 각각 한 메시, 셔터·광고판·창·환기창·천장 조명은 종류별 인스턴스. 키트가 없으면 그 종류만 단색 박스로 그림
    character.ts            원격 플레이어: glTF SWAT(팀별·개인전 중립 군복 템플릿을 SkeletonUtils.clone, 개인전 이름표는 적 빨강), AnimationMixer 이동 블렌드, 무기 소켓(총 손잡이가 오른 주먹을 지남), 왼손 받침(주먹이 받침점을 감싸도록 손 방향과 손목 IK), 머리 크기, 방패, 마비 스파크, 원격 발사 킥. 시야 절두체 밖 캐릭터는 애니메이션 계산을 건너뛰고 다음에 보일 때 따라잡음(setCharacterView)
    viewmodel.ts            1인칭: SWAT 팔(빗장뼈 아래 삼각형만 그림)과 glTF 총. 총은 manifest `fp` 위치(기존 화면 구도)에 두고, 오른손 주먹은 손잡이를·왼손 주먹은 받침점(손바닥을 위로 해 핸드가드를 감쌈)을 쥐도록 손 방향을 정한 뒤 그 주먹 위치가 나오는 손목으로 rig.ts IK를 맞춤. 나이프는 칼을 아래팔 축에 맞춰 돌려 손목이 곧음. 팀 색 소매(개인전은 중립 군복, setTeam 때 처음 만들고, 만들기에 실패한 팀은 다시 시도하지 않음), 한손 무기는 왼팔을 내림, 아킴보는 왼손에 반전된 두 번째 총
    effects.ts              이펙트 레시피: 총구 화염, 재질별 착탄, 땅 먼지, 피(벽 핏자국 1.5m 이내), 폭발(섬광·불덩이·충격파·불꽃·연기·그을음, 동적 광원 없음), 미사일 꼬리. 합계 그리기 호출 4 이하, 발사마다 할당 없음
    particles.ts            ParticleBatch(카메라를 보는 인스턴스 사각형, CPU 시뮬레이션, 알파·가산 두 배치)와 TracerBatch(예광탄 리본 링 버퍼)
    decals.ts               DecalPool: 박스 면과 땅(y=0)에 붙는 탄흔 96개·핏자국/그을음 32개 링 버퍼(곱하기 합성, 드럼통·INVISIBLE 제외)
    hud.ts                  HUD, 점수판(개인전은 `setFfa`로 ME/RIVAL 점수 막대·순위 한 목록·중립 킬 피드. Tab으로 열면 아래에 조작키와, 증강모드(`setAugmentMode`)에서는 보유 증강 설명), 증강 카드 화면, 무기 슬롯, 마비 오버레이
    audio.ts                WebAudio 절차 합성 사운드. 무기별 발사음·볼트·펌프를 OfflineAudioContext로 미리 렌더링해 재생, 마스터 리미터
    textures.ts             캔버스 절차 텍스처
    perf.ts                 draw call·삼각형·셰이더 프로그램·프레임 시간 측정 (Game.perf(), ?debug&perf 오버레이)
    art/
      manifest.json         에셋 등록의 단일 원본 (무기·캐릭터(`files`: red·blue·개인전 neutral, `teamFile()`로 고름)와 자세별 뼈 오프셋(holds, 나이프의 주먹 쥐기 fist)·오른 주먹 속 손잡이 자리(tp.position·rotation, build-art가 리그와 대조)·3인칭 왼손 주먹이 감쌀 받침점(tp.support, 권총류는 오른 주먹 옆에서 손잡이를 함께 쥐는 supportGrip)·1인칭 배치(fp: 위치·배율·support·oneHanded·pump·반동)·팔(arms.scale)·맵·이펙트·예산)
      rig.ts                절차적 뼈 레이어: 상체 정면 유지, 자세(holds)와 왼손 받침(손 방향 + 주먹 위치에서 거꾸로 구한 손목 IK, 1·3인칭 공용), 검지 말아 쥐기, 조준 pitch 분배, 앉기(다리 2관절 IK), 머리 크기, 벤치용 손목 꺾임 측정(wristBend)
      manifest.ts           manifest 타입과 조회 함수
      generated.json        변환 결과의 파일별 sha256·?v=용 앞 16자 해시·크기·삼각형 수 (스크립트가 쓰고 check-art가 내용 잠금으로 대조)
      loader.ts             loadArt(): GLB·WebP 로드(매직 바이트 확인, meshopt, GLTFLoader 청크 60초 제한, Safari·구 Firefox는 <img>로 디코드), 실패는 [art] 경고와 failures 목록이고 다음 경기 시작 때 한 번 다시 받음, releaseGpu()
      materials.ts          glTF 재질을 MeshLambertMaterial로 변환·공유
      placeholder.ts        로드 실패 시 쓰는 대체 모형
      hitbox.ts             서버 히트박스·CROUCH_DROP·방패 크기의 클라이언트 사본
      preview.ts            ?art 미리보기 페이지와 캐릭터·1인칭·맵 벤치 (서버 미접속, 결과는 window.__artReport)
      effects-bench.ts      ?art 이펙트 벤치 (고정 시간 간격·시드 고정으로 레시피별 프레임 재현)
  src/gen/game_pb.ts        npm run gen 으로 생성 (gitignore)
  public/art/               glTF·WebP 에셋과 LICENSES.json (파일별 출처·라이선스)
  scripts/check-art.mjs     빌드 때 에셋 등록·라이선스·예산 검사
  e2e/*.mjs                 Playwright 브라우저 테스트
```

## 서버 스레드 모델

- tokio 런타임이 WebSocket 연결을 받고, 연결마다 `net::client_session`이 돕니다. 받은 바이너리 프레임은 `ClientMessage`로 디코딩해 `std::sync::mpsc`로 허브 스레드에 넘깁니다. 디코딩에 실패한 프레임은 버리고, 16KB를 넘는 프레임이 오면 연결을 끊습니다.
- `game-hub` 스레드 하나가 모든 방을 소유합니다. 30Hz로 돌며 받은 메시지를 처리하고 진행 중인 방마다 `Match`를 한 틱씩 진행한 뒤 스냅샷을 보냅니다. 클라이언트별 송신 큐(512)가 가득 차면 그 클라이언트를 끊습니다.
- 방 상태: 대기실(Lobby) → 진행(InGame) → 결과(8초) → 대기실 복귀. 최대 10명, 팀당 5명.
- 대기실 자리는 팀마다 위치가 고정된 5칸(사람·봇·빈칸)입니다. 경기를 시작하면 팀마다 봇 칸 수만큼 봇이 들어가고, 경기 중 나간 사람의 자리는 채우지 않습니다. 한 팀이 비면 `Match::remove_player`가 남은 팀을 승자로 정하고(기권승), 평소처럼 결과 → 대기실 복귀로 이어집니다.
- 도중 참가: 경기 중인 방의 `JoinRoom`은 `join_running_match`가 받습니다. 빈 칸에만 앉히고(`Seats::sit_open`, 팀은 `Room::seat_mid_match`가 인원이 적은 쪽으로), `Match::add_player`가 재투입과 같은 규칙(적에게서 먼 스폰, 3초 보호, 증강모드면 카드 선택 중엔 패를 주고 그 뒤엔 즉시 무작위)으로 넣습니다. 들어온 사람에게는 `MatchStart`, 다른 사람에게는 `RosterUpdate`를 보냅니다. 결과 화면 중(또는 승자가 정해진 틱)에는 거절합니다.
- 방의 대전 방식(`Room.match_type`)은 만들 때 정해지고 바뀌지 않습니다. 개인전 방도 같은 `Seats([[Seat; 5]; 2])` 저장을 쓰며, 칸 i를 `(i / 5, i % 5)`로 읽어 0~9번 한 줄로 다룹니다(`iter()` 순서가 곧 번호 순서). 와이어로는 `team` UNSPECIFIED와 `slot` 0~9로 주고받고, 새로 온 사람은 번호가 가장 앞선 빈칸(없으면 봇 칸)에 앉습니다.
- 개인전 선수는 `Team::Unspecified`입니다. 피해를 줄 수 있는지는 `Rules::hostile(a, b)`(`ffa || a != b`) 한 곳에서 정하고, 히트스캔·근접·미사일 접촉·폭발이 저마다 자기 자신(미사일은 쏜 사람)을 뺍니다. 승자는 `Winner::Team` 또는 `Winner::Player`이고, 개인전은 킬을 더한 직후 목표에 닿은 사람이, 혼자 남으면 그 사람이 이깁니다. 개인전 스폰은 맵별로 고른 `ffa_spawns` 중 다른 생존자에게서 가장 먼 점입니다.

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
| fire_weapons | 사격 판정: 히트스캔/근접/산탄/테이저, lag compensation, 미사일 발사와 비행·폭발(`fly_missiles`), 수류탄 투척(`throw_grenade`)과 비행·튕김·폭발(`fly_grenades`, `roll_grenade`가 `physics::sweep_ball`로 충돌), 대미지·킬 이벤트 |
| update_perks | 증강의 지속 효과: 때로는 휴식이(앉아 있을 때 회복) |
| respawn_players | 사망 3초 후 재투입, 3초 보호, 증강에 맞는 무기 지급 |
| record_history | lag compensation용 위치 기록 (최대 15틱 되감기) |
| advance_tick | 틱 번호 증가, 경기 시계 |

경기 시작 대기(증강 선택 15초 + 대기 5초)는 `Match::tick`에서 스케줄을 돌리지 않고 `freeze_tick`만 진행합니다.

## 네트워크 동기화

### 클라이언트 → 서버
- 클라이언트는 60Hz 고정 스텝으로 `InputCmd`(seq, 이동키, yaw/pitch, 사격, 장전, 스코프, 슬롯, 앉기, render_tick)를 만들어 `InputBatch`로 보냅니다.
- 사격 명령의 `yaw`/`pitch`에는 반동이 반영된 조준각이 실립니다. 서버는 한 틱 안의 첫 번째 사격 명령의 조준각을 씁니다(마지막 것을 쓰면 첫 발이 위로 튀는 문제가 있었음).
- 연사 쿨다운은 `COOLDOWN_SLACK = 0.002`초의 여유를 둬서 60Hz 명령과 30Hz 틱 사이의 반올림 오차로 발사가 밀리지 않게 합니다.

### 이동 예측과 보정
- 클라이언트는 입력을 즉시 자기 위치에 적용(예측)하고, 스냅샷의 `last_input_seq` 이후 명령을 다시 적용(replay)해 서버 위치에 맞춥니다.
- `client/src/game/movement.ts`는 서버 `physics.rs`의 `step_move`·`step_vertical`, `systems.rs` `consume_inputs`의 이동 부분과 같은 규칙이어야 합니다. 벽에 비비는 경우 축별로 미끄러지게 처리합니다.
- 한 명령은 수평 이동(그때 발 높이에서 발~머리 사이에 걸친 박스만 막음) → 수직 이동(서 있으면 점프, 중력, 머리 위 박스와 발밑 바닥·박스 윗면에서 멈춤) 순서입니다. `vy`가 0이고 발이 바닥 높이에 있으면 서 있는 상태입니다. 보정 때는 서버의 `y`·`vy`에서 다시 적용합니다. 마비 중에는 중력만 받고, 증강 선택 대기 중에는 서버가 명령을 이동에 쓰지 않습니다.

### Lag compensation
- 클라이언트는 원격 플레이어를 보간해서 그리며, 지금 보고 있는 서버 틱을 `render_tick`으로 보냅니다.
- 서버는 사격 시 표적들을 그 틱 위치(`History`)로 되돌려 판정합니다. 되감기는 최대 15틱(0.5초)입니다.
- 미사일과 수류탄은 실제로 날아가는 투사체라 lag compensation 대상이 아닙니다.

### 스냅샷
- 매 틱 모든 플레이어 상태와 그 틱의 이벤트(사격, 대미지, 킬, 장전, 부활, 폭발, 수류탄 투척)를 보냅니다.
- 수신자별 필드: 탄약, 장전 남은 시간, 선택 무기, 주무기/보조무기, 남은 수류탄, 제시된 증강 카드와 교체 가능 여부.
- 공용 필드: 점수, 대기/선택 남은 시간, 경기 시계, 비행 중인 미사일, 터지지 않은 수류탄.
- 수류탄 투척은 클라이언트가 바로 예측합니다(던지기 동작·소리, 그 전 칸 무기로 복귀). 서버가 수류탄을 한 틱 늦게 꺼냈을 수 있어 발사 입력을 8명령(`THROW_HOLD`) 더 보내고, 그동안의 스냅샷은 무기·탄약 표시에 쓰지 않습니다. 돌아온 무기는 0.6초 동안 쏠 수 없어 이 입력으로 발사되지 않습니다.

## 클라이언트 렌더링

- 월드 씬에 맵, 원격 캐릭터, 이펙트를 그립니다. 1인칭 뷰모델은 별도 씬·카메라(FOV 50)로, 깊이를 지운 뒤 그 위에 그립니다(스코프 중에는 생략).
- 이름표는 벽에 가려지면 숨깁니다(레이캐스트).
- 아군·적 구분은 `Game.isAlly()` 한 곳에서 합니다. 개인전에서는 늘 거짓이라 다른 플레이어 모두에게 적 규칙(조준할 때만 이름표, 쏠 때만 레이더, 투과)이 적용됩니다. 개인전에서는 `MatchEnd.winner`와 내 팀이 둘 다 UNSPECIFIED라, 승패는 `winner_id`로만 판단합니다.
- 투과(Wallhack) 증강은 적 캐릭터(카메라 레이어 1)를 깊이 비교를 반대로 한(`GreaterDepth`) 반투명 단색 재질로 한 번 더 그려, 벽에 가려진 부분만 보이게 합니다. 스킨드 메시도 포즈가 적용된 채 그려지고, `polygonOffset`으로 겹침 깜빡임을 막습니다.
- 원격 캐릭터는 이동 방향·속도를 보간 위치에서 추정해 run·runBack·strafe 클립을 섞고(서버의 `moving` 플래그는 약 3틱 앞서 다리가 일찍 멈추기 때문), 그 위에 `rig.ts`가 자세·조준·앉기를 덧씌웁니다. 앉기는 클립 없이 몸을 `CROUCH_DROP`만큼 내리고 다리를 2관절 IK로 바닥에 붙입니다. 머리 뼈가 히트박스 머리 시작(1.5m)에 있어 머리 크기 증강은 뼈 배율만 바꿉니다.
- 손은 주먹으로 무기를 쥡니다(1·3인칭 공통).
  - 오른손: 무기의 손잡이(모델 원점)를 주먹 구멍에 둡니다. 주먹 구멍은 holdRifle 자세에서 가운데·약지·새끼손가락이 감싸는 원의 중심이며, `tools/art/lib/character.mjs`가 재서 manifest `tp.position`으로 둡니다. 손잡이 축은 수직에서 앞으로 약 29° 기울어 권총 손잡이 각도와 맞습니다.
  - 나이프: 손잡이를 이 축에 맞춰 칼날이 엄지 쪽으로 나오게 만들었습니다(정방향 파지). 손목은 곧게 두고 칼끝 방향은 팔로 만듭니다. 1인칭은 칼을 아래팔 축(무기와 무관한 상수)에 맞춰 돌린 `fp.rotation`, 3인칭은 팔을 낮춘 준비 자세(`holds.knife`)입니다.
  - 손목 꺾임: `Rig.wristBend()`가 로드 시(곧은 손목) 대비 손이 아래팔 축에서 벗어난 각도를 잽니다(아래팔 둘레 비틀림 제외). `?art` 벤치와 `e2e/art.mjs`가 나이프 손목 20° 이하를 검사하고, 다른 무기 값은 INFO로 보여 줍니다.
  - 왼손: 받침점(`support`)에 주먹이 오도록 손 방향을 먼저 정하고, 그 주먹 위치가 나오는 손목으로 IK를 맞춥니다.
    - 손 방향은 두 가지입니다. 긴 총은 손바닥을 위로 해 핸드가드를 감싸고, 3인칭 권총류(`tp.supportGrip`)는 오른손 파지를 좌우로 뒤집어 오른 주먹 왼쪽에서 손잡이를 함께 쥡니다.
    - `?art` 벤치는 손가락 마디로 잰 손바닥 방향도 검사합니다. 주먹 위치는 IK가 맞춘 값을 다시 재는 것이라, 손 방향이 틀려도 통과하기 때문입니다.
- `?debug&hitbox`로 열면 원격 플레이어마다 서버 히트박스(앉기·머리 크기·방패 포함)를 선으로 그립니다.
- `?debug&collision`으로 열면 모든 MapBox 모서리를 한 `LineSegments`로 그립니다(보이는 상자는 노랑, INVISIBLE은 자홍). 보이는 모델과 충돌 박스가 맞는지 확인할 때 씁니다.
- 처치 배너(`hud.ts`의 `showKillBanner`)는 내 `KillEvent`마다 `game.ts`가 센 연속 처치 수와 헤드샷 여부로 그립니다. 탄환·해골은 코드로 그린 인라인 SVG이고, 등장·소멸은 CSS 애니메이션(`kb-life`, `kb-slam`, `kb-shake`)이며, 애니메이션이 끝나면 `animationend`로 숨깁니다.
- 이펙트는 파티클 알파·가산 배치, 예광탄 리본, 데칼 풀의 네 번으로 그립니다(예산은 manifest `budgets.drawCalls.effects`). 파티클이 가득 차면 수명이 가장 많이 지난 것을 바꿔 씁니다.
- 정적 맵은 그리기 호출 16회(보급창고)·12회(웨어하우스)입니다(예산 35). `InstancedMesh`는 인스턴스 하나라도 보이면 전부 그리므로, 정적 맵의 삼각형은 약 3만 개로 늘었습니다.
- `?debug` 쿼리로 열면 `window.__game`이 노출되어 e2e 테스트가 내부 상태를 읽습니다.
