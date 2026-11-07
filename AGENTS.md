# AGENTS.md

Sudden Front는 웹 브라우저에서 하는 1인칭 FPS(서든어택 스타일)로, 5대5 팀 데스매치와 최대 10명 개인전이 있습니다. 이 파일은 프로젝트 요약과 작업 규칙을 담습니다. 무기·증강 수치표, 프로토콜 필드 번호 같은 세부 사항은 [docs/](docs/README.md)에 정리되어 있어 여기에 옮겨 적지 않습니다. docs와 코드가 다르면 코드가 기준입니다.

- **서버**: Rust, `bevy_ecs` 0.18(방마다 `World` 하나, 30Hz 틱), `axum` WebSocket, `prost`
- **클라이언트**: TypeScript(strict), three.js, Vite, protobuf-es. 캐릭터·총기(1·3인칭)·맵 소품·이펙트는 출처·라이선스를 기록한 스타일라이즈드 glTF/WebP 에셋(`client/public/art/`, 규칙 7)을 씁니다. 사운드는 코드로 합성하고, 벽·바닥·하늘·이름표 텍스처는 코드로 그립니다.
- **통신**: `proto/game.proto`(패키지 `game.v1`)의 메시지를 WebSocket `/ws` 바이너리 프레임 하나에 하나씩 전송
- **판정**: 서버 권위. 클라이언트는 이동 예측·보정, 원격 플레이어 보간, 반동, 렌더링·HUD·사운드를 담당
- **모드**: 일반모드(HP 100), 증강모드(HP 150, 시작 시 증강 카드 1장 선택), 왕대두모드(HP 100, 킬·데스로 레벨 −5~3과 머리 크기가 바뀌고 3레벨은 20초 무적의 왕대두)
- **대전 방식**: 팀 데스매치(5대5), 개인전(팀 없이 최대 10명). 모드와 따로 고르며 여섯 조합 모두 됩니다. 모드와 맵은 대기실에서 방장이 바꿀 수 있습니다(대전 방식은 방을 만들 때만)

## 빌드·실행·검증

Rust 1.89 이상과 Node.js 22.12 이상이 필요합니다(`bevy_ecs` 0.18, `@bufbuild/protoc-gen-es`와 Vite 8의 요구 버전). 각 명령은 적힌 폴더에서 실행합니다.

1. **클라이언트 빌드** — `client/`에서 `npm ci` 후 `npm run build`. proto 코드 생성 → 에셋 검사(`scripts/check-art.mjs`) → `tsc --noEmit` → `client/dist` 번들 순서로 돌며, 오류 없이 끝나면 완료입니다. 별도 lint 명령은 없으므로 TypeScript 변경은 이 명령으로 검사합니다. 의존성을 바꿀 때가 아니면 `npm install` 대신 `npm ci`를 씁니다. `npm install`은 npm 버전에 따라 `package-lock.json`을 고쳐 씁니다.
2. **서버 실행** — `server/`에서 `cargo run --release`. 프로젝트 폴더의 `./run.sh`는 클라이언트 빌드 → 서버 빌드 → 서버 시작을 한 번에 합니다. `Sudden Front server: http://localhost:<포트>`가 출력되면 준비된 것입니다. 서버가 `/ws`와 `client/dist` 정적 파일을 함께 제공하며, 정적 파일 경로는 `CLIENT_DIST`로 바꿉니다.
   - 포트: `PORT`가 없으면 8800, 사용 중이면 8801~8819 중 빈 포트로 열고 준비 줄 앞에 알려 줍니다. `PORT`를 지정하면 그 포트만 쓰고, 사용 중이면 한 줄 오류로 종료합니다(exit 1). 여러 서버를 띄우는 검증에서는 `PORT`를 직접 지정해 포트를 확정합니다.
   - 클라이언트를 고치는 중에는 서버를 켠 채 `client/`에서 `npm run dev`로 http://localhost:5173 을 씁니다. Vite가 `/ws`를 `PORT`(없으면 8800)로 프록시하므로, 서버가 다른 포트로 열렸으면 같은 `PORT`를 주고 실행합니다.
3. **서버 테스트와 검사** — `server/`에서 `cargo test --release`, `cargo fmt`, `cargo clippy --all-targets -- -D warnings`. 테스트가 모두 통과하고 clippy 경고가 0이면 완료입니다. 테스트는 그 테스트가 검증하는 로직이 있는 파일 맨 아래 `tests` 모듈에 있으므로, 규칙을 바꾸면 그 파일의 테스트를 추가하거나 고칩니다. 서버 코드 규칙은 [server/AGENTS.md](server/AGENTS.md)에 있습니다(규칙 12).
4. **브라우저 e2e** — release 서버를 켠 상태에서 `client/`에서 `node e2e/<스크립트>.mjs [URL] [스크린샷 폴더]`(URL을 생략하면 `client/e2e/server.mjs`가 `PORT`(없으면 8800)로 만든 주소, 스크린샷 폴더 기본값은 `e2e-shots`). 종료 코드 0이면 통과이며, `SKIP`으로 출력된 항목은 검증되지 않은 것입니다. 스크립트별 검증 범위는 [docs/development.md](docs/development.md)에 있습니다.
   - Microsoft Edge가 필요합니다. `playwright-core`가 `msedge` 채널의 headless Edge와 SwiftShader로 렌더링합니다. Edge가 없으면 스크립트를 `client/e2e/` 안에 임시 사본으로 복사해 `channel`만 `"chrome"` 등 설치된 브라우저로 바꿔 실행하고, 끝나면 사본을 지웁니다. 사본이 `client/e2e/` 안에 있어야 `playwright-core`를 찾습니다.
   - 기본 URL은 Rust 서버가 제공하는 `client/dist`를 엽니다. 클라이언트를 고쳤으면 `npm run build`를 먼저 해야 새 코드가 테스트됩니다.
   - 이전 서버 프로세스를 종료하고 새로 빌드한 서버를 띄웁니다. 남아 있으면 옛 바이너리로 테스트하게 되고, Windows에서는 exe가 잠겨 빌드가 실패합니다.
