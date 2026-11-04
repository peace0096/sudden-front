//! Lobby and room management. Owns every match and drives them at a fixed tick.

use crate::game::{weapons, Match, TEAM_SIZE, TICK_RATE};
use crate::pb::{self, client_message::Payload as In, server_message::Payload as Out};
use prost::Message;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

pub const MAX_PLAYERS: usize = 10;
/// Both teams, in the order the seats are indexed.
const TEAMS: [pb::Team; 2] = [pb::Team::Red, pb::Team::Blue];
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
    weapon: pb::WeaponKind,
}

/// Who holds a seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seat {
    Open,
    Bot,
    Human(u32),
}

/// A room's seats: five on each team, indexed `[red, blue][slot]`. A seat keeps its position,
/// so when someone leaves it just becomes open and nobody shifts.
#[derive(Debug)]
struct Seats([[Seat; TEAM_SIZE]; 2]);

impl Seats {
    /// Every seat starts as a bot, or open.
    fn new(bots: bool) -> Self {
        Self([[if bots { Seat::Bot } else { Seat::Open }; TEAM_SIZE]; 2])
    }

    /// Every seat as `(team, slot, seat)`, red first.
    fn iter(&self) -> impl Iterator<Item = (usize, usize, Seat)> + '_ {
        self.0.iter().enumerate().flat_map(|(team, seats)| {
            seats
                .iter()
                .enumerate()
                .map(move |(slot, seat)| (team, slot, *seat))
        })
    }

    fn find(&self, id: u32) -> Option<(usize, usize)> {
        self.iter()
            .find(|(_, _, seat)| *seat == Seat::Human(id))
            .map(|(team, slot, _)| (team, slot))
    }

    fn humans(&self, team: usize) -> usize {
        self.0[team]
            .iter()
            .filter(|seat| matches!(seat, Seat::Human(_)))
            .count()
    }

    fn bots(&self, team: usize) -> usize {
        self.0[team]
            .iter()
            .filter(|seat| **seat == Seat::Bot)
            .count()
    }

    fn bot_count(&self) -> usize {
        self.bots(0) + self.bots(1)
    }

    fn has_open(&self, team: usize) -> bool {
        self.0[team].contains(&Seat::Open)
    }

    /// The seat a newcomer to `team` would take, and what it holds now: the first open seat, or
    /// else the first bot's. `None` when the team is all humans.
    fn free_seat(&self, team: usize) -> Option<(usize, Seat)> {
        let seats = &self.0[team];
        [Seat::Open, Seat::Bot]
            .into_iter()
            .find_map(|kind| Some((seats.iter().position(|seat| *seat == kind)?, kind)))
    }

    /// Seats `id` on `team`; a bot in the seat gives it up. `None` when the team is all humans.
    fn sit(&mut self, team: usize, id: u32) -> Option<usize> {
        let (slot, _) = self.free_seat(team)?;
        self.0[team][slot] = Seat::Human(id);
        Some(slot)
    }

    /// Moves `id` to the other team, as a newcomer would sit there. What they displace takes
    /// their old seat: an open seat stays open and a bot moves over, so both teams keep their size.
    /// `false` when `id` has no seat or the other team is all humans.
    fn switch(&mut self, id: u32) -> bool {
        let Some((team, slot)) = self.find(id) else {
            return false;
        };
        let Some((new_slot, displaced)) = self.free_seat(1 - team) else {
            return false;
        };
        self.0[1 - team][new_slot] = Seat::Human(id);
        self.0[team][slot] = displaced;
        true
    }

    /// Frees the seat of `id`.
    fn leave(&mut self, id: u32) {
        if let Some((team, slot)) = self.find(id) {
            self.0[team][slot] = Seat::Open;
        }
    }

    /// Puts a bot in a seat or takes it out. `false` when nothing changed: there is no such seat,
    /// a human holds it, or it already was so.
    fn set_bot(&mut self, team: usize, slot: usize, bot: bool) -> bool {
        let wanted = if bot { Seat::Bot } else { Seat::Open };
        match self.0.get_mut(team).and_then(|seats| seats.get_mut(slot)) {
            Some(seat) if !matches!(seat, Seat::Human(_)) && *seat != wanted => {
                *seat = wanted;
                true
            }
            _ => false,
        }
    }
}

