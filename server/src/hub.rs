//! Lobby and room management. Owns every match and drives them at a fixed tick.

use crate::game::{weapons, Match, TICK_RATE};
use crate::pb::{self, client_message::Payload as In, server_message::Payload as Out};
use prost::Message;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

pub const MAX_PLAYERS: usize = 10;
pub const TEAM_SIZE: usize = 5;
const RESULTS_SECONDS: f32 = 8.0;
const MAX_NAME_CHARS: usize = 16;
const MAX_ROOM_NAME_CHARS: usize = 24;

pub enum Inbound {
    Connect { id: u32, tx: Sender<Vec<u8>> },
    Message { id: u32, msg: pb::ClientMessage },
    Disconnect { id: u32 },
}

struct Client {
    tx: Sender<Vec<u8>>,
    name: String,
    hello: bool,
    room: Option<u32>,
}

struct Member {
    id: u32,
    team: pb::Team,
    weapon: pb::WeaponKind,
}

struct Room {
    id: u32,
    name: String,
    host: u32,
    fill_bots: bool,
    map: pb::MapId,
    mode: pb::GameMode,
    kills_to_win: u32,
    members: Vec<Member>,
    game: Option<Match>,
    results_left: Option<f32>,
}

impl Room {
    fn count(&self, team: pb::Team) -> usize {
        self.members.iter().filter(|m| m.team == team).count()
    }

    fn emptier_team(&self) -> pb::Team {
        if self.count(pb::Team::Red) <= self.count(pb::Team::Blue) {
            pb::Team::Red
        } else {
            pb::Team::Blue
        }
    }
}

#[derive(Default)]
struct Hub {
    clients: HashMap<u32, Client>,
    rooms: BTreeMap<u32, Room>,
    next_room_id: u32,
    match_seed: u64,
}