5. **부하 테스트와 모니터링** — `tests/client/`에서 `cargo run --release -- --count <봇 수> [--duration <초>]`(봇이 브라우저처럼 접속해 10명씩 방을 만들고 경기, `--duration`이 없으면 Ctrl+C까지). 검사는 그 폴더에서 `cargo fmt`·`cargo clippy --all-targets -- -D warnings`·`cargo test --release`(서버와 같은 lint). 서버·Prometheus·Grafana는 프로젝트 폴더에서 `docker compose -f infra/docker-compose.yml up -d --build`(게임 8800, Grafana 3000, Prometheus 9090). 서버 지표는 `/metrics`. 자세한 내용은 [docs/load-test.md](docs/load-test.md)에 있습니다.

## 작업 규칙

1. **판정 규칙과 수치의 원본은 서버입니다.** 무기 스탯, 증강 효과, HP, 각종 시간은 `server/src/game/`에서 정하고, 클라이언트는 `MatchStart`의 `GameConfig`로 받은 값을 씁니다. 이유: 판정을 일관되게 하고 치트를 막습니다. 다만 클라이언트에는 예측·표시용으로 서버 값을 손으로 옮긴 사본이 있습니다: `client/src/game/game.ts`의 `MAX_PITCH`·`BLOOM_DECAY`·`BLOOM_HOLD_EXTRA`·`VEST_MAX_HP`·`BURST_*`와 `reloadTime()`의 장전 배율, `movement.ts`의 `CMD_DT`·`CONTACT_EPS`·`FOOTING`, `client/src/game/art/hitbox.ts`의 히트박스·`CROUCH_DROP`·방패 크기·`HEAD_SCALE_MAX`(= `augments.rs`의 `BIG_HEAD_MAX`, 캐릭터 경계용), `lobby.ts`의 `TEAM_SIZE`·`KILL_TARGETS`, `augments.ts`의 증강 등급과 설명 속 수치, `world.ts`의 `TILTED_HEIGHT`(= `map.rs`의 `CONTAINER_H`, 기울어진 컨테이너의 충돌 조각 높이), `weapons.ts`·`hud.ts`의 수류탄 설명 속 수치(개수·3초·반경 5 m), `hud.ts`의 왕대두모드 Tab 규칙 문구 속 1레벨(`big_head.rs`의 `COMEBACK_LEVEL`)·0레벨(`KING_END_LEVEL`). 서버의 해당 값을 바꾸면 같은 변경에서 사본도 고칩니다. 이유: 사본이 어긋나도 컴파일 오류가 나지 않고, 예측·HUD만 서버 판정과 달라집니다.
2. **이동 규칙은 서버와 클라이언트를 함께 고칩니다.** `server/src/game/physics.rs`의 `step_move`·`step_vertical`(점프·중력·박스 윗면 착지), `systems/input.rs`의 `walk_step`·`consume_inputs`, `client/src/game/movement.ts`는 규칙과 상수(`CONTACT_EPS`·`FOOTING` 등, 점프 속도·중력은 `GameConfig`로 받음)가 같아야 합니다. 이유: 클라이언트 예측이 서버와 어긋나면 보정할 때 위치가 튑니다. 확인: `e2e/wall-rub.mjs`, `e2e/play.mjs`(예측 오차).
3. **프로토콜은 `proto/game.proto`에서만 바꿉니다.** 서버는 `cargo build`가(`build.rs`, protoc 내장이라 따로 설치할 필요 없음), 클라이언트는 `npm run gen`(`dev`·`build`가 자동 실행)이 코드를 다시 만듭니다. 생성물(`client/src/gen/`, 서버 `OUT_DIR`)은 손대지 않습니다. 지운 필드 번호는 `reserved`로 남깁니다(예: `Snapshot`의 `reserved 14`). 이유: 옛 클라이언트와 번호가 충돌하지 않게 합니다.
4. **봇의 이동·조준·사격·장전은 사람과 같은 `InputCmd` 큐로 넣습니다.** `server/src/game/systems/bot.rs`의 `bot_think`가 명령을 만들어 넣으면 사람과 같은 시스템이 판정합니다. 새 봇 행동도 입력 생성으로 구현합니다. 이유: 규칙 구현을 하나로 유지합니다. 무기 관리만 예외로 직접 처리합니다: 탄이 떨어진 테이저를 1번 무기로 바꾸기와 예비 탄약 보충(`systems/bot.rs`), 증강 무기가 2번 칸이면 그 무기를 든 채 시작(`systems/weapon.rs`의 `issue_weapons`), 재투입 때 주무기 재추첨(`systems/respawn.rs`의 `respawn_players`), 우지 아킴보 봇의 기관단총 고정(`mod.rs`).
5. **반동은 클라이언트, 탄퍼짐은 서버가 정합니다.** 클라이언트(`client/src/game/game.ts`)가 반동을 더한 조준각을 `InputCmd`의 yaw/pitch로 보내고, 서버가 무작위 탄퍼짐을 더해 판정합니다. 서버는 한 틱 안의 **첫 번째** 사격 명령의 조준각을 씁니다. 이유: 화면과 판정이 일치해야 하고, 마지막 명령을 쓰면 반동이 이미 더해진 각도로 첫 발이 위로 튑니다.
6. **증강 효과는 `Perks.active`로 판정합니다.** `Perks.augment`는 고른 카드(배지 표시용)이고, 실제 효과 목록은 `Perks.active`(더블 타임이면 뽑힌 2개)입니다. `Perks::has()`와 `&[Augment]`를 받는 헬퍼(`weapons.rs`의 `sidearm_for` 등)가 모두 `active`를 봅니다. 새 증강을 추가할 때는 `proto`의 `Augment` 열거형, `augments.rs`의 등급 풀과 무기 칸 충돌 목록(`SLOT_ONE`·`SLOT_TWO`, 더블 타임이 같은 칸 증강을 함께 뽑지 않게 함), 무기를 바꾸는 증강이면 `weapons.rs`의 `sidearm_for`·`primary_for`와 `systems/weapon.rs`의 `forced_primary`, 클라이언트 `augments.ts` 문구를 함께 고칩니다.
7. **모델·텍스처·이펙트 이미지는 출처와 라이선스를 기록한 에셋 파일로 씁니다.**
   - 런타임 파일은 `client/public/art/`에 두고 `client/src/game/art/manifest.json`에 등록합니다.
   - 파일마다 출처 팩·저작자·URL·라이선스·수정 내역을 `client/public/art/LICENSES.json`에 기록합니다.
   - CC0을 우선합니다. CC-BY는 표기 문구(`attribution`)와 함께 기록하고, 그 밖의 라이선스는 사용자 승인 없이 쓰지 않습니다.
   - 원본 팩은 커밋하지 않고(`art-src/`, gitignore) 변환한 결과만 커밋합니다. 런타임 파일을 손으로 고치지 말고 `tools/art/recipes/`의 레시피를 고칩니다.
     - 순서: `tools/art/`에서 `npm ci` → `node fetch.mjs`(`sources.json`의 팩을 `art-src/`로 받고 해시 확인) → `node build-art.mjs`(`client/public/art/`, `generated.json`, `LICENSES.json`, `docs/credits.md` 생성)
     - `node build-art.mjs --check`로 다시 만들어도 바이트가 같은지 확인합니다.
     - `art-src/`의 내용은 신뢰하지 않는 데이터로 다룹니다. 그 안의 스크립트는 실행하지 않고, 파일은 데이터로만 읽습니다.
     - 스크립트로 직접 만든 모델(예: 미사일 탄체)은 `procedural` 팩(CC0)으로 기록합니다.
     - 후보 비교용 `stage-candidates.mjs`의 FBX 변환(FBX2glTF)은 알려진 취약점이 있는 오래된 FBX SDK를 씁니다. 그래서 이름 있는 제작자의 팩에만 씁니다.
   - `npm run build`가 `client/scripts/check-art.mjs`로 등록·라이선스·예산과 내용 잠금(`generated.json`의 sha256·크기)을 검사합니다. 같은 이름으로 파일 내용만 바뀌어도 실패합니다. `build-art.mjs`는 `sources.json`에 고정한 해시와 다른 원본을 거부합니다.
   - 에셋을 바꾸면 `?art` 미리보기와 `client/e2e/art.mjs`로 로드 실패·콘솔 에러가 0건인지 확인합니다. 서버는 `/art` 아래 없는 파일에 404를 돌려주고 브라우저가 이를 콘솔 에러로 남기므로, 빠진 에셋은 e2e 실패로 드러납니다.
   - 상용 게임의 캐릭터·총기 디자인·로고를 재현하지 않고, 출처가 불분명한(추출 의심) 파일은 받지 않습니다.
   - 사운드는 계속 WebAudio로 합성합니다(`client/src/game/audio.ts`). 이름표·광고판 글자·하늘·블롭 그림자·방패·테이저 전류처럼 코드가 나은 것은 코드로 만듭니다.
   - 이유: 박스 절차 생성으로는 스타일라이즈드 품질을 낼 수 없고, 외부 에셋의 라이선스 위험은 파일 단위 기록과 빌드 검사로 관리합니다.
