# 프로토콜

정의는 `proto/game.proto`(패키지 `game.v1`) 한 곳에 있습니다.

- 서버: `server/build.rs`가 prost-build로 생성 → `crate::pb`
- 클라이언트: `npm run gen`(buf + protoc-gen-es) → `client/src/gen/game_pb.ts` (gitignore, 빌드 시 자동 생성)

모든 메시지는 WebSocket(`ws://<host>/ws`) 바이너리 프레임 하나에 하나씩 담습니다. 클라이언트는 `ClientMessage`, 서버는 `ServerMessage`를 보내며 둘 다 `oneof payload` 봉투입니다.

## 흐름

```
Client                                   Server
  Hello{name}                    ──▶
                                 ◀──     Welcome{player_id, name}
                                 ◀──     RoomList
  CreateRoom{name, fill_bots, map, mode, match_type} / JoinRoom{room_id}
                                 ◀──     RoomState (방 정보 + 참가자)
  SwitchTeam(팀전) / SetReady(방장 외) / SetKillsToWin·SetGameMode·SetMap·SetBotSlot(방장)
  StartGame(방장)                ──▶
                                 ◀──     MatchStart{your_id, config, map, roster}
  InputBatch{cmds[]} (60Hz 명령) ──▶
  SelectWeapon(B키) / PickAugment / RefreshAugments{slot}
                                 ◀──     Snapshot (30Hz)
                                 ◀──     RosterUpdate (참가/이탈)
                                 ◀──     MatchEnd{winner | winner_id, scoreboard, return_in}
  LeaveRoom                      ──▶     LeftRoom
  Ping{client_time}              ──▶     Pong
```

## ClientMessage

| 번호 | 메시지 | 설명 |
| --- | --- | --- |
| 1 | Hello | 닉네임 (최대 16자) |
| 2 | CreateRoom | 방 이름(최대 24자), 맵, 모드(일반/증강), 방식 `match_type`(팀 데스매치/개인전, 지정하지 않거나 모르는 값이면 팀 데스매치), `fill_bots`(방장 칸 외 모든 칸을 봇으로 시작) |
| 3 | JoinRoom | 방 번호. 경기 중인 방이면 빈 칸에 도중 참가: 들어온 사람은 `MatchStart`, 나머지는 `RosterUpdate`를 받음. 결과 화면 중이면 "경기가 끝나는 중입니다. 잠시 후 다시 들어오세요.", 빈 칸이 없으면 "빈 자리가 없습니다." |
| 4 | LeaveRoom | 방 나가기 |
| 5 | SwitchTeam | 팀 변경 (대기실). 개인전 방에서는 거부 |
| 6 | StartGame | 방장만 |
| 7 | InputBatch | `InputCmd` 묶음 |
| 8 | Ping | 왕복 시간 측정 |
| 9 | SelectWeapon | 주무기 선택 (게임 중 B키). 다음 경기에도 유지 |
| 10 | PickAugment | 제시된 카드 중 하나 선택 |
| 11 | RefreshAugments | `slot`(0~2) 카드 한 장만 교체, 카드당 1회 |
| 12 | SetKillsToWin | 방장만, 10/20/30/50 중 하나 (기본값 팀전 30, 개인전 20) |
| 13 | SetBotSlot | 방장만, 대기실에서 `team`·`slot`(0~4) 칸에 봇을 넣거나 뺌(`bot`). 개인전 방은 `team` UNSPECIFIED에 방 전체 칸 번호 `slot`(0~9). 사람이 있는 칸은 무시 |
| 14 | SetReady | 방장이 아닌 플레이어가 대기실에서 준비(`ready`)를 켜거나 끔. 방장 것과 경기 중의 것은 무시. 방장 외 모두가 준비해야 `StartGame`이 받아들여지고, 경기가 시작되면 모두의 준비가 풀림 |
| 15 | SetGameMode | 방장만, 대기실에서 다음 경기의 모드 `mode`(NORMAL/AUGMENT). 방장이 아니면 "방장만 게임 모드를 바꿀 수 있습니다.", 같은 값·UNSPECIFIED·모르는 값·경기 중의 것은 아무에게도 알리지 않고 무시. 바뀌면 방 사람들에게 `RoomState`, 로비에 `RoomList` |
| 16 | SetMap | 방장만, 대기실에서 다음 경기의 맵 `map`(SUPPLY_YARD/WAREHOUSE). 거부·무시 규칙은 SetGameMode와 같음("방장만 맵을 바꿀 수 있습니다.") |

### InputCmd (1/60초 한 스텝)

| 필드 | 설명 |
| --- | --- |
| seq | 증가하는 명령 번호 (보정 기준) |
| forward/back/left/right | 이동 키 |
| fire, reload | 사격, 장전 |
| yaw, pitch | 시점. 사격 시에는 반동이 반영된 조준각 |
| render_tick | 클라이언트가 원격 플레이어를 그리던 서버 틱 (lag compensation) |
| aim | 스코프 (저격소총) |
| slot | 먼저 바꿀 무기 칸: 1 주무기, 2 보조, 3 나이프, 4 수류탄(남아 있을 때만, 없으면 무시), 0 유지. 수류탄을 들고 `fire`면 던지고 그 전 칸으로 돌아감 |
| crouch | 앉기 |
| jump | 점프(14). 누르고 있는 동안 보내며, 서 있을 때만 뜀 |

## ServerMessage

