# server/AGENTS.md

서버(`server/`) 코드를 고칠 때 따르는 규칙입니다. 저장소 전체 규칙은 루트 [AGENTS.md](../AGENTS.md)에 있고, 여기서는 서버 코드에만 해당하는 것을 다룹니다. 검사로 강제하는 규칙은 어기면 컴파일이나 `cargo clippy`가 실패하고, 나머지는 리뷰로 지킵니다.

## 규칙

1. **코드는 도메인별 패키지에 두고, 파일 이름은 도메인 이름으로 짓습니다.** `systems.rs`·`components.rs`처럼 종류 이름 아래 여러 도메인을 모은 파일은 만들지 않습니다.
   - 틱 시스템은 `src/game/systems/<도메인>.rs`에 하나씩 둡니다. 실행 순서는 `src/game/systems/mod.rs`의 `schedule()`에서만 정합니다.
     - 예: 사격은 `systems/fire.rs`입니다.
     - 새 시스템을 만들면 그 체인에 넣습니다.
   - ECS 컴포넌트는 `src/game/components/<도메인>.rs`, 리소스는 `src/game/resources/<도메인>.rs`에 둡니다. 각 `mod.rs`가 `pub use`로 다시 내보냅니다.
   - 대기실과 방 관련 코드는 `src/hub/`에 있습니다. 허브는 `mod.rs`, 좌석은 `seats.rs`, 방은 `room.rs`입니다.
   - `mod.rs`가 맡는 일은 모듈 선언, 다시 내보내기, 조립(스케줄 구성, `Match`의 공개 API)뿐입니다.
   - 이유: 파일 이름만 보고 규칙이 있는 곳을 찾을 수 있고, 한 파일이 커지면서 여러 도메인이 얽히는 것을 막습니다.
2. **비즈니스 로직은 순수 함수로 두고, 단위 테스트를 같은 파일 맨 아래에 씁니다.**
   - 순수 함수는 결과가 인자로만 정해지는 함수입니다. `World`·`Query`·`Res`에 접근하거나 I/O를 하지 않습니다. 다음 두 가지는 허용합니다.
     - 평범한 구조체를 `&mut`로 받아 고치는 것(예: `&mut Weapon`)
     - 난수를 `&mut GameRng`로 받는 것
   - 시스템 함수(`Query`·`Res`를 받는 함수)가 하는 일은 네 가지뿐입니다. 컴포넌트를 읽고, 순수 함수를 부르고, 결과를 쓰고, 이벤트를 남깁니다.
     - 예: `systems/fire.rs`의 `fire_weapons`는 `chamber`·`spread`·`first_struck`을 부르고, 맞힌 결과는 `land_hits`가 `take_hit`·`reward_killer`로 처리합니다.
   - 테스트는 그 로직이 있는 `.rs` 파일 맨 아래의 `#[cfg(test)] mod tests`에 둡니다.
     - 순수 함수의 단위 테스트를 먼저 쓰고, `Match`를 여러 틱 돌리는 시나리오 테스트는 그 뒤에 둡니다.
     - 시나리오 테스트가 함께 쓰는 도우미(`duel`, `place`, `cmd` 등)는 `src/game/test_support.rs`에 있습니다.
   - 테스트 이름은 그 테스트가 지키는 규칙을 영어 문장으로 씁니다(예: `a_kill_below_level_zero_goes_straight_to_level_one`).
   - 개발자가 조정하는 밸런스 값(`KING_TIME`, 무기 대미지 등)은 숫자로 적지 않고 상수와 비교합니다. 숫자로 적으면 밸런스를 바꿀 때마다 테스트가 깨집니다.
   - 이유: 규칙을 ECS 없이 바로 검증할 수 있고, 규칙과 테스트가 한곳에 있어 함께 고치게 됩니다.
   - 확인: 자동 검사는 없습니다. 새 로직에 테스트가 없으면 리뷰에서 막습니다.
3. **`unsafe` 코드는 쓰지 않습니다.**
   - `Cargo.toml`의 `[lints.rust] unsafe_code = "forbid"`가 컴파일 오류로 막습니다.
   - 이유: 게임 서버에는 unsafe가 필요한 곳이 없으므로, 메모리 안전성을 컴파일러에 맡깁니다.
4. **넘칠 수 있는 정수 연산은 넘칠 때 어떻게 할지 코드에 드러냅니다.**
   - 체력·탄약·점수·틱·카운터에는 `saturating_*`를 씁니다.
   - 계속 늘어나는 ID(봇·방·미사일·수류탄)에는 `wrapping_*`를 씁니다.
   - 넘치는 경우를 따로 처리해야 하면 `checked_*`를 씁니다.
   - 넘칠 수 없는 것이 분명한 곳은 `#[expect(clippy::arithmetic_side_effects, reason = "...")]`에 그 이유를 적습니다(예: `nav.rs`의 격자 계산, `hub/mod.rs`의 틱 시계).
   - 확인: `Cargo.toml`이 `clippy::arithmetic_side_effects`를 warn으로 켜고, clippy를 `-D warnings`로 돌려 실패시킵니다. 실수(`f32`) 연산은 검사 대상이 아니고, 테스트 코드도 같은 규칙을 따릅니다.
   - 이유: release 빌드는 정수 오버플로를 조용히 감싸서(wrap) 엉뚱한 값을 만듭니다. 허브 스레드 하나가 모든 방을 돌리므로 panic도 피해야 합니다.
5. **서버 작업을 마칠 때마다 `server/`에서 `cargo fmt`와 `cargo clippy --all-targets -- -D warnings`를 실행합니다.**
   - 둘 다 경고 없이 끝나야 작업이 끝난 것입니다. `cargo test --release`도 함께 돌립니다.
   - Claude Code에서는 Stop 훅(`.claude/hooks/server-check.sh`)이 이 일을 대신합니다.
     - 작업을 마칠 때 서버 소스가 마지막 검사 뒤로 바뀌었으면 `cargo fmt`를 적용하고 clippy를 돌립니다.
     - fmt가 파일을 고쳤거나 clippy가 실패하면 종료를 한 번 막고 결과를 보여 줍니다.
     - 다른 도구에서는 직접 실행합니다.
   - 이유: 포맷과 린트를 사람이 기억하지 않아도 되고, 규칙 3·4의 검사가 매번 돌게 됩니다.