8. **`Game`의 필드·메서드 이름을 바꾸면 e2e도 고칩니다.** `?debug`로 열면 `client/src/main.ts`가 `Game` 인스턴스를 `window.__game`으로 노출합니다. `client/e2e/*.mjs`는 `pos`·`lastSnapshot`·`locked` 같은 private 필드와 `stats()` 같은 메서드를 직접 쓰고, `augment.mjs`·`wall-rub.mjs`는 필드에 값을 쓰거나 메서드를 바꿔 끼웁니다. 다음 이름도 e2e가 직접 부릅니다.
   - 캐릭터: `Character.update`(인자 10개, 순서 고정)·`setLook`·`setWeapon`, `name` 스프라이트의 `visible`
   - 1인칭: `viewModel.setWeapon`·`setShield`
   - 그 밖: `effects.explosion`, `game.syncMissiles`·`onShot`·`tryLocalFire`·`hud.crosshair`, `Game.perf()`
   - 개인전: `ffa.mjs`는 `ffa`·`isAlly()`·`config.matchType`·`roster`·`remotes`(와 그 `character.made.file`)·`viewModel.arms.file`·`myTeam`·`matchOver`·`hasFirstSnapshot`·`connection`을 쓰고, 인스턴스의 `onMatchEnd`를 감싸 받은 `MatchEnd`를 기록합니다.
   - 수류탄: `grenade.mjs`는 `weaponKind`·`grenadesLeft`·`grenades`(날아가는 수류탄 맵)·`pos`·`yaw`·`pitch`·`lastSnapshot`(`grenades`·`grenadesLeft`)을 쓰고, `effects.explosion`과 인스턴스의 `onSnapshot`을 감싸 폭발과 마지막 수류탄 위치를 기록합니다. 무기 칸 표시(`.slot-bar`, `.slot-chip.spent`)와 탄약 칸(`.ammo-count .mag`·`.reserve`)도 읽습니다.
   - 처치 배너: `kill-banner.mjs`는 `onKill`·`lastKillAt`·`roster`·`myId`를 쓰고, `.kill-banner`(와 `hidden`·`headshot-kill` 클래스)·`.kb-title`·`.kb-sub`·`.kb-bullet`·`.kb-skull`·`.kb-victim-name`(팀 색 클래스)·`.kb-victim-label`을 읽습니다.
   - 왕대두모드: `big-head.mjs`는 `config.mode`·`minLevel`·`maxLevel`·`kingTime`, `lastSnapshot`의 `players`(`level`·`headScale`·`kingLeft`·`weapon`), `roster`·`myId`·`remotes`(와 그 `entry`·`state`·`shownX`·`shownZ`, `character.headScale`·`crown`), `pos`·`yaw`·`pitch`를 쓰고, `onKill`을 인자 5개(마지막이 `SpecialKill`)로 부르며 인스턴스의 `onSnapshot`을 감싸 서버의 특수 샷을 기록합니다. 화면에서는 `.level-panel`·`.level-value`·`.level-pips .pip`(`on`, `data-level`), 처치 배너의 `.kb-special`과 `dot-shot`·`cute-shot` 클래스, 킬 피드의 `.special`·`.headshot`, `.slot-chip.king`, `.big-head-notice`를 읽고, 방 만들기 폼의 `.mode-select`에서 "왕대두모드"를 고릅니다.
   - 대기실 설정: `kill-target.mjs`는 방장의 `.room-mode-select`·`.room-map-select`·`.kill-target-select`를 고르고, 경기 시작 뒤 `config.mode`·`mapId`를 읽습니다. 방 만들기 폼의 `.mode-select`·`.map-select`와는 다른 이름이어야 합니다(다른 e2e가 방 만들기 전에 그쪽을 고름).
   - `?art` 페이지의 벤치 결과(`window.__artReport`, `Character.measure()`)는 `client/e2e/art.mjs`가 읽습니다.
   - `world.ts`가 붙이는 이름도 e2e가 씁니다. 장면의 `"world"` 그룹은 `perf.mjs`가 정적 맵 그리기 호출을 잴 때 찾고, 메시 이름(BoxKind·PropKind 이름, 키트는 `<파일>#<메시>`)은 `?art` 맵 벤치가 찾습니다.
   - 이펙트: `perf.mjs`가 `effects.measure()`와 장면의 `"effects"` 그룹으로 이펙트 그리기 호출을 잽니다.
   - `perf.mjs`는 이 밖에도 `Game`의 `scene`·`renderer`·`camera`, `viewModel.scene`·`camera`, `config.weapons`, `weaponKind`, `trigger`, `myId`를 직접 씁니다.
   - Esc 화면(`hud.ts`의 `.pause`): e2e는 그 가운데를 눌러 경기를 재개하고, `#ui-root .pause button`(첫 버튼)을 방 나가기로 누릅니다. 그래서 감도 컨트롤(`.pause-settings`)은 나가기 버튼 뒤에 붙이고 화면 아래쪽에 둡니다. `play.mjs`는 `.pause .sensitivity`의 숫자 칸과 버튼도 씁니다.
   - 확인: 바꾼 이름을 `client/e2e/`에서 검색합니다.