pub fn run(rx: Receiver<Inbound>) {
    let mut hub = Hub::default();
    let tick = Duration::from_secs_f64(1.0 / TICK_RATE as f64);
    let mut next_tick = Instant::now() + tick;
    loop {
        let wait = next_tick.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(event) => hub.handle(event),
            Err(RecvTimeoutError::Timeout) => {
                hub.tick();
                next_tick += tick;
                // Don't try to catch up after a long stall.
                if next_tick < Instant::now() {
                    next_tick = Instant::now() + tick;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn clean_text(raw: &str, max_chars: usize) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn encode(payload: Out) -> Vec<u8> {
    pb::ServerMessage {
        payload: Some(payload),
    }
    .encode_to_vec()
}

impl Hub {
    fn send_bytes(&mut self, id: u32, bytes: Vec<u8>) {
        let Some(client) = self.clients.get(&id) else {
            return;
        };
        // A client this far behind has lost its session anyway. Dropping its
        // sender closes the socket; the Disconnect event performs the cleanup.
        if client.tx.try_send(bytes).is_err() {
            if let Some(client) = self.clients.get_mut(&id) {
                let (dead, _) = tokio::sync::mpsc::channel(1);
                client.tx = dead;
            }
        }
    }

    fn send(&mut self, id: u32, payload: Out) {
        self.send_bytes(id, encode(payload));
    }

    fn error(&mut self, id: u32, message: &str) {
        self.send(
            id,
            Out::Error(pb::ErrorMessage {
                message: message.to_string(),
            }),
        );
    }

    fn handle(&mut self, event: Inbound) {
        match event {
            Inbound::Connect { id, tx } => {
                self.clients.insert(
                    id,
                    Client {
                        tx,
                        name: format!("Player{id}"),
                        hello: false,
                        room: None,
                    },
                );
            }
            Inbound::Disconnect { id } => {
                self.leave_room(id);
                self.clients.remove(&id);
            }
            Inbound::Message { id, msg } => {
                if let Some(payload) = msg.payload {
                    self.handle_message(id, payload);
                }
            }
        }
    }

    fn handle_message(&mut self, id: u32, payload: In) {
        match payload {
            In::Hello(hello) => self.on_hello(id, hello),
            In::CreateRoom(create) => self.on_create_room(id, create),
            In::JoinRoom(join) => self.on_join_room(id, join.room_id),
            In::LeaveRoom(_) => {
                self.leave_room(id);
                self.send(id, Out::LeftRoom(pb::LeftRoom {}));
                let list = self.room_list();
                self.send(id, list);
            }
            In::SwitchTeam(_) => self.on_switch_team(id),
            In::SelectWeapon(select) => self.on_select_weapon(id, select.weapon),
            In::StartGame(_) => self.on_start_game(id),
            In::SetKillsToWin(set) => self.on_set_kills_to_win(id, set.kills),
            In::PickAugment(pick) => {
                if let (Ok(augment), Some(game)) = (pb::Augment::try_from(pick.augment), self.game_of(id)) {
                    game.pick_augment(id, augment);
                }
            }
            In::RefreshAugments(refresh) => {
                if let Some(game) = self.game_of(id) {
                    game.refresh_augments(id, refresh.slot);
                }
            }
            In::Input(batch) => self.on_input(id, batch),
            In::Ping(ping) => self.send(
                id,
                Out::Pong(pb::Pong {
                    client_time: ping.client_time,
                }),
            ),
        }
    }

    fn game_of(&mut self, id: u32) -> Option<&mut Match> {
        let room_id = self.clients.get(&id)?.room?;
        self.rooms.get_mut(&room_id)?.game.as_mut()
    }

    fn on_hello(&mut self, id: u32, hello: pb::Hello) {
        let Some(client) = self.clients.get_mut(&id) else {
            return;
        };
        let name = clean_text(&hello.name, MAX_NAME_CHARS);
        if !name.is_empty() {
            client.name = name;
        }
        client.hello = true;
        let name = client.name.clone();
        self.send(
            id,
            Out::Welcome(pb::Welcome {
                player_id: id,
                name,
                weapons: weapons::all_stats(),
            }),
        );
        let list = self.room_list();
        self.send(id, list);
    }

    fn on_create_room(&mut self, id: u32, create: pb::CreateRoom) {
        let Some(client) = self.clients.get(&id) else {
            return;
        };
        if !client.hello || client.room.is_some() {
            return;
        }
        let mut name = clean_text(&create.name, MAX_ROOM_NAME_CHARS);
        if name.is_empty() {
            name = format!("{}의 방", client.name);
        }
        self.next_room_id += 1;
        let room_id = self.next_room_id;
        self.rooms.insert(
            room_id,
            Room {
                id: room_id,
                name,
                host: id,
                fill_bots: create.fill_bots,
                map: match pb::MapId::try_from(create.map) {
                    Ok(pb::MapId::Warehouse) => pb::MapId::Warehouse,
                    _ => pb::MapId::SupplyYard,
                },
                mode: match pb::GameMode::try_from(create.mode) {
                    Ok(pb::GameMode::Augment) => pb::GameMode::Augment,
                    _ => pb::GameMode::Normal,
                },
                kills_to_win: crate::game::KILLS_TO_WIN,
                members: vec![Member {
                    id,
                    team: pb::Team::Red,
                    weapon: pb::WeaponKind::Rifle,
                }],
                game: None,
                results_left: None,
            },
        );
        self.clients.get_mut(&id).unwrap().room = Some(room_id);
        self.send_room_state(room_id);
        self.broadcast_room_list();
    }

    fn on_join_room(&mut self, id: u32, room_id: u32) {
        let Some(client) = self.clients.get(&id) else {
            return;
        };
        if !client.hello || client.room.is_some() {
            return;
        }
        let Some(room) = self.rooms.get_mut(&room_id) else {
            self.error(id, "존재하지 않는 방입니다.");
            let list = self.room_list();
            self.send(id, list);
            return;
        };
        if room.game.is_some() {
            self.error(id, "이미 게임이 진행 중인 방입니다.");
            return;
        }
        if room.members.len() >= MAX_PLAYERS {
            self.error(id, "방이 가득 찼습니다.");
            return;
        }
        let team = room.emptier_team();
        room.members.push(Member {
            id,
            team,
            weapon: pb::WeaponKind::Rifle,
        });
        self.clients.get_mut(&id).unwrap().room = Some(room_id);
        self.send_room_state(room_id);
        self.broadcast_room_list();
    }

    fn on_switch_team(&mut self, id: u32) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        if room.game.is_some() {
            return;
        }
        let Some(current) = room.members.iter().find(|m| m.id == id).map(|m| m.team) else {
            return;
        };
        let target = if current == pb::Team::Red {
            pb::Team::Blue
        } else {
            pb::Team::Red
        };
        if room.count(target) >= TEAM_SIZE {
            self.error(id, "상대 팀이 가득 찼습니다.");
            return;
        }
        if let Some(member) = room.members.iter_mut().find(|m| m.id == id) {
            member.team = target;
        }
        self.send_room_state(room_id);
    }

    fn on_select_weapon(&mut self, id: u32, weapon: i32) {
        let Ok(kind) = pb::WeaponKind::try_from(weapon) else {
            return;
        };
        if !weapons::PRIMARIES.contains(&kind) {
            return;
        }
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        if let Some(member) = room.members.iter_mut().find(|m| m.id == id) {
            member.weapon = kind;
        }
        match room.game.as_mut() {
            Some(game) => game.select_weapon(id, kind),
            None => self.send_room_state(room_id),
        }
    }

    fn on_set_kills_to_win(&mut self, id: u32, kills: u32) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        if room.host != id {
            self.error(id, "방장만 목표 킬 수를 바꿀 수 있습니다.");
            return;
        }
        if room.game.is_some() || !crate::game::KILL_TARGETS.contains(&kills) {
            return;
        }
        room.kills_to_win = kills;
        self.send_room_state(room_id);
        self.broadcast_room_list();
    }

    fn on_start_game(&mut self, id: u32) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get(&room_id) else {
            return;
        };
        if room.host != id {
            self.error(id, "방장만 게임을 시작할 수 있습니다.");
            return;
        }
        if room.game.is_some() {
            return;
        }
        if !room.fill_bots && (room.count(pb::Team::Red) == 0 || room.count(pb::Team::Blue) == 0) {
            self.error(id, "봇 없이 시작하려면 양 팀에 최소 1명씩 있어야 합니다.");
            return;
        }

        let humans: Vec<(u32, String, pb::Team, pb::WeaponKind)> = room
            .members
            .iter()
            .filter_map(|m| {
                self.clients
                    .get(&m.id)
                    .map(|c| (m.id, c.name.clone(), m.team, m.weapon))
            })
            .collect();
        let fill_bots = room.fill_bots;
        let map = room.map;
        self.match_seed = self.match_seed.wrapping_add(1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9);
        let seed = self.match_seed ^ now;
        let mut game = Match::with_mode(&humans, map, room.mode, fill_bots, seed);
        game.set_kills_to_win(room.kills_to_win);
        let starts: Vec<(u32, pb::MatchStart)> = humans
            .iter()
            .map(|(pid, _, _, _)| (*pid, game.start_message(*pid)))
            .collect();
        self.rooms.get_mut(&room_id).unwrap().game = Some(game);
        for (pid, start) in starts {
            self.send(pid, Out::MatchStart(start));
        }
        self.broadcast_room_list();
    }

    fn on_input(&mut self, id: u32, batch: pb::InputBatch) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        if let Some(game) = self.rooms.get_mut(&room_id).and_then(|r| r.game.as_mut()) {
            game.push_inputs(id, batch.cmds);
        }
    }

    fn leave_room(&mut self, id: u32) {
        let Some(room_id) = self.clients.get_mut(&id).and_then(|c| c.room.take()) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        room.members.retain(|m| m.id != id);
        if room.members.is_empty() {
            self.rooms.remove(&room_id);
            self.broadcast_room_list();
            return;
        }
        if room.host == id {
            room.host = room.members[0].id;
        }
        let member_ids: Vec<u32> = room.members.iter().map(|m| m.id).collect();
        if let Some(game) = room.game.as_mut() {
            let roster = game.remove_player(id);
            for member in member_ids {
                self.send(
                    member,
                    Out::RosterUpdate(pb::RosterUpdate {
                        roster: roster.clone(),
                    }),
                );
            }
        } else {
            self.send_room_state(room_id);
        }
        self.broadcast_room_list();
    }

    fn tick(&mut self) {
        let dt = 1.0 / TICK_RATE as f32;
        let room_ids: Vec<u32> = self.rooms.keys().copied().collect();
        for room_id in room_ids {
            self.tick_room(room_id, dt);
        }
    }

    fn tick_room(&mut self, room_id: u32, dt: f32) {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };

        // Results screen countdown, then everyone is back in the waiting room.
        if let Some(left) = room.results_left.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                room.results_left = None;
                room.game = None;
                self.send_room_state(room_id);
                self.broadcast_room_list();
            }
            return;
        }

        let Some(game) = room.game.as_mut() else {
            return;
        };
        game.tick();

        let member_ids: Vec<u32> = room.members.iter().map(|m| m.id).collect();
        let base = game.shared_snapshot();
        let personal: Vec<(u32, Vec<u8>)> = member_ids
            .iter()
            .map(|id| {
                let mut snapshot = base.clone();
                game.fill_personal(*id, &mut snapshot);
                (*id, encode(Out::Snapshot(snapshot)))
            })
            .collect();

        let ended = game
            .winner()
            .map(|winner| encode(Out::MatchEnd(game.end_message(winner, RESULTS_SECONDS))));
        if ended.is_some() {
            room.results_left = Some(RESULTS_SECONDS);
        }

        for (id, bytes) in personal {
            self.send_bytes(id, bytes);
        }
        if let Some(bytes) = ended {
            for id in member_ids {
                self.send_bytes(id, bytes.clone());
            }
        }
    }

    fn room_info(&self, room: &Room) -> pb::RoomInfo {
        pb::RoomInfo {
            id: room.id,
            name: room.name.clone(),
            host_name: self
                .clients
                .get(&room.host)
                .map(|c| c.name.clone())
                .unwrap_or_default(),
            player_count: room.members.len() as u32,
            max_players: MAX_PLAYERS as u32,
            phase: if room.game.is_some() {
                pb::RoomPhase::InGame as i32
            } else {
                pb::RoomPhase::Waiting as i32
            },
            fill_bots: room.fill_bots,
            map: room.map as i32,
            mode: room.mode as i32,
            kills_to_win: room.kills_to_win,
        }
    }

    fn room_list(&self) -> Out {
        Out::RoomList(pb::RoomList {
            rooms: self.rooms.values().map(|r| self.room_info(r)).collect(),
        })
    }

    fn broadcast_room_list(&mut self) {
        let bytes = encode(self.room_list());
        let lobby: Vec<u32> = self
            .clients
            .iter()
            .filter(|(_, c)| c.hello && c.room.is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in lobby {
            self.send_bytes(id, bytes.clone());
        }
    }

    fn send_room_state(&mut self, room_id: u32) {
        let Some(room) = self.rooms.get(&room_id) else {
            return;
        };
        let players: Vec<pb::RoomPlayer> = room
            .members
            .iter()
            .map(|m| pb::RoomPlayer {
                id: m.id,
                name: self
                    .clients
                    .get(&m.id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                team: m.team as i32,
                is_host: m.id == room.host,
                weapon: m.weapon as i32,
            })
            .collect();
        let bytes = encode(Out::RoomState(pb::RoomState {
            room: Some(self.room_info(room)),
            players,
        }));
        let ids: Vec<u32> = room.members.iter().map(|m| m.id).collect();
        for id in ids {
            self.send_bytes(id, bytes.clone());
        }
    }
}