struct Room {
    id: u32,
    name: String,
    host: u32,
    map: pb::MapId,
    mode: pb::GameMode,
    kills_to_win: u32,
    seats: Seats,
    /// Everyone in the room, longest first: the first takes over as host. Each holds a seat.
    members: Vec<Member>,
    game: Option<Match>,
    results_left: Option<f32>,
}

impl Room {
    /// Seats a newcomer on the team with fewer humans, or else on the other one. When the humans
    /// are level, a team with an open seat is preferred. `false` when both teams are all humans.
    fn seat_newcomer(&mut self, id: u32) -> bool {
        let first = match self.seats.humans(0).cmp(&self.seats.humans(1)) {
            Ordering::Less => 0,
            Ordering::Greater => 1,
            // Level: a team with an open seat comes before one where a bot would have to give way.
            Ordering::Equal if !self.seats.has_open(0) && self.seats.has_open(1) => 1,
            Ordering::Equal => 0,
        };
        for team in [first, 1 - first] {
            if self.seats.sit(team, id).is_some() {
                return true;
            }
        }
        false
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
            In::SetBotSlot(set) => self.on_set_bot_slot(id, set),
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
            Out::Welcome(pb::Welcome { player_id: id, name }),
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
        let mut seats = Seats::new(create.fill_bots);
        seats.sit(0, id);
        self.rooms.insert(
            room_id,
            Room {
                id: room_id,
                name,
                host: id,
                map: match pb::MapId::try_from(create.map) {
                    Ok(pb::MapId::Warehouse) => pb::MapId::Warehouse,
                    _ => pb::MapId::SupplyYard,
                },
                mode: match pb::GameMode::try_from(create.mode) {
                    Ok(pb::GameMode::Augment) => pb::GameMode::Augment,
                    _ => pb::GameMode::Normal,
                },
                kills_to_win: crate::game::KILLS_TO_WIN,
                seats,
                members: vec![Member {
                    id,
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
        if room.members.len() >= MAX_PLAYERS || !room.seat_newcomer(id) {
            self.error(id, "방이 가득 찼습니다.");
            return;
        }
        room.members.push(Member {
            id,
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
        if room.seats.find(id).is_none() {
            return;
        }
        if !room.seats.switch(id) {
            self.error(id, "상대 팀이 가득 찼습니다.");
            return;
        }
        // Neither the players nor the bots of the room change, so the room list stays as it is.
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
        if let Some(game) = room.game.as_mut() {
            game.select_weapon(id, kind);
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
        // A repeat of the current value is dropped, or it would re-broadcast the room list.
        if room.game.is_some()
            || room.kills_to_win == kills
            || !crate::game::KILL_TARGETS.contains(&kills)
        {
            return;
        }
        room.kills_to_win = kills;
        self.send_room_state(room_id);
        self.broadcast_room_list();
    }

    fn on_set_bot_slot(&mut self, id: u32, set: pb::SetBotSlot) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        if room.host != id {
            self.error(id, "방장만 봇 자리를 바꿀 수 있습니다.");
            return;
        }
        let Some(team) = TEAMS.iter().position(|team| *team as i32 == set.team) else {
            return;
        };
        // A seat no one can change, or already as asked, must not re-broadcast the room list.
        if room.game.is_some() || !room.seats.set_bot(team, set.slot as usize, set.bot) {
            return;
        }
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
        if (0..2).any(|team| room.seats.humans(team) + room.seats.bots(team) == 0) {
            self.error(
                id,
                "양 팀에 사람이나 봇이 최소 1명씩 있어야 시작할 수 있습니다.",
            );
            return;
        }

        let humans: Vec<(u32, String, pb::Team, pb::WeaponKind)> = room
            .members
            .iter()
            .filter_map(|m| {
                let (team, _) = room.seats.find(m.id)?;
                let client = self.clients.get(&m.id)?;
                Some((m.id, client.name.clone(), TEAMS[team], m.weapon))
            })
            .collect();
        let bots = [room.seats.bots(0), room.seats.bots(1)];
        let map = room.map;
        self.match_seed = self.match_seed.wrapping_add(1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9);
        let seed = self.match_seed ^ now;
        let mut game = Match::with_mode(&humans, map, room.mode, bots, seed);
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
        room.seats.leave(id);
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
            map: room.map as i32,
            mode: room.mode as i32,
            kills_to_win: room.kills_to_win,
            bot_count: room.seats.bot_count() as u32,
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
        let (mut players, mut bots) = (Vec::new(), Vec::new());
        for (team, slot, seat) in room.seats.iter() {
            let (team, slot) = (TEAMS[team] as i32, slot as u32);
            match seat {
                Seat::Human(id) => players.push(pb::RoomPlayer {
                    id,
                    name: self
                        .clients
                        .get(&id)
                        .map(|c| c.name.clone())
                        .unwrap_or_default(),
                    team,
                    is_host: id == room.host,
                    slot,
                }),
                Seat::Bot => bots.push(pb::RoomBot { team, slot }),
                Seat::Open => {}
            }
        }
        let bytes = encode(Out::RoomState(pb::RoomState {
            room: Some(self.room_info(room)),
            players,
            bots,
        }));
        let ids: Vec<u32> = room.members.iter().map(|m| m.id).collect();
        for id in ids {
            self.send_bytes(id, bytes.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{channel, Receiver};

    const RED: usize = 0;
    const BLUE: usize = 1;

    fn room_with(bots: bool) -> Room {
        Room {
            id: 1,
            name: "room".into(),
            host: 0,
            map: pb::MapId::SupplyYard,
            mode: pb::GameMode::Normal,
            kills_to_win: crate::game::KILLS_TO_WIN,
            seats: Seats::new(bots),
            members: Vec::new(),
            game: None,
            results_left: None,
        }
    }

    // ---------------------------------------------------------------- seats

    #[test]
    fn a_newcomer_takes_an_open_seat_before_any_bot_and_then_the_first_bot_gives_way() {
        let mut seats = Seats::new(true);
        seats.set_bot(RED, 2, false);
        assert_eq!(seats.sit(RED, 7), Some(2));
        assert_eq!(seats.sit(RED, 8), Some(0));
        assert_eq!(seats.sit(RED, 9), Some(1));
        assert_eq!((seats.humans(RED), seats.bots(RED)), (3, 2));
    }

    #[test]
    fn a_team_of_five_humans_has_no_seat_left() {
        let mut seats = Seats::new(true);
        for id in 1..=5 {
            assert!(seats.sit(BLUE, id).is_some());
        }
        assert_eq!((seats.humans(BLUE), seats.bots(BLUE)), (5, 0));
        assert_eq!(seats.sit(BLUE, 6), None);
    }

    #[test]
    fn leaving_frees_the_seat_as_open_and_nobody_shifts() {
        let mut seats = Seats::new(false);
        for id in 1..=3 {
            seats.sit(RED, id);
        }
        seats.leave(1);
        assert_eq!(seats.find(2), Some((RED, 1)));
        assert_eq!(seats.find(3), Some((RED, 2)));
        assert_eq!(seats.sit(RED, 4), Some(0));

        // In a room full of bots the freed seat is open too, not a bot again.
        let mut seats = Seats::new(true);
        seats.sit(RED, 1);
        seats.leave(1);
        assert_eq!(seats.bots(RED), 4);
        assert_eq!(seats.find(1), None);
    }

    #[test]
    fn switching_into_a_team_with_no_open_seat_swaps_places_with_a_bot() {
        let mut seats = Seats::new(true);
        seats.sit(RED, 1);
        // Blue is all bots: the first gives way and moves into the seat left behind.
        assert!(seats.switch(1));
        assert_eq!(seats.find(1), Some((BLUE, 0)));
        assert_eq!(seats.0[RED][0], Seat::Bot);
        assert_eq!((seats.bots(RED), seats.bots(BLUE)), (5, 4));
        assert_eq!(seats.bot_count(), 9);

        // And back: the same swap the other way.
        assert!(seats.switch(1));
        assert_eq!(seats.find(1), Some((RED, 0)));
        assert_eq!((seats.bots(RED), seats.bots(BLUE)), (4, 5));
    }

    #[test]
    fn switching_into_an_open_seat_leaves_the_old_one_open() {
        let mut seats = Seats::new(false);
        seats.sit(RED, 1);
        seats.set_bot(BLUE, 0, true);
        // Blue's open seat comes before its bot's.
        assert!(seats.switch(1));
        assert_eq!(seats.find(1), Some((BLUE, 1)));
        assert_eq!(seats.0[RED][0], Seat::Open);
        assert_eq!(seats.bot_count(), 1);
    }

    #[test]
    fn switching_is_refused_into_a_team_of_five_humans() {
        let mut seats = Seats::new(false);
        for id in 1..=5 {
            seats.sit(BLUE, id);
        }
        seats.sit(RED, 6);
        assert!(!seats.switch(6));
        assert_eq!(seats.find(6), Some((RED, 0)));
    }

    #[test]
    fn only_a_seat_without_a_human_can_be_flipped() {
        let mut seats = Seats::new(false);
        seats.sit(RED, 1);
        assert!(seats.set_bot(RED, 1, true));
        assert!(!seats.set_bot(RED, 1, true));
        assert!(seats.set_bot(RED, 1, false));
        assert!(!seats.set_bot(RED, 1, false));
        assert!(!seats.set_bot(RED, 0, true), "a human holds it");
        assert!(!seats.set_bot(RED, TEAM_SIZE, true), "no such seat");
        assert!(!seats.set_bot(2, 0, true), "no such team");
        assert_eq!(seats.find(1), Some((RED, 0)));
    }

    #[test]
    fn newcomers_go_to_the_team_with_fewer_humans_red_on_a_tie() {
        let mut room = room_with(true);
        assert!(room.seat_newcomer(1));
        assert_eq!(room.seats.find(1), Some((RED, 0)));
        assert!(room.seat_newcomer(2));
        assert_eq!(room.seats.find(2), Some((BLUE, 0)));
        // Level again, and red has no open seat: its first bot gives way.
        assert!(room.seat_newcomer(3));
        assert_eq!(room.seats.find(3), Some((RED, 1)));
        assert_eq!(room.seats.bot_count(), 7);
    }

    #[test]
    fn a_team_with_an_open_seat_comes_before_displacing_a_bot_when_the_humans_are_level() {
        let mut room = room_with(false);
        assert!(room.seat_newcomer(1));
        // The host puts bots in red's seats 2 to 5.
        for slot in 1..TEAM_SIZE {
            room.seats.set_bot(RED, slot, true);
        }
        assert!(room.seat_newcomer(2));
        assert_eq!(room.seats.find(2), Some((BLUE, 0)));
        // Level again: red has no open seat and blue has, so no red bot is removed.
        assert!(room.seat_newcomer(3));
        assert_eq!(room.seats.find(3), Some((BLUE, 1)));
        assert_eq!(room.seats.bots(RED), 4);
    }

    #[test]
    fn fewer_humans_decide_even_when_a_bot_has_to_give_way() {
        let mut room = room_with(false);
        room.seats.sit(RED, 1);
        room.seats.sit(RED, 2);
        room.seats.sit(BLUE, 3);
        for slot in 1..TEAM_SIZE {
            room.seats.set_bot(BLUE, slot, true);
        }
        assert!(room.seat_newcomer(4));
        assert_eq!(room.seats.find(4), Some((BLUE, 1)));
        assert_eq!(room.seats.bots(BLUE), 3);
    }

    #[test]
    fn a_room_of_ten_humans_seats_no_eleventh() {
        let mut room = room_with(false);
        for id in 1..=10 {
            assert!(room.seat_newcomer(id));
        }
        assert_eq!((room.seats.humans(RED), room.seats.humans(BLUE)), (5, 5));
        assert!(!room.seat_newcomer(11));
    }

    // ---------------------------------------------------------------- the hub

    /// A hub with connected clients that are only inboxes.
    struct Harness {
        hub: Hub,
        inboxes: HashMap<u32, Receiver<Vec<u8>>>,
    }

    impl Harness {
        /// Every id has connected and said hello.
        fn new(ids: &[u32]) -> Self {
            let mut harness = Self {
                hub: Hub::default(),
                inboxes: HashMap::new(),
            };
            for &id in ids {
                let (tx, rx) = channel(1024);
                harness.hub.handle(Inbound::Connect { id, tx });
                harness.inboxes.insert(id, rx);
                harness.send(
                    id,
                    In::Hello(pb::Hello {
                        name: format!("p{id}"),
                    }),
                );
            }
            harness.forget();
            harness
        }

        fn send(&mut self, id: u32, payload: In) {
            let msg = pb::ClientMessage {
                payload: Some(payload),
            };
            self.hub.handle(Inbound::Message { id, msg });
        }

        /// What `id` has been sent since it last looked.
        fn take(&mut self, id: u32) -> Vec<Out> {
            let inbox = self.inboxes.get_mut(&id).unwrap();
            let mut heard = Vec::new();
            while let Ok(bytes) = inbox.try_recv() {
                heard.push(
                    pb::ServerMessage::decode(bytes.as_slice())
                        .unwrap()
                        .payload
                        .unwrap(),
                );
            }
            heard
        }

        fn forget(&mut self) {
            for inbox in self.inboxes.values_mut() {
                while inbox.try_recv().is_ok() {}
            }
        }

        /// Who plays in the match running in a room.
        fn roster(&mut self, room_id: u32) -> Vec<pb::RosterEntry> {
            let room = self.hub.rooms.get_mut(&room_id).unwrap();
            room.game.as_mut().unwrap().roster()
        }
    }

    fn create(fill_bots: bool) -> In {
        In::CreateRoom(pb::CreateRoom {
            fill_bots,
            ..Default::default()
        })
    }

    fn join(room_id: u32) -> In {
        In::JoinRoom(pb::JoinRoom { room_id })
    }

    fn flip(team: pb::Team, slot: usize, bot: bool) -> In {
        In::SetBotSlot(pb::SetBotSlot {
            team: team as i32,
            slot: slot as u32,
            bot,
        })
    }

    fn start() -> In {
        In::StartGame(pb::StartGame {})
    }

    fn states(heard: Vec<Out>) -> Vec<pb::RoomState> {
        heard
            .into_iter()
            .filter_map(|m| match m {
                Out::RoomState(state) => Some(state),
                _ => None,
            })
            .collect()
    }

    fn lists(heard: Vec<Out>) -> Vec<pb::RoomList> {
        heard
            .into_iter()
            .filter_map(|m| match m {
                Out::RoomList(list) => Some(list),
                _ => None,
            })
            .collect()
    }

    fn errors(heard: Vec<Out>) -> Vec<String> {
        heard
            .into_iter()
            .filter_map(|m| match m {
                Out::Error(error) => Some(error.message),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_room_seats_its_host_first_and_has_bots_only_if_asked() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(true));
        h.send(2, create(false));
        let with_bots = &h.hub.rooms[&1].seats;
        assert_eq!(with_bots.find(1), Some((RED, 0)));
        assert_eq!((with_bots.bots(RED), with_bots.bots(BLUE)), (4, 5));
        let without_bots = &h.hub.rooms[&2].seats;
        assert_eq!(without_bots.find(2), Some((RED, 0)));
        assert_eq!(without_bots.bot_count(), 0);

        // What an idle client sees in the room list.
        let list = lists(h.take(3)).pop().unwrap();
        assert_eq!(
            list.rooms.iter().map(|r| r.bot_count).collect::<Vec<_>>(),
            [9, 0]
        );
    }

    #[test]
    fn the_host_flips_seats_and_a_repeat_tells_nobody_anything() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(false));
        h.send(2, join(1));
        h.forget();

        // An open seat gets a bot: the room and the lobby hear it.
        h.send(1, flip(pb::Team::Red, 3, true));
        for id in [1, 2] {
            let seen = states(h.take(id));
            assert_eq!(seen.len(), 1);
            assert_eq!(
                seen[0].bots,
                [pb::RoomBot {
                    team: pb::Team::Red as i32,
                    slot: 3
                }]
            );
            assert_eq!(seen[0].room.as_ref().unwrap().bot_count, 1);
        }
        assert_eq!(lists(h.take(3)).pop().unwrap().rooms[0].bot_count, 1);

        // Asking for what already is changes nothing and says nothing.
        h.send(1, flip(pb::Team::Red, 3, true));
        h.send(1, flip(pb::Team::Red, 2, false));
        for id in [1, 2, 3] {
            assert!(h.take(id).is_empty(), "client {id} was told something");
        }

        // The bot comes out again.
        h.send(1, flip(pb::Team::Red, 3, false));
        assert!(states(h.take(2)).pop().unwrap().bots.is_empty());
        assert_eq!(lists(h.take(3)).pop().unwrap().rooms[0].bot_count, 0);
        h.forget();

        // Not the host: told so, nothing else happens.
        h.send(2, flip(pb::Team::Red, 1, true));
        assert_eq!(errors(h.take(2)), ["방장만 봇 자리를 바꿀 수 있습니다."]);
        // A player's seat, a seat that isn't there and no team at all are ignored.
        h.send(1, flip(pb::Team::Red, 0, true));
        h.send(1, flip(pb::Team::Blue, 0, false));
        h.send(1, flip(pb::Team::Red, TEAM_SIZE, true));
        h.send(1, flip(pb::Team::Unspecified, 0, true));
        for id in [1, 2, 3] {
            assert!(h.take(id).is_empty(), "client {id} was told something");
        }
    }

    #[test]
    fn switching_into_a_full_team_swaps_places_with_a_bot_and_the_lobby_hears_nothing() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(true));
        h.send(2, join(1));
        assert_eq!(h.hub.rooms[&1].seats.find(2), Some((BLUE, 0)));
        h.forget();

        // Red has no open seat: its first bot gives way and takes the guest's old seat, so it
        // is still five against five.
        h.send(2, In::SwitchTeam(pb::SwitchTeam {}));
        let seats = &h.hub.rooms[&1].seats;
        assert_eq!(seats.find(2), Some((RED, 1)));
        assert_eq!(seats.0[BLUE][0], Seat::Bot);
        let size = |team| seats.humans(team) + seats.bots(team);
        assert_eq!((size(RED), size(BLUE)), (5, 5));
        assert_eq!(states(h.take(1)).len(), 1);
        assert!(h.take(3).is_empty(), "no player or bot came or went");

        // With an open seat on the other side, that is taken and the old seat is left open.
        h.send(1, flip(pb::Team::Blue, 0, false));
        h.forget();
        h.send(2, In::SwitchTeam(pb::SwitchTeam {}));
        let seats = &h.hub.rooms[&1].seats;
        assert_eq!(seats.find(2), Some((BLUE, 0)));
        assert_eq!(seats.0[RED][1], Seat::Open);
        assert!(h.take(3).is_empty());
    }

    #[test]
    fn switching_into_a_team_of_five_humans_is_refused() {
        let mut h = Harness::new(&[1, 2, 3, 4, 5, 6]);
        h.send(1, create(false));
        for id in 2..=6 {
            h.send(id, join(1));
        }
        // Joining alternates, which leaves red with 1, 3 and 5: move two of them over.
        for id in [3, 5] {
            h.send(id, In::SwitchTeam(pb::SwitchTeam {}));
        }
        assert_eq!(h.hub.rooms[&1].seats.humans(BLUE), 5);
        h.forget();
        h.send(1, In::SwitchTeam(pb::SwitchTeam {}));
        assert_eq!(errors(h.take(1)), ["상대 팀이 가득 찼습니다."]);
        assert_eq!(h.hub.rooms[&1].seats.find(1), Some((RED, 0)));
    }

    #[test]
    fn a_match_needs_someone_on_each_team_and_gets_the_bots_its_seats_hold() {
        let mut h = Harness::new(&[1]);
        h.send(1, create(false));
        h.forget();
        h.send(1, start());
        assert_eq!(
            errors(h.take(1)),
            ["양 팀에 사람이나 봇이 최소 1명씩 있어야 시작할 수 있습니다."]
        );
        assert!(h.hub.rooms[&1].game.is_none());

        // Two bots on red, three on blue.
        for (team, slot) in [
            (pb::Team::Red, 1),
            (pb::Team::Red, 2),
            (pb::Team::Blue, 0),
            (pb::Team::Blue, 1),
            (pb::Team::Blue, 2),
        ] {
            h.send(1, flip(team, slot, true));
        }
        h.send(1, start());
        let roster = h.roster(1);
        let on = |team: pb::Team, bots: bool| {
            roster
                .iter()
                .filter(|r| r.team == team as i32 && r.is_bot == bots)
                .count()
        };
        assert_eq!((on(pb::Team::Red, false), on(pb::Team::Red, true)), (1, 2));
        assert_eq!(
            (on(pb::Team::Blue, false), on(pb::Team::Blue, true)),
            (0, 3)
        );

        // Once it runs the seats stay as they are.
        h.forget();
        h.send(1, flip(pb::Team::Red, 3, true));
        assert!(h.take(1).is_empty());
        assert_eq!(h.hub.rooms[&1].seats.bots(RED), 2);
    }

    #[test]
    fn two_players_can_play_a_match_without_any_bot() {
        // The room made with the checkbox off: every seat is open, so the guest takes blue's first.
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create(false));
        h.send(2, join(1));
        h.send(1, start());
        let roster = h.roster(1);
        assert_eq!(roster.len(), 2);
        assert!(roster.iter().all(|r| !r.is_bot));
    }

    #[test]
    fn a_side_that_holds_only_bots_is_enough_to_start() {
        let mut h = Harness::new(&[1]);
        h.send(1, create(false));
        h.send(1, flip(pb::Team::Blue, 0, true));
        h.forget();
        h.send(1, start());
        assert!(errors(h.take(1)).is_empty());
        let roster = h.roster(1);
        assert_eq!(roster.len(), 2);
        assert!(roster
            .iter()
            .any(|r| r.is_bot && r.team == pb::Team::Blue as i32));
    }

    #[test]
    fn an_eleventh_player_cannot_join() {
        let ids: Vec<u32> = (1..=11).collect();
        let mut h = Harness::new(&ids);
        h.send(1, create(false));
        for id in 2..=10 {
            h.send(id, join(1));
        }
        assert_eq!(h.hub.rooms[&1].members.len(), 10);
        h.forget();
        h.send(11, join(1));
        assert_eq!(errors(h.take(11)), ["방이 가득 찼습니다."]);
        assert_eq!(h.hub.rooms[&1].members.len(), 10);
        assert_eq!(h.hub.clients[&11].room, None);
    }

    #[test]
    fn the_next_member_hosts_when_the_host_leaves_and_the_old_seat_opens() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(true));
        h.send(2, join(1));
        h.send(3, join(1));
        h.forget();

        h.send(1, In::LeaveRoom(pb::LeaveRoom {}));
        assert_eq!(h.hub.rooms[&1].host, 2);
        assert_eq!(h.hub.rooms[&1].seats.0[RED][0], Seat::Open);
        for id in [2, 3] {
            let state = states(h.take(id)).pop().unwrap();
            assert!(state.players.iter().all(|p| p.is_host == (p.id == 2)));
        }

        // The new host can flip seats, and the other member still can't.
        h.send(3, flip(pb::Team::Red, 0, true));
        assert_eq!(errors(h.take(3)), ["방장만 봇 자리를 바꿀 수 있습니다."]);
        h.send(2, flip(pb::Team::Red, 0, true));
        assert_eq!(h.hub.rooms[&1].seats.0[RED][0], Seat::Bot);
        assert_eq!(states(h.take(3)).len(), 1);
    }

    #[test]
    fn a_disconnect_frees_the_seat() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(false));
        h.send(2, join(1));
        h.forget();

        h.hub.handle(Inbound::Disconnect { id: 2 });
        let room = &h.hub.rooms[&1];
        assert_eq!(room.members.len(), 1);
        assert_eq!(room.seats.find(2), None);
        assert_eq!(room.seats.0[BLUE][0], Seat::Open);
        assert!(!h.hub.clients.contains_key(&2));
        // The room hears it, and the lobby sees one player there.
        assert_eq!(states(h.take(1)).pop().unwrap().players.len(), 1);
        assert_eq!(lists(h.take(3)).pop().unwrap().rooms[0].player_count, 1);
    }

    #[test]
    fn a_player_who_leaves_mid_match_is_not_replaced() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create(true));
        h.send(2, join(1));
        h.send(1, start());
        assert_eq!(h.roster(1).len(), 10);