9. **수치나 규칙을 바꾸면 같은 변경에서 문서도 고칩니다.** 무기·조작·맵·봇 → `docs/gameplay.md`, 증강 → `docs/augment-mode.md`, 프로토콜 → `docs/protocol.md`, 구조·동기화 → `docs/architecture.md`, 빌드·테스트·수치 위치 → `docs/development.md`, 설계 결정·알려진 이슈 → `docs/history.md`, 조작·무기·증강 표 → 루트 `README.md`. 이유: 수치표와 설명이 문서에 따로 있어 코드만 고치면 문서가 어긋납니다.
10. **문서는 한국어, 코드 주석·식별자·커밋 메시지는 영어로 씁니다.** 기존 코드와 커밋 이력이 이 방식을 따릅니다.
11. **작업을 하나 마칠 때마다 커밋하고, 푸시는 개발자가 허용할 때만 합니다.** 작업 하나(보통 사용자 요청 하나)를 끝내면 바로 커밋합니다. 작업 트리와 스테이징 영역에는 개발자나 다른 세션이 남긴 커밋하지 않은 변경이 있을 수 있으므로, 이번 작업의 파일만 경로를 지정해 커밋하고(`git add -A`·`git add .`·`git commit -a` 대신) 같은 파일 안의 이전 변경도 빼고 이번 작업 부분만 넣습니다. 이전 변경과 같은 줄을 고쳐 나눌 수 없으면 그 파일은 커밋하지 않고 개발자에게 알립니다. `git push`는 자동으로 하지 않고 개발자가 허용할 때만 하며, 허용은 그 한 번의 푸시에만 적용합니다. 이유: 작업 단위로 히스토리를 남겨 무엇이 바뀌었는지 추적하고 작업 하나만 되돌릴 수 있게 합니다. 푸시는 원격 저장소에 올라가 되돌리기 어려우므로 개발자가 정합니다. 확인: 커밋 뒤 `git show HEAD`에 이번 작업의 변경만 들어 있습니다.
12. **서버 코드는 [server/AGENTS.md](server/AGENTS.md)의 규칙을 따릅니다.** 다음 다섯 가지입니다: 도메인별 패키지와 도메인 이름의 파일, 순수 함수와 같은 파일 맨 아래의 단위 테스트, `unsafe` 금지, 넘칠 수 있는 정수 연산의 처리, 작업을 마칠 때 `cargo fmt`·`cargo clippy`. 이유: 서버 코드에만 해당하는 규칙을 한곳에 모아 두면, 서버 파일을 다룰 때 함께 읽힙니다. 확인: `unsafe`와 정수 연산은 `cargo clippy --all-targets -- -D warnings`가 막습니다. Claude Code에서는 Stop 훅(`.claude/hooks/server-check.sh`)이 작업을 마칠 때 fmt와 clippy를 돌리고, 실패하면 종료를 한 번 막습니다.