| 번호 | 메시지 | 설명 |
| --- | --- | --- |
| 1 | Welcome | 내 ID, 닉네임 |
| 2 | RoomList | 방 목록 (`RoomInfo`: 모드, 방식 `match_type`, 맵, 목표 킬, 봇 수 `bot_count` 포함) |
| 3 | RoomState | 현재 방, 참가자(`RoomPlayer`: 팀, 칸 번호 `slot`, 준비 여부 `ready`(7, 방장은 늘 false)), 봇 칸(`RoomBot`: 팀, 칸 번호). 개인전 방은 팀이 UNSPECIFIED이고 칸 번호가 0~9 |
| 4 | LeftRoom | 방에서 나감 |
| 5 | ErrorMessage | 거부 사유 |
| 6 | MatchStart | 게임 설정(`GameConfig`), 맵(`MapData`: 박스·소품), 명단 |
| 7 | Snapshot | 틱 상태 |
| 8 | RosterUpdate | 명단 변경 |
| 9 | MatchEnd | 승리 팀 `winner`(목표 킬 달성, 또는 상대 팀이 모두 나가 기권승) 또는 개인전 승자 `winner_id`(목표 킬 달성, 또는 혼자 남아 기권승. 이때 `winner`는 UNSPECIFIED, 팀 점수는 0), 점수판, 대기실 복귀까지 남은 시간 |
| 10 | Pong | |

### GameConfig
틱 레이트(30), 목표 킬, 플레이어 크기·눈높이, 부활 시간, 무기 스탯(수류탄 포함: `magazine`이 목숨당 개수, `fire_interval`이 던진 뒤 다음 무기까지 시간, `splash_radius`가 폭발 반경), 꺼내기 시간, 앉기 계수, 최대 HP, 모드, 시작 대기 시간, 방식(`match_type`, 15), 점프 이륙 속도(`jump_speed`, 16)·중력(`gravity`, 17).

### MapData
이름, `MapId`, 반폭(`half_x`, `half_z`), 충돌 박스 `boxes`(`MapBox`: 종류와 min/max), 소품 `props`(`MapProp`: 종류, 위치, 회전, 크기). `MapProp.tilt`(8)는 기울어진 컨테이너가 길이 방향으로 +X 끝을 들어 올린 각도(rad)이고, 그 충돌은 서버가 이 기울기를 따라 자른 INVISIBLE 박스 조각으로 `boxes`에 들어 있습니다.

### Snapshot

| 번호 | 필드 | 범위 | 설명 |
| --- | --- | --- | --- |
| 1 | tick | 공용 | 서버 틱 |
| 2 | last_input_seq | 수신자 | 처리된 마지막 명령 |
| 3-5 | magazine, reserve, reload_remaining | 수신자 | 현재 무기 탄약과 장전 |
| 6-7 | red_score, blue_score | 공용 | 팀 점수. 개인전은 0이고 개인 점수는 `PlayerState`의 킬 |
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
| 20 | grenades | 공용 | 아직 터지지 않은 수류탄 (`Grenade`: id, 위치, 속도. 멈추면 속도 0. 클라이언트가 최대 0.1초 외삽하고, 스냅샷 사이 속도가 2.5m/s 넘게 바뀌면 부딪힌 소리를 냄) |
| 21 | grenades_left | 수신자 | 이번 생에 남은 수류탄 개수(4번 칸) |

### PlayerState
위치(x, z)와 발 높이(`y`, 21)·수직 속도(`vy`, 22, 서 있으면 0. 클라이언트가 자기 명령을 다시 적용할 때 씀), yaw/pitch, hp, 생존, 이동 중, 장전 중, 보호 중, 부활까지 남은 시간, 킬/데스, 들고 있는 무기, 앉기, 고른 증강(`augment`), 방패 HP, 머리 크기 배율(`head_scale`), 실제 적용 증강 목록(`active`, 더블 타임이면 2개), 마비 남은 시간(`stun_left`).

### GameEvent

| 번호 | 이벤트 | 설명 |
| --- | --- | --- |
| 1 | ShotEvent | 발사 위치·도착 위치, 맞은 플레이어, 근접/산탄 펠릿/폭발형 여부 |
| 2 | DamageEvent | 공격자, 피해자, 양, 부위, 방패에 맞았는지 |
| 3 | KillEvent | 킬러, 피해자, 헤드샷, 무기 |
| 4 | ReloadEvent | |
| 5 | RespawnEvent | |
| 6 | ExplosionEvent | 미사일·수류탄 폭발 위치 |
| 7 | ThrowEvent (`grenade_thrown`) | 수류탄을 던진 플레이어. 수류탄 자체는 `Snapshot.grenades`로 옴 |

## 열거형

- `WeaponKind`: RIFLE, SNIPER, MACHINE_GUN, SMG, PISTOL, KNIFE, SHOTGUN(7), MISSILE(8, 옛 RPG 자리), PISTOL_PLUS, MAGNUM(10), AKIMBO_SMG(11), TASER(12), GRENADE(13, 4번 칸)
- `Augment`: INFINITE_AMMO … MISSILE(13, 옛 RPG 자리), MAGNUM, BIG_HEAD, VEST, AKIMBO, DOUBLE_TIME(18), TASER(19)
- `AugmentGrade`: SILVER, GOLD, PRISM
- `GameMode`: NORMAL, AUGMENT
- `MatchType`: TEAM_DEATHMATCH, FREE_FOR_ALL (`GameMode`와 따로 고르는 방식. UNSPECIFIED는 팀 데스매치로 봄)
- `Team`: RED, BLUE. 개인전 선수는 UNSPECIFIED
- `MapId`: SUPPLY_YARD, WAREHOUSE
- `HitZone`: HEAD, BODY, LEGS

정확한 번호는 `proto/game.proto`를 기준으로 합니다. 필드를 지울 때는 번호를 `reserved`로 남깁니다. 지금 비워 둔 번호: `Snapshot` 14, `RoomPlayer` 5(옛 대기실 무기 표시), `Welcome` 3(옛 대기실 무기 선택용 스탯), `RoomInfo` 7(옛 `fill_bots`).
