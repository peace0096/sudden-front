# Sudden Front 문서

웹 브라우저용 5대5 팀 데스매치 FPS(서든어택 스타일)의 설계와 작업 내용을 정리한 문서입니다.

| 문서 | 내용 |
| --- | --- |
| [architecture.md](architecture.md) | 전체 구조, 서버 ECS 스케줄, 네트워크 동기화(예측·보정·lag compensation) |
| [protocol.md](protocol.md) | `proto/game.proto` 메시지 흐름과 필드 설명 |
| [gameplay.md](gameplay.md) | 규칙, 조작, 맵, 무기 수치, 반동·탄퍼짐 모델, 봇 |
| [augment-mode.md](augment-mode.md) | 증강모드: 진행 흐름, 카드 배분, 증강 19종과 전용 무기 |
| [development.md](development.md) | 빌드·실행, 코드 위치, 테스트(cargo test, Playwright e2e), 수치 조정 방법 |
| [history.md](history.md) | 작업 이력, 설계 결정, 알려진 이슈 |
| [credits.md](credits.md) | 에셋 출처와 라이선스 (`tools/art/build-art.mjs`가 자동 생성) |

## 한눈에 보기

- 서버: Rust, bevy_ecs 0.18(방마다 World 하나, 30Hz), axum WebSocket, prost
- 클라이언트: TypeScript, three.js, Vite, protobuf-es. 캐릭터·총기·맵 소품·이펙트는 출처·라이선스를 기록한 스타일라이즈드 glTF/WebP 에셋, 사운드와 벽·하늘·이름표 텍스처는 코드로 생성
- 프로토콜: Protobuf3 바이너리 프레임 over WebSocket (`/ws`)
- 서버 권위(authoritative) 판정, 클라이언트 이동 예측 + 서버 보정, 사격 시 lag compensation
- 모드: 일반모드(HP 100), 증강모드(HP 150, 시작 시 증강 카드 선택)
- 저장소: https://github.com/peace0096/sudden-front (private)