## 구조

```
proto/game.proto           패킷 정의의 단일 원본
server/
  AGENTS.md                서버 코드 규칙(도메인별 패키지, 순수 함수와 단위 테스트, unsafe 금지, 정수 오버플로, fmt·clippy)
  Cargo.toml               [lints]: unsafe_code forbid, clippy::arithmetic_side_effects
server/src/                테스트는 각 파일 맨 아래 tests 모듈
  main.rs                  axum 라우터: /ws, /metrics, /art(없는 파일은 404), client/dist 정적 파일
  net.rs                   연결별 WebSocket 세션 ↔ 허브 스레드(mpsc). 16KB 초과 프레임은 연결 종료, 디코딩 실패 프레임은 버림
  metrics.rs               Prometheus 지표(틱 시간·FPS, 연결·방·인원, ECS World 크기, WebSocket 트래픽, Linux 프로세스)
  hub/                     로비와 방, 30Hz 틱 루프
    mod.rs                 허브: 메시지 처리, 방장 권한, 방 목록·대기실 상태 전송, 틱 루프
    seats.rs               좌석(팀별 5칸, 개인전은 같은 10칸을 번호 0~9로)과 칸 배정
    room.rs                방: 설정·멤버, 신규·도중 참가 좌석 선택
  game/
    mod.rs                 Match(World + Schedule), 증강 적용, 선택·대기 단계, 틱·HP·부활 상수
    snapshot.rs            명단·시작·스냅샷·종료 메시지
    systems/               틱 규칙 시스템, 도메인마다 한 파일(판정 상수는 각 파일 상단). mod.rs의 schedule()이 실행 순서를 정함
      input.rs, weapon.rs      입력(명령 예산·시선·이동), 무기 관리(쿨다운·블룸·장전·스코프·무기 지급)
      fire.rs, blast.rs        사격·근접·방패 판정과 피해·처치, 폭발 피해
      missile.rs, grenade.rs   미사일 비행, 수류탄 던지기·튕김·퓨즈
      perk.rs, king.rs         휴식 회복, 왕대두 시간
      respawn.rs, history.rs   부활과 스폰, lag compensation용 위치 기록
      tick.rs                  경기 틱
      bot.rs                   봇 AI. 상단에 반응·조준 상수
    components/            ECS 컴포넌트(player, weapon, perks, input, history, bot)
    resources/             ECS 리소스(arena, rules, match_state, projectile, event_log, rng)
    physics.rs             step_move(축별 미끄러짐 충돌), 레이·시야 판정
    nav.rs                 봇용 보행 격자와 BFS 경로 탐색
    weapons.rs             무기 스탯, 공용 계수, 모드별 대미지, 증강별 무기 지급
    augments.rs            증강 등급 풀·확률, 카드 배분, 더블 타임, 선택·대기 시간, 증강 수치
    big_head.rs            왕대두모드: 레벨 범위·레벨별 머리 배율·왕대두 시간 상수, 레벨 증감, 즉위(crown)
    map.rs                 맵 데이터(점대칭 생성), 팀 스폰과 개인전 스폰(ffa_spawns), 컨테이너 계단(WAREHOUSE_CLIMBS), 기울어진 컨테이너 충돌 조각(lean_slices)
    test_support.rs        시나리오 테스트 공용 도우미(#[cfg(test)])
client/src/
  main.ts, net.ts, lobby.ts   진입점·화면 전환·에셋 프리로드와 시작 대기·?art 미리보기, WebSocket encode/decode, 로그인·방 목록·대기실
  settings.ts                 개인 설정(마우스 감도 배율, localStorage). 로비와 게임 Esc 화면이 같은 슬라이더를 씀
  weapons.ts, augments.ts     무기 설명·B키 메뉴 카드, 증강 카드 문구·등급
  game/game.ts             게임 루프, 입력, 예측·보정, 반동, 스냅샷 반영, HUD·증강 카드 연결
  game/movement.ts         이동 예측(서버 step_move와 같은 규칙)
  game/*.ts                world(맵: 컨테이너·상자·드럼통·가로등은 키트 모델 InstancedMesh, 벽·지붕은 병합 박스와 코드 텍스처, ?debug&collision), character(원격 캐릭터: glTF SWAT, 애니메이션 믹서 + art/rig.ts의 조준·자세·앉기 IK), viewmodel(1인칭: SWAT 팔 삼각형 일부 + glTF 총, 주먹이 손잡이·받침점을 감싸도록 손 방향을 정하고 rig.ts IK로 손목을 맞춤, setTeam으로 소매 색), effects(particles.ts 인스턴스 파티클·예광탄 리본, decals.ts 탄흔·핏자국·그을음 풀. 합계 그리기 호출 4 이하), hud(HUD·점수판·증강 카드·B키 무기 메뉴), audio(무기별 발사음을 미리 렌더링해 재생), textures
  game/art/                에셋 manifest(단일 원본)·로더(loadArt, releaseGpu)·Lambert 변환·대체 모형·히트박스 사본(hitbox.ts)·절차적 뼈 레이어(rig.ts)·?art 미리보기와 벤치
  game/perf.ts             draw call·프레임 시간 측정(Game.perf(), ?debug&perf 오버레이)
  gen/game_pb.ts           npm run gen 생성물(gitignore)
client/public/art/         glTF·WebP 에셋과 LICENSES.json(파일별 출처·라이선스)
client/scripts/check-art.mjs  빌드 때 에셋 등록·라이선스·예산 검사
tools/art/                 에셋 파이프라인: sources.json·fetch(원본 팩 받기)·inspect(측정)·build-art와 recipes(변환, --check)·gallery-shots(비교 스크린샷)
client/e2e/*.mjs           Playwright 브라우저 테스트
tests/client/              부하 테스트 봇(Rust, 서버와 따로 빌드): bonsai-bt 행동 트리(tree.rs)·칠판과 잎 노드(agent.rs)·연결 루프(bot.rs)·10명 단위 방 배정(plan.rs)·서버 봇을 옮긴 전투(combat/)
infra/                     docker-compose.yml(서버·Prometheus·Grafana·cAdvisor), docker/server.Dockerfile, prometheus/·grafana/ 설정과 대시보드, pulumi/(Google Cloud VM, TypeScript)
```

