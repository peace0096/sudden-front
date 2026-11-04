# 개발 가이드

## 준비물

- Rust 1.89 이상(`bevy_ecs` 0.18 요구), Node.js 22.12 이상(`@bufbuild/protoc-gen-es`, Vite 8 요구)
- Microsoft Edge (e2e 테스트용, playwright-core가 `msedge` 채널 사용)

## 빌드와 실행

```powershell
cd client; npm ci; npm run build           # proto 생성 → 에셋 검사 → 타입 검사 → client/dist
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
| art.mjs | `?art` 미리보기: 에셋 로드 실패·콘솔 에러·`[art]` 경고 0건, 서버 미접속. 캐릭터 벤치로 히트박스 일치(목 높이 서기 1.50·앉기 1.00, pitch ±1.45에서 목 위치, 머리 크기별 머리 꼭대기, 발 높이), 무기별 총구, 캐릭터당 그리기 호출 확인. 1인칭 벤치(무기별 대기·반동·재장전·볼트/펌프·칼질, 빨강/파랑)로 손과 IK 목표의 거리 1.5cm 이내, 대기 중 총구가 화면 안, 그리기 호출 5 이하, 팔 삼각형 예산 확인. 맵 벤치(모든 BoxKind·PropKind)로 키트 인스턴스와 충돌 박스의 1cm 이내 일치, 컨테이너 색, 텍스처 붙은 소품 판(셔터·광고판·창·환기창)이 원래 텍스처와 상관 0.8 이상, 정적 맵 그리기 호출 35 이하 확인. 이펙트 벤치로 모든 레시피 동시 재생 시 그리기 호출 예산, 풀 상한, 데칼이 박스 면(6방향)에 붙는지 확인 |
| perf.mjs | 맵별 봇 경기에서 draw call·삼각형·셰이더 프로그램·프레임 시간 측정(`e2e-shots/perf.json`). manifest 예산으로 판정: 경기 전체 평균 100·최대 130 이하, 정적 맵(`"world"` 그룹) 35 이하. 10초 연사 구간에서 이펙트 그리기 호출 예산과 지오메트리 수 고정(발사마다 할당 없음) 확인 |

Windows에서 서버를 띄우고 테스트하는 패턴:

```powershell
$s = Start-Process ..\server\target\release\sudden-front-server.exe -PassThru -WindowStyle Hidden
node e2e/augment.mjs "http://localhost:8800/?debug" e2e-shots
Stop-Process $s.Id
```

이전 서버 프로세스가 남아 있으면 exe가 잠겨 빌드가 실패하거나 옛 바이너리로 테스트하게 되므로 먼저 종료합니다.