        h.send(2, In::LeaveRoom(pb::LeaveRoom {}));
        let roster = h.roster(1);
        assert_eq!(roster.len(), 9);
        assert_eq!(roster.iter().filter(|r| r.is_bot).count(), 8);
        // Blue still has its bots, so the match goes on, and the leaver's seat is open.
        assert!(h.hub.rooms[&1].game.as_ref().unwrap().winner().is_none());
        assert_eq!(h.hub.rooms[&1].seats.0[BLUE][0], Seat::Open);
    }

    #[test]
    fn a_team_that_loses_its_last_player_forfeits_and_the_room_goes_back_to_waiting() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create(false));
        h.send(2, join(1));
        // Four bots beside the host on red; blue is only the guest.
        for slot in 1..TEAM_SIZE {
            h.send(1, flip(pb::Team::Red, slot, true));
        }
        h.send(1, start());
        h.forget();

        h.send(2, In::LeaveRoom(pb::LeaveRoom {}));
        h.hub.tick();
        let end = h
            .take(1)
            .into_iter()
            .find_map(|m| match m {
                Out::MatchEnd(end) => Some(end),
                _ => None,
            })
            .expect("the host is told the match is over");
        assert_eq!(end.winner, pb::Team::Red as i32);
        assert_eq!(h.roster(1).len(), 5, "no bot took the leaver's place");

        // The results stay up for a while, then the room is back with its seats as they were set.
        let mut ticks = 0;
        while h.hub.rooms[&1].game.is_some() {
            h.hub.tick();
            ticks += 1;
            assert!(ticks <= 10 * TICK_RATE, "the room never left the results");
        }
        let back = states(h.take(1)).pop().unwrap();
        assert_eq!(back.room.unwrap().phase, pb::RoomPhase::Waiting as i32);
        let seats = &h.hub.rooms[&1].seats;
        assert_eq!(seats.find(1), Some((RED, 0)));
        assert_eq!(seats.0[BLUE], [Seat::Open; TEAM_SIZE]);
        assert_eq!(seats.bots(RED), 4);
    }
}