수치를 바꿀 위치 전체 표는 [docs/development.md](docs/development.md)의 "수치 조정 위치"에 있습니다.

## 서버 틱과 네트워크 동기화

- **스레드**: tokio가 WebSocket 연결을 받고, `game-hub` 스레드 하나가 모든 방을 소유해 30Hz로 진행합니다. 방 사이 공유 상태가 없어 락을 쓰지 않습니다. 클라이언트별 송신 큐(512)가 차면 그 연결을 끊습니다.
- **방 상태**: 대기실 → 진행 → 결과(8초) → 대기실. 최대 10명, 팀당 5명(개인전은 같은 10칸을 번호 0~9로 씀).
- **틱 순서**(`server/src/game/systems/mod.rs`의 `schedule()`, chained): `bot_think → consume_inputs → update_weapons → fire_weapons → update_perks → update_kings → respawn_players → record_history → advance_tick`. 증강모드의 선택(15초)·대기(5초) 동안은 스케줄 대신 `freeze_tick`만 돌고, 그 틱 수(`frozen_ticks`)는 경기 시계에서 뺍니다.
- **입력**: 클라이언트는 60Hz 고정 스텝으로 `InputCmd`(seq, 이동키, yaw/pitch, 사격, 장전, 스코프, 무기 칸, 앉기, `render_tick`)를 만들어 `InputBatch`로 보냅니다. 서버는 틱마다 명령 2개분의 예산을 더해(최대 8) 그만큼만 적용하며, 더 빠른 입력은 스피드핵으로 봅니다.
- **예측·보정**: 클라이언트는 입력을 즉시 자기 위치에 적용하고, 스냅샷의 `last_input_seq` 이후 명령을 다시 적용(replay)해 서버 위치에 맞춥니다.
- **Lag compensation**: 클라이언트는 원격 플레이어를 보간해 그리며 그 서버 틱을 `render_tick`으로 보냅니다. 서버는 사격 시 표적을 그 틱 위치(`History`, 최대 15틱 = 0.5초)로 되돌려 판정합니다. 미사일은 실제 투사체라 대상이 아닙니다.
- **스냅샷**: 매 틱 모든 플레이어 상태와 그 틱의 이벤트(사격·대미지·킬·장전·부활·폭발)를 보냅니다. 탄약·장전·무기·증강 카드는 수신자별 필드, 점수·남은 시간·경기 시계·미사일은 공용 필드입니다.
- 메시지 흐름과 필드 번호: [docs/protocol.md](docs/protocol.md)

