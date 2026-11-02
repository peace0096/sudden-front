# 프로토콜

정의는 `proto/game.proto`(패키지 `game.v1`) 한 곳에 있습니다.

- 서버: `server/build.rs`가 prost-build로 생성 → `crate::pb`
- 클라이언트: `npm run gen`(buf + protoc-gen-es) → `client/src/gen/game_pb.ts` (gitignore, 빌드 시 자동 생성)

모든 메시지는 WebSocket(`ws://<host>/ws`) 바이너리 프레임 하나에 하나씩 담습니다. 클라이언트는 `ClientMessage`, 서버는 `ServerMessage`를 보내며 둘 다 `oneof payload` 봉투입니다.

## 흐름

```
Client                                   Server
  Hello{name}                    ──▶
                                 ◀──     Welcome{player_id, weapons[]}
                                 ◀──     RoomList
  CreateRoom{name, fill_bots, map, mode} / JoinRoom{room_id}
                                 ◀──     RoomState (방 정보 + 참가자)
  SwitchTeam / SelectWeapon / SetKillsToWin(방장)
  StartGame(방장)                ──▶
                                 ◀──     MatchStart{your_id, config, map, roster}
  InputBatch{cmds[]} (60Hz 명령) ──▶
  PickAugment / RefreshAugments{slot}
                                 ◀──     Snapshot (30Hz)
                                 ◀──     RosterUpdate (참가/이탈, 봇 대체)
                                 ◀──     MatchEnd{winner, scoreboard, return_in}
  LeaveRoom                      ──▶     LeftRoom
  Ping{client_time}              ──▶     Pong
```

## ClientMessage

| 번호 | 메시지 | 설명 |
| --- | --- | --- |
| 1 | Hello | 닉네임 (최대 16자) |
| 2 | CreateRoom | 방 이름(최대 24자), 봇 채우기, 맵, 모드 |
| 3 | JoinRoom | 방 번호 |
| 4 | LeaveRoom | 방 나가기 |
| 5 | SwitchTeam | 팀 변경 (대기실) |
| 6 | StartGame | 방장만 |
| 7 | InputBatch | `InputCmd` 묶음 |
| 8 | Ping | 왕복 시간 측정 |
| 9 | SelectWeapon | 주무기 선택 (대기실 또는 게임 중 B키) |
| 10 | PickAugment | 제시된 카드 중 하나 선택 |
| 11 | RefreshAugments | `slot`(0~2) 카드 한 장만 교체, 카드당 1회 |
| 12 | SetKillsToWin | 방장만, 10/20/30/50 중 하나 |

### InputCmd (1/60초 한 스텝)

| 필드 | 설명 |
| --- | --- |
| seq | 증가하는 명령 번호 (보정 기준) |
| forward/back/left/right | 이동 키 |
| fire, reload | 사격, 장전 |
| yaw, pitch | 시점. 사격 시에는 반동이 반영된 조준각 |
| render_tick | 클라이언트가 원격 플레이어를 그리던 서버 틱 (lag compensation) |
| aim | 스코프 (저격소총) |
| slot | 먼저 바꿀 무기 칸: 1 주무기, 2 보조, 3 나이프, 0 유지 |
| crouch | 앉기 |

## ServerMessage

| 번호 | 메시지 | 설명 |
| --- | --- | --- |
| 1 | Welcome | 내 ID, 모든 무기 스탯(`WeaponStats`) |
| 2 | RoomList | 방 목록 (`RoomInfo`: 모드, 맵, 목표 킬 포함) |
| 3 | RoomState | 현재 방과 참가자 |
| 4 | LeftRoom | 방에서 나감 |
| 5 | ErrorMessage | 거부 사유 |
| 6 | MatchStart | 게임 설정(`GameConfig`), 맵 박스·소품, 명단 |
| 7 | Snapshot | 틱 상태 |
| 8 | RosterUpdate | 명단 변경 |
| 9 | MatchEnd | 승리 팀, 점수판, 대기실 복귀까지 남은 시간 |
| 10 | Pong | |

### GameConfig
틱 레이트(30), 목표 킬, 플레이어 크기·눈높이, 부활 시간, 무기 스탯, 꺼내기 시간, 앉기 계수, 최대 HP, 모드, 시작 대기 시간.

### Snapshot

| 번호 | 필드 | 범위 | 설명 |
| --- | --- | --- | --- |
| 1 | tick | 공용 | 서버 틱 |
| 2 | last_input_seq | 수신자 | 처리된 마지막 명령 |
| 3-5 | magazine, reserve, reload_remaining | 수신자 | 현재 무기 탄약과 장전 |
| 6-7 | red_score, blue_score | 공용 | |
| 8 | players | 공용 | `PlayerState` 목록 |
| 9 | events | 공용 | `GameEvent` 목록 |
| 10 | selected_weapon | 수신자 | 다음 부활부터 쓸 주무기 |
| 11 | primary_weapon | 수신자 | 이번 생의 1번 무기 |
| 12 | freeze_left | 공용 | 시작까지 남은 대기 시간 |
| 13 | offered | 수신자 | 고를 카드, 고른 뒤엔 빈 목록 |
| 14 | (reserved) | | 옛 `can_refresh` |
| 15 | sidearm_weapon | 수신자 | 2번 무기 (증강에 따라 바뀜) |
| 16 | pick_left | 공용 | 카드 선택 남은 시간 |
| 17 | clock | 공용 | 경기 경과 시간 |
| 18 | refreshable | 수신자 | 카드별 교체 가능 여부 |
| 19 | missiles | 공용 | 비행 중인 미사일 (위치+속도, 클라이언트가 외삽) |

### PlayerState
위치(x, z), yaw/pitch, hp, 생존, 이동 중, 장전 중, 보호 중, 부활까지 남은 시간, 킬/데스, 들고 있는 무기, 앉기, 고른 증강(`augment`), 방패 HP, 머리 크기 배율(`head_scale`), 실제 적용 증강 목록(`active`, 더블 타임이면 2개), 마비 남은 시간(`stun_left`).

### GameEvent

| 번호 | 이벤트 | 설명 |
| --- | --- | --- |
| 1 | ShotEvent | 발사 위치·도착 위치, 맞은 플레이어, 근접/산탄 펠릿/폭발형 여부 |
| 2 | DamageEvent | 공격자, 피해자, 양, 부위, 방패에 맞았는지 |
| 3 | KillEvent | 킬러, 피해자, 헤드샷, 무기 |
| 4 | ReloadEvent | |
| 5 | RespawnEvent | |
| 6 | ExplosionEvent | 미사일 폭발 위치 |

## 열거형

- `WeaponKind`: RIFLE, SNIPER, MACHINE_GUN, SMG, PISTOL, KNIFE, SHOTGUN(7), MISSILE(8, 옛 RPG 자리), PISTOL_PLUS, MAGNUM(10), AKIMBO_SMG(11), TASER(12)
- `Augment`: INFINITE_AMMO … MISSILE(13, 옛 RPG 자리), MAGNUM, BIG_HEAD, VEST, AKIMBO, DOUBLE_TIME(18), TASER(19)
- `AugmentGrade`: SILVER, GOLD, PRISM
- `GameMode`: NORMAL, AUGMENT
- `MapId`: SUPPLY_YARD, WAREHOUSE
- `HitZone`: HEAD, BODY, LEGS

정확한 번호는 `proto/game.proto`를 기준으로 합니다. 필드를 지울 때는 번호를 `reserved`로 남깁니다.