## 게임 규칙 요약

- 5대5 팀 데스매치. 방장이 대기실에서 고른 목표 킬(10/20/30/50, 기본 30)을 먼저 채운 팀이 이깁니다. 아군 사격과 자기 피해는 없습니다.
- 개인전: 팀 없이 최대 10명이 싸우고, 목표 킬(같은 선택지, 기본 20)을 먼저 채운 사람이 이깁니다.
  - 선수의 팀은 `UNSPECIFIED`이고, 피해 판정은 서버 `Rules::hostile()`, 아군 판단은 클라이언트 `Game.isAlly()` 한 곳에서 합니다. 팀 0을 그냥 두면 모두 아군·파랑으로 처리되기 때문입니다.
  - 대기실은 10칸 한 블록(팀 변경 없음, 2명 이상이면 시작)이고, 스폰은 맵별로 고른 `ffa_spawns` 중 다른 생존자에게서 가장 먼 곳입니다.
  - 모두 회색 중립 군복(`characters/swat-neutral.glb`)을 입고 이름표는 빨강입니다. 혼자 남으면 기권승입니다.
- 사망 3초 후 탄약을 채워 재투입되고 3초간 보호됩니다(먼저 쏘면 해제).
- 대기실은 팀마다 위치가 고정된 5칸(사람·봇·빈칸)이고, 방장이 빈칸마다 봇을 넣거나 뺍니다. 방 생성 때 "빈 자리는 봇으로 채우기"를 켜면 방장 외 모든 칸이 봇으로 시작합니다. 봇 칸만 남은 팀에 사람이 입장하면 봇이 빠지고, 팀을 바꿔 들어오면 그 봇이 옮긴 사람의 원래 칸으로 갑니다. 양 팀에 사람이나 봇이 1명 이상 있어야 시작합니다.
- 방장 외 모든 플레이어가 준비(`SetReady`)해야 방장이 시작할 수 있고(서버 `on_start_game`이 검사, 클라이언트는 시작 버튼을 끔), 경기가 시작되면 준비가 풀립니다. 손님이 있는 e2e는 시작 전에 손님이 `.actions .ready-btn`을 누릅니다.
- 경기 중 나간 자리는 비워 두고, 한 팀에 아무도 남지 않으면 남은 팀이 기권승합니다. 시간 제한이 없어 상대가 없으면 경기가 끝나지 않기 때문입니다.
- 경기 중에도 빈 자리로 도중 참가할 수 있습니다(`hub/mod.rs`의 `join_running_match` → `Match::add_player`). 봇 칸은 경기 중에 양보하지 않고, 결과 화면 중에는 거절합니다.
- 맵: 보급창고(야외 100×64m), 웨어하우스(실내 컨테이너 창고, 단층 컨테이너마다 상자 계단). `map.rs`가 점대칭으로 만들어 `MatchStart`로 보냅니다. 기울어진 컨테이너는 서버가 기울기(`MapProp.tilt`)를 보내고, 충돌은 그 기울기를 따라 자른 INVISIBLE 조각 상자(`lean_slices`)입니다.
- 무기 칸: 1 주무기(소총 M416·저격소총 AWP·기관총 M249·기관단총 UZI 중 게임 중 B키로 선택, 다음 경기에도 유지), 2 권총, 3 나이프, 4 수류탄(목숨당 1개, 던지면 3초 뒤 폭발하고 그 전 칸으로 복귀, 남은 개수는 `Weapon.holstered[3]`). 바꾸면 0.35초 꺼내는 시간이 있고 진행 중인 장전은 취소되며, 탄약은 무기별로 유지됩니다.
- 무기 표의 이동속도·탄퍼짐에는 공용 계수 `SPEED_SCALE`(0.9)과 `SPREAD_SCALE`(0.75, 샷건 제외)이 곱해집니다. 무기별 상대 밸런스를 유지한 채 전체 체감을 바꿀 때 이 계수를 조정합니다.
- 증강모드: 카드 3장 중 1장을 15초 안에 고르고(카드마다 1회 교체, 이미 나온 증강은 다시 안 나옴, 안 고르면 무작위, 봇도 무작위) 5초 대기 후 전투합니다. 등급 확률은 실버 40% / 골드 35% / 프리즘 25%이고 증강은 19종이며, 고른 증강은 부활해도 유지됩니다. 증강 전용 무기는 샷건, 권총+, 매그넘, 테이저, 미사일 런처, 우지 아킴보입니다.
- 왕대두모드: 처치 +1레벨·사망 −1레벨(−5~3, 마이너스에서 처치하면 바로 1레벨), 레벨별 머리 배율을 모델과 머리 히트박스에 적용합니다(`Perks.level`·`head_scale`). 3레벨이 되면 20초 동안 무적(맞힘 판정은 하되 피해 없음)·HP 회복·뿅망치(`WeaponKind.HAMMER`, 한 방, 이동속도 5.94)만 사용하는 왕대두가 되고(`Perks.king_ticks`, 뿅망치를 든 동안 `Weapon::switch_to`가 거부), 끝나면 0레벨과 새 무기로 돌아갑니다(`update_kings`). 머리가 가장 작은(마이너스, 경기 안 최저 레벨) 상대를 헤드샷으로 처치하면 도트 샷, 뿅망치로 처치하면 큐트 샷(`KillEvent.special`)입니다.
- 세부 수치와 증강 효과: [docs/gameplay.md](docs/gameplay.md), [docs/augment-mode.md](docs/augment-mode.md), [docs/big-head-mode.md](docs/big-head-mode.md)

## 알려진 이슈

- 증강·무기 밸런스는 실제 대전으로 검증하지 않았습니다.
- `e2e/play.mjs`의 "정지 상태 예측 오차" 체크가 가끔 실패합니다(0.13m, 개인전 작업 중 0.078m). 둘 다 재실행하면 0.0000m로 통과했고 원인은 미확인입니다. 실패하면 재실행해 재현 여부부터 확인합니다.
- 미사일·테이저는 서버 단위 테스트와 스크린샷으로만 확인했고, 브라우저에서 실제로 쏴서 맞히는 e2e는 없습니다. 수류탄 e2e(`grenade.mjs`)도 던지기·폭발 시점까지만 보고, 피해는 서버 단위 테스트로 확인합니다.
- 미사일은 lag compensation이 없어 빠르게 움직이는 표적은 화면과 폭발 위치가 어긋날 수 있습니다.
- Ctrl 앉기 중 W를 누르면 브라우저 탭 닫기(Ctrl+W)와 겹칠 수 있으나 사람이 확인하지 않았습니다. C 키 앉기가 대안입니다.
- 루트 `README.md` 무기 표의 이동속도·탄퍼짐은 공용 계수 적용 전 값입니다. 실제 값은 `server/src/game/weapons.rs`가 기준입니다.
- 성능은 SwiftShader 기준선(docs/development.md)만 자동으로 잽니다. 내장 그래픽 PC에서의 실제 프레임은 `?debug&perf` 오버레이로 사람이 확인해야 합니다. 스킨드 캐릭터로 바꾼 뒤 SwiftShader의 평균 프레임은 20~40% 느려졌습니다(그리기 호출은 35~43% 감소).
- 캐릭터 모델의 팔이 짧아 3인칭 긴 총의 앞손은 핸드가드까지 닿지 못하고 탄창 우물·리시버 앞을 쥐며, 소총 자세는 몸을 40°쯤 비튼 자세입니다. 앉으면 무릎이 다리 히트박스 밖으로 0.1~0.2m 나옵니다(보이는 것만).
- 총 모델은 탄창·노리쇠가 분리되지 않은 단일 메시라, 1인칭 재장전·볼트 동작은 손과 총의 숙임·기울임으로만 보여 줍니다.
- 1인칭 총을 쥔 오른손목은 약 64° 꺾여 있습니다(아킴보 71°, 3인칭 총 자세의 오른손목은 16~30°). 1인칭 아래팔이 무기와 무관하게 47° 올라오는데 총을 쥔 손은 수평이기 때문입니다. 왼손목도 1인칭 받침손 25°(재장전 중 52°)·아킴보 94.5°, 3인칭 받침손 46~75°로 꺾여 있습니다. 나이프만 손목을 곧게 맞췄고, 총은 `art.mjs`가 INFO로만 출력합니다.
- 가산 합성 이펙트(총구 화염 등)는 밝은 하늘 앞에서 거의 흰색으로 보입니다. SwiftShader처럼 프레임이 아주 낮으면(3~5fps) 45~100ms짜리 짧은 이펙트는 그려지지 않을 수 있습니다.
