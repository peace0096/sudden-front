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
/// The game modes and maps a room can be set to.
const MODES: [pb::GameMode; 2] = [pb::GameMode::Normal, pb::GameMode::Augment];
const MAPS: [pb::MapId; 2] = [pb::MapId::SupplyYard, pb::MapId::Warehouse];
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
    /// Pressed ready in the waiting room. The host's is never set: they start the match instead.
    ready: bool,
}

/// Who holds a seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seat {
    Open,
    Bot,
    Human(u32),
}

/// A room's seats: five on each team, indexed `[red, blue][slot]`. A seat keeps its position,
/// so when someone leaves it just becomes open and nobody shifts. A free-for-all room has no
/// teams and numbers the same seats 0 to 9 instead: seat `i` is `[i / 5][i % 5]`.
#[derive(Debug)]
struct Seats([[Seat; TEAM_SIZE]; 2]);

// A free-for-all room's seats are exactly the two teams' seats.
const _: () = assert!(MAX_PLAYERS == 2 * TEAM_SIZE);

impl Seats {
    /// Every seat starts as a bot, or open.
    fn new(bots: bool) -> Self {
        Self([[if bots { Seat::Bot } else { Seat::Open }; TEAM_SIZE]; 2])
    }

    /// `(team, slot)` of the seat a free-for-all room numbers `index`, if there is one.
    fn at(index: usize) -> Option<(usize, usize)> {
        (index < MAX_PLAYERS).then_some((index / TEAM_SIZE, index % TEAM_SIZE))
    }

    /// Every seat as `(team, slot, seat)`, red first, which is also the order of the
    /// free-for-all numbering.
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

    /// Seats `id` in the first open seat of `team`; no bot gives way. `None` when it has none.
    fn sit_open(&mut self, team: usize, id: u32) -> Option<usize> {
        let slot = self.0[team].iter().position(|seat| *seat == Seat::Open)?;
        self.0[team][slot] = Seat::Human(id);
        Some(slot)
    }

    /// Seats `id` in a free-for-all room: the first open seat by number, or else the first bot's,
    /// which the bot gives up. Returns the seat's number; `None` when every seat holds a human.
    fn sit_first(&mut self, id: u32) -> Option<usize> {
        let index = [Seat::Open, Seat::Bot]
            .into_iter()
            .find_map(|kind| self.iter().position(|(_, _, seat)| seat == kind))?;
        let (team, slot) = Self::at(index)?;
        self.0[team][slot] = Seat::Human(id);
        Some(index)
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
    /// Fixed when the room is made: the seats mean something else in each.
    match_type: pb::MatchType,
    kills_to_win: u32,
    seats: Seats,
    /// Everyone in the room, longest first: the first takes over as host. Each holds a seat.
    members: Vec<Member>,
    game: Option<Match>,
    results_left: Option<f32>,
}

impl Room {
    fn ffa(&self) -> bool {
        self.match_type == pb::MatchType::FreeForAll
    }

    /// Seats a newcomer on the team with fewer humans, or else on the other one. When the humans
    /// are level, a team with an open seat is preferred. A free-for-all room takes the first open
    /// seat, or else the first bot's. `false` when every seat a newcomer could take holds a human.
    fn seat_newcomer(&mut self, id: u32) -> bool {
        if self.ffa() {
            return self.seats.sit_first(id).is_some();
        }
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

    /// Seats a newcomer to a running match, in an open seat only: bots keep theirs until the match
    /// is over. A team room takes the team with fewer players, bots included, then the one with
    /// fewer humans, then red. A free-for-all room takes the first open seat by number. Returns the
    /// team they play for, `Unspecified` in a free-for-all; `None` when no seat is open.
    fn seat_mid_match(&mut self, id: u32) -> Option<pb::Team> {
        if self.ffa() {
            // Red's seats are the first in the numbering.
            (0..2).find_map(|team| self.seats.sit_open(team, id))?;
            return Some(pb::Team::Unspecified);
        }
        let seats = &self.seats;
        let team = (0..2)
            .filter(|&team| seats.has_open(team))
            .min_by_key(|&team| (seats.humans(team) + seats.bots(team), seats.humans(team)))?;
        self.seats.sit_open(team, id)?;
        Some(TEAMS[team])
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
            In::SetGameMode(set) => self.on_set_game_mode(id, set.mode),
            In::SetMap(set) => self.on_set_map(id, set.map),
            In::SetBotSlot(set) => self.on_set_bot_slot(id, set),
            In::SetReady(set) => self.on_set_ready(id, set.ready),
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
        let match_type = match pb::MatchType::try_from(create.match_type) {
            Ok(pb::MatchType::FreeForAll) => pb::MatchType::FreeForAll,
            _ => pb::MatchType::TeamDeathmatch,
        };
        // The host takes the first seat: red's first, which is number 0 in a free-for-all room.
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
                match_type,
                kills_to_win: if match_type == pb::MatchType::FreeForAll {
                    crate::game::KILLS_TO_WIN_FFA
                } else {
                    crate::game::KILLS_TO_WIN
                },
                seats,
                members: vec![Member {
                    id,
                    weapon: pb::WeaponKind::Rifle,
                    ready: false,
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
            self.join_running_match(id, room_id);
            return;
        }
        if room.members.len() >= MAX_PLAYERS || !room.seat_newcomer(id) {
            self.error(id, "방이 가득 찼습니다.");
            return;
        }
        room.members.push(Member {
            id,
            weapon: pb::WeaponKind::Rifle,
            ready: false,
        });
        self.clients.get_mut(&id).unwrap().room = Some(room_id);
        self.send_room_state(room_id);
        self.broadcast_room_list();
    }

    /// Joins the match a room is playing, in a seat nobody holds. The newcomer goes straight into
    /// the match and everyone already in hears who is new; at the end they are all back in the
    /// waiting room together.
    fn join_running_match(&mut self, id: u32, room_id: u32) {
        let Some(name) = self.clients.get(&id).map(|c| c.name.clone()) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        let Some(game) = room.game.as_ref() else {
            return;
        };
        // Over: the results are up, or will be at the next tick after a forfeit.
        if room.results_left.is_some() || game.winner().is_some() {
            self.error(id, "경기가 끝나는 중입니다. 잠시 후 다시 들어오세요.");
            return;
        }
        if room.members.len() >= MAX_PLAYERS {
            self.error(id, "방이 가득 찼습니다.");
            return;
        }
        let Some(team) = room.seat_mid_match(id) else {
            self.error(id, "빈 자리가 없습니다.");
            return;
        };
        let weapon = pb::WeaponKind::Rifle;
        room.members.push(Member {
            id,
            weapon,
            ready: false,
        });
        let others: Vec<u32> = room.members.iter().map(|m| m.id).filter(|&m| m != id).collect();
        let game = room.game.as_mut().unwrap();
        let roster = game.add_player(id, name, team, weapon);
        let start = game.start_message(id);
        self.clients.get_mut(&id).unwrap().room = Some(room_id);
        self.send(id, Out::MatchStart(start));
        for member in others {
            self.send(
                member,
                Out::RosterUpdate(pb::RosterUpdate {
                    roster: roster.clone(),
                }),
            );
        }
        self.broadcast_room_list();
    }

    /// A player other than the host says they are ready, or no longer ready, in the waiting room.
    fn on_set_ready(&mut self, id: u32, ready: bool) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        // The host starts the match rather than readying, and nothing changes while one runs.
        if room.host == id || room.game.is_some() {
            return;
        }
        let Some(member) = room.members.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if member.ready == ready {
            return;
        }
        member.ready = ready;
        self.send_room_state(room_id);
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
        if room.ffa() {
            self.error(id, "개인전에는 팀이 없습니다.");
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

    /// The host picks the next match's rules in the waiting room: normal or augment mode.
    fn on_set_game_mode(&mut self, id: u32, mode: i32) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        if room.host != id {
            self.error(id, "방장만 게임 모드를 바꿀 수 있습니다.");
            return;
        }
        let Some(mode) = pb::GameMode::try_from(mode).ok().filter(|mode| MODES.contains(mode)) else {
            return;
        };
        // As for the kill target: a repeat would only re-broadcast the room list.
        if room.game.is_some() || room.mode == mode {
            return;
        }
        room.mode = mode;
        self.send_room_state(room_id);
        self.broadcast_room_list();
    }

    /// The host picks the next match's arena in the waiting room.
    fn on_set_map(&mut self, id: u32, map: i32) {
        let Some(room_id) = self.clients.get(&id).and_then(|c| c.room) else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        if room.host != id {
            self.error(id, "방장만 맵을 바꿀 수 있습니다.");
            return;
        }
        let Some(map) = pb::MapId::try_from(map).ok().filter(|map| MAPS.contains(map)) else {
            return;
        };
        if room.game.is_some() || room.map == map {
            return;
        }
        room.map = map;
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
        // A free-for-all room's seats have no team, only a number; a team room's need a team.
        let seat = if room.ffa() {
            if set.team != pb::Team::Unspecified as i32 {
                return;
            }
            Seats::at(set.slot as usize)
        } else {
            TEAMS
                .iter()
                .position(|team| *team as i32 == set.team)
                .map(|team| (team, set.slot as usize))
        };
        let Some((team, slot)) = seat else {
            return;
        };
        // A seat no one can change, or already as asked, must not re-broadcast the room list.
        if room.game.is_some() || !room.seats.set_bot(team, slot, set.bot) {
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
        let ffa = room.ffa();
        if ffa {
            if room.seats.iter().filter(|(_, _, seat)| *seat != Seat::Open).count() < 2 {
                self.error(id, "개인전은 사람이나 봇이 2명 이상 있어야 시작할 수 있습니다.");
                return;
            }
        } else if (0..2).any(|team| room.seats.humans(team) + room.seats.bots(team) == 0) {
            self.error(
                id,
                "양 팀에 사람이나 봇이 최소 1명씩 있어야 시작할 수 있습니다.",
            );
            return;
        }
        // Everyone but the host must be ready; bots always are.
        let waiting: Vec<String> = room
            .members
            .iter()
            .filter(|m| m.id != room.host && !m.ready)
            .map(|m| self.clients.get(&m.id).map(|c| c.name.clone()).unwrap_or_default())
            .collect();
        if !waiting.is_empty() {
            self.error(
                id,
                &format!("모든 플레이어가 준비해야 시작할 수 있습니다. (준비 안 함: {})", waiting.join(", ")),
            );
            return;
        }

        let humans: Vec<(u32, String, pb::Team, pb::WeaponKind)> = room
            .members
            .iter()
            .filter_map(|m| {
                let (team, _) = room.seats.find(m.id)?;
                let client = self.clients.get(&m.id)?;
                let team = if ffa { pb::Team::Unspecified } else { TEAMS[team] };
                Some((m.id, client.name.clone(), team, m.weapon))
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
        let mut game = Match::with_rules(&humans, map, room.mode, room.match_type, bots, seed);
        game.set_kills_to_win(room.kills_to_win);
        let starts: Vec<(u32, pb::MatchStart)> = humans
            .iter()
            .map(|(pid, _, _, _)| (*pid, game.start_message(*pid)))
            .collect();
        let room = self.rooms.get_mut(&room_id).unwrap();
        room.game = Some(game);
        // Readiness is spent on this match: back in the waiting room, everyone readies again.
        for member in room.members.iter_mut() {
            member.ready = false;
        }
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
            .map(|_| encode(Out::MatchEnd(game.end_message(RESULTS_SECONDS))));
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
            match_type: room.match_type as i32,
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
        for (index, (team, slot, seat)) in room.seats.iter().enumerate() {
            // A free-for-all room's seats go by number, with no team.
            let (team, slot) = if room.ffa() {
                (pb::Team::Unspecified as i32, index as u32)
            } else {
                (TEAMS[team] as i32, slot as u32)
            };
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
                    ready: id != room.host && room.members.iter().any(|m| m.id == id && m.ready),
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
            match_type: pb::MatchType::TeamDeathmatch,
            kills_to_win: crate::game::KILLS_TO_WIN,
            seats: Seats::new(bots),
            members: Vec::new(),
            game: None,
            results_left: None,
        }
    }

    fn ffa_room_with(bots: bool) -> Room {
        Room {
            match_type: pb::MatchType::FreeForAll,
            kills_to_win: crate::game::KILLS_TO_WIN_FFA,
            ..room_with(bots)
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

    #[test]
    fn free_for_all_seats_are_numbered_red_then_blue() {
        assert_eq!(Seats::at(0), Some((RED, 0)));
        assert_eq!(Seats::at(4), Some((RED, 4)));
        assert_eq!(Seats::at(7), Some((BLUE, 2)));
        assert_eq!(Seats::at(9), Some((BLUE, 4)));
        assert_eq!(Seats::at(10), None);
    }

    #[test]
    fn a_free_for_all_newcomer_takes_the_first_open_seat_before_any_bot() {
        let mut room = ffa_room_with(true);
        // The host takes bots out of seats 7 and 3.
        room.seats.set_bot(BLUE, 2, false);
        room.seats.set_bot(RED, 3, false);
        for id in 1..=3 {
            assert!(room.seat_newcomer(id));
        }
        assert_eq!(room.seats.find(1), Seats::at(3));
        assert_eq!(room.seats.find(2), Seats::at(7));
        // No open seat is left: the first bot gives way, whatever team its seat was on.
        assert_eq!(room.seats.find(3), Seats::at(0));
        assert_eq!(room.seats.bot_count(), 7);
    }

    #[test]
    fn a_free_for_all_room_of_ten_humans_seats_them_in_order_and_no_eleventh() {
        let mut room = ffa_room_with(false);
        for id in 1..=10 {
            assert!(room.seat_newcomer(id));
            assert_eq!(room.seats.find(id), Seats::at(id as usize - 1));
        }
        assert!(!room.seat_newcomer(11));
    }

    #[test]
    fn a_running_match_seats_a_newcomer_on_the_team_with_fewer_players_then_fewer_humans_then_red() {
        let mut room = room_with(false);
        room.seats.sit(RED, 1);
        room.seats.sit(BLUE, 2);
        // Level in players and in humans: red.
        assert_eq!(room.seat_mid_match(3), Some(pb::Team::Red));
        assert_eq!(room.seats.find(3), Some((RED, 1)));

        // Three bots beside blue's human: blue has fewer humans, but red has fewer players.
        for slot in 1..4 {
            room.seats.set_bot(BLUE, slot, true);
        }
        assert_eq!(room.seat_mid_match(4), Some(pb::Team::Red));
        assert_eq!(room.seat_mid_match(5), Some(pb::Team::Red));
        assert_eq!(room.seats.find(5), Some((RED, 3)));
        // Four players each: the team with fewer humans.
        assert_eq!(room.seat_mid_match(6), Some(pb::Team::Blue));
        assert_eq!(room.seats.find(6), Some((BLUE, 4)));
        assert_eq!(room.seats.bots(BLUE), 3, "no bot gave way");
    }

    #[test]
    fn a_running_match_seats_a_newcomer_only_where_a_seat_is_open() {
        // Humans and bots hold every seat.
        let mut room = room_with(true);
        for id in 1..=3 {
            assert!(room.seat_newcomer(id));
        }
        assert_eq!(room.seat_mid_match(4), None);
        assert_eq!(room.seats.find(4), None);

        // A player leaves, and their seat is the only one a newcomer can take.
        room.seats.leave(2);
        assert_eq!(room.seat_mid_match(4), Some(pb::Team::Blue));
        assert_eq!(room.seats.find(4), Some((BLUE, 0)));
        assert_eq!(room.seat_mid_match(5), None);
        assert_eq!(room.seats.bot_count(), 7, "no bot gave way");
    }

    #[test]
    fn a_running_free_for_all_seats_a_newcomer_in_the_first_open_seat_by_number() {
        let mut room = ffa_room_with(true);
        assert!(room.seat_newcomer(1));
        // The host takes bots out of seats 7 and 3.
        room.seats.set_bot(BLUE, 2, false);
        room.seats.set_bot(RED, 3, false);
        assert_eq!(room.seat_mid_match(2), Some(pb::Team::Unspecified));
        assert_eq!(room.seats.find(2), Seats::at(3));
        assert_eq!(room.seat_mid_match(3), Some(pb::Team::Unspecified));
        assert_eq!(room.seats.find(3), Seats::at(7));
        // Only bots' seats are left.
        assert_eq!(room.seat_mid_match(4), None);
        assert_eq!(room.seats.bot_count(), 7);
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

        /// The snapshot `id` would be sent of the match running in a room, as it stands now.
        fn snapshot(&mut self, room_id: u32, id: u32) -> pb::Snapshot {
            let game = self.hub.rooms.get_mut(&room_id).unwrap().game.as_mut().unwrap();
            let mut snapshot = game.shared_snapshot();
            game.fill_personal(id, &mut snapshot);
            snapshot
        }
    }

    fn create(fill_bots: bool) -> In {
        In::CreateRoom(pb::CreateRoom {
            fill_bots,
            ..Default::default()
        })
    }

    fn create_ffa(fill_bots: bool) -> In {
        In::CreateRoom(pb::CreateRoom {
            fill_bots,
            match_type: pb::MatchType::FreeForAll as i32,
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

    fn ready(on: bool) -> In {
        In::SetReady(pb::SetReady { ready: on })
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
        h.send(2, ready(true));
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

        // Nor once their match runs.
        for id in 2..=10 {
            h.send(id, ready(true));
        }
        h.send(1, start());
        h.forget();
        h.send(11, join(1));
        assert_eq!(errors(h.take(11)), ["방이 가득 찼습니다."]);
        assert_eq!(h.roster(1).len(), 10);
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
        h.send(2, ready(true));
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
        h.send(2, ready(true));
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
        assert_eq!((end.winner, end.winner_id), (pb::Team::Red as i32, 0));
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

    // ---------------------------------------------------------------- readiness

    #[test]
    fn the_host_starts_only_once_every_other_player_is_ready() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(true));
        h.send(2, join(1));
        h.send(3, join(1));
        h.forget();
        h.send(1, start());
        assert_eq!(errors(h.take(1)), ["모든 플레이어가 준비해야 시작할 수 있습니다. (준비 안 함: p2, p3)"]);
        assert!(h.hub.rooms[&1].game.is_none());

        h.send(2, ready(true));
        h.forget();
        h.send(1, start());
        assert_eq!(errors(h.take(1)), ["모든 플레이어가 준비해야 시작할 수 있습니다. (준비 안 함: p3)"]);

        // Readying and then taking it back counts as not ready.
        h.send(3, ready(true));
        h.send(3, ready(false));
        h.forget();
        h.send(1, start());
        assert_eq!(errors(h.take(1)), ["모든 플레이어가 준비해야 시작할 수 있습니다. (준비 안 함: p3)"]);

        h.send(3, ready(true));
        h.forget();
        h.send(1, start());
        assert!(errors(h.take(1)).is_empty());
        assert!(h.hub.rooms[&1].game.is_some());
        // Readiness is spent on the match: everyone readies again for the next one.
        assert!(h.hub.rooms[&1].members.iter().all(|m| !m.ready));
    }

    #[test]
    fn the_room_shows_who_is_ready_and_the_host_never_is() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create(false));
        h.send(2, join(1));
        h.forget();

        h.send(2, ready(true));
        let shown = |state: pb::RoomState| state.players.iter().map(|p| (p.id, p.ready)).collect::<Vec<_>>();
        assert_eq!(shown(states(h.take(1)).pop().unwrap()), [(1, false), (2, true)]);
        h.forget();
        // Saying it again changes nothing, so nobody is told.
        h.send(2, ready(true));
        assert!(h.take(1).is_empty());

        // The host starts instead of readying.
        h.send(1, ready(true));
        assert!(h.take(2).is_empty());
        assert!(!h.hub.rooms[&1].members[0].ready);

        // A host who takes over keeps no readiness of their own.
        h.send(1, In::LeaveRoom(pb::LeaveRoom {}));
        let state = states(h.take(2)).pop().unwrap();
        assert_eq!(shown(state), [(2, false)]);
    }

    // ---------------------------------------------------------------- mode and map

    fn set_mode(mode: i32) -> In {
        In::SetGameMode(pb::SetGameMode { mode })
    }

    fn set_map(map: i32) -> In {
        In::SetMap(pb::SetMap { map })
    }

    const AUGMENT: i32 = pb::GameMode::Augment as i32;
    const WAREHOUSE: i32 = pb::MapId::Warehouse as i32;

    #[test]
    fn the_host_sets_the_mode_and_the_map_in_the_waiting_room_and_the_match_is_played_so() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(true));
        h.send(2, join(1));
        h.forget();

        // The room and the lobby hear each change.
        h.send(1, set_mode(AUGMENT));
        h.send(1, set_map(WAREHOUSE));
        for id in [1, 2] {
            let room = states(h.take(id)).pop().unwrap().room.unwrap();
            assert_eq!((room.mode, room.map), (AUGMENT, WAREHOUSE));
        }
        let listed = lists(h.take(3)).pop().unwrap().rooms.pop().unwrap();
        assert_eq!((listed.mode, listed.map), (AUGMENT, WAREHOUSE));

        // What already is, UNSPECIFIED and values this server doesn't know change nothing and say nothing.
        for set in [set_mode(AUGMENT), set_map(WAREHOUSE), set_mode(0), set_mode(99), set_map(0), set_map(99)] {
            h.send(1, set);
        }
        for id in [1, 2, 3] {
            assert!(h.take(id).is_empty(), "client {id} was told something");
        }

        // A guest is told it is the host's to choose.
        h.send(2, set_mode(pb::GameMode::Normal as i32));
        h.send(2, set_map(pb::MapId::SupplyYard as i32));
        assert_eq!(errors(h.take(2)), ["방장만 게임 모드를 바꿀 수 있습니다.", "방장만 맵을 바꿀 수 있습니다."]);
        assert_eq!((h.hub.rooms[&1].mode, h.hub.rooms[&1].map), (pb::GameMode::Augment, pb::MapId::Warehouse));

        h.send(2, ready(true));
        h.forget();
        h.send(1, start());
        let start = h
            .take(1)
            .into_iter()
            .find_map(|m| match m {
                Out::MatchStart(start) => Some(start),
                _ => None,
            })
            .expect("the match starts");
        assert_eq!(start.config.unwrap().mode, AUGMENT);
        assert_eq!(start.map.unwrap().id, WAREHOUSE);

        // Nothing changes while the match runs.
        h.forget();
        h.send(1, set_mode(pb::GameMode::Normal as i32));
        h.send(1, set_map(pb::MapId::SupplyYard as i32));
        for id in [1, 3] {
            assert!(h.take(id).is_empty(), "client {id} was told something");
        }
        assert_eq!((h.hub.rooms[&1].mode, h.hub.rooms[&1].map), (pb::GameMode::Augment, pb::MapId::Warehouse));
    }

    // ---------------------------------------------------------------- free-for-all rooms

    const NO_TEAM: i32 = pb::Team::Unspecified as i32;

    fn match_end(heard: Vec<Out>) -> Option<pb::MatchEnd> {
        heard.into_iter().find_map(|m| match m {
            Out::MatchEnd(end) => Some(end),
            _ => None,
        })
    }

    #[test]
    fn a_free_for_all_room_has_20_kills_and_numbered_seats_without_teams() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create_ffa(true));
        h.send(2, join(1));
        let state = states(h.take(2)).pop().unwrap();
        let room = state.room.unwrap();
        assert_eq!(
            (room.match_type, room.kills_to_win),
            (pb::MatchType::FreeForAll as i32, crate::game::KILLS_TO_WIN_FFA)
        );
        let players: Vec<(u32, i32, u32)> = state.players.iter().map(|p| (p.id, p.team, p.slot)).collect();
        assert_eq!(players, [(1, NO_TEAM, 0), (2, NO_TEAM, 1)]);
        let bots: Vec<(i32, u32)> = state.bots.iter().map(|b| (b.team, b.slot)).collect();
        assert_eq!(bots, (2..10).map(|slot| (NO_TEAM, slot)).collect::<Vec<_>>());

        // The lobby's list says what kind of room it is.
        let listed = lists(h.take(3)).pop().unwrap().rooms.pop().unwrap();
        assert_eq!(
            (listed.match_type, listed.kills_to_win),
            (pb::MatchType::FreeForAll as i32, crate::game::KILLS_TO_WIN_FFA)
        );
    }

    #[test]
    fn a_room_is_a_team_room_unless_made_a_free_for_all() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create(false));
        // A match type this server doesn't know.
        h.send(
            2,
            In::CreateRoom(pb::CreateRoom {
                match_type: 99,
                ..Default::default()
            }),
        );
        for id in [1, 2] {
            let room = states(h.take(id)).pop().unwrap().room.unwrap();
            assert_eq!(
                (room.match_type, room.kills_to_win),
                (pb::MatchType::TeamDeathmatch as i32, crate::game::KILLS_TO_WIN)
            );
        }
    }

    #[test]
    fn nobody_can_switch_teams_in_a_free_for_all_room() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create_ffa(false));
        h.send(2, join(1));
        h.forget();
        h.send(2, In::SwitchTeam(pb::SwitchTeam {}));
        assert_eq!(errors(h.take(2)), ["개인전에는 팀이 없습니다."]);
        assert_eq!(h.hub.rooms[&1].seats.find(2), Seats::at(1));
        assert!(h.take(1).is_empty(), "the room heard nothing");
    }

    #[test]
    fn the_host_flips_free_for_all_seats_by_number() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create_ffa(false));
        h.send(2, join(1));
        h.forget();

        h.send(1, flip(pb::Team::Unspecified, 7, true));
        assert_eq!(h.hub.rooms[&1].seats.0[BLUE][2], Seat::Bot);
        let state = states(h.take(2)).pop().unwrap();
        assert_eq!(state.bots, [pb::RoomBot { team: NO_TEAM, slot: 7 }]);
        h.forget();

        // No seat 10, no seat on a team, and no bot in a player's seat.
        h.send(1, flip(pb::Team::Unspecified, MAX_PLAYERS, true));
        h.send(1, flip(pb::Team::Red, 3, true));
        h.send(1, flip(pb::Team::Unspecified, 1, true));
        for id in [1, 2] {
            assert!(h.take(id).is_empty(), "client {id} was told something");
        }
        assert_eq!(h.hub.rooms[&1].seats.bot_count(), 1);

        h.send(1, flip(pb::Team::Unspecified, 7, false));
        assert!(states(h.take(2)).pop().unwrap().bots.is_empty());
    }

    #[test]
    fn a_free_for_all_needs_two_players_or_bots_to_start() {
        let mut h = Harness::new(&[1]);
        h.send(1, create_ffa(false));
        h.forget();
        h.send(1, start());
        assert_eq!(
            errors(h.take(1)),
            ["개인전은 사람이나 봇이 2명 이상 있어야 시작할 수 있습니다."]
        );
        assert!(h.hub.rooms[&1].game.is_none());

        // One bot, in any seat, is enough.
        h.send(1, flip(pb::Team::Unspecified, 9, true));
        h.forget();
        h.send(1, start());
        let start = h
            .take(1)
            .into_iter()
            .find_map(|m| match m {
                Out::MatchStart(start) => Some(start),
                _ => None,
            })
            .expect("the match starts");
        let config = start.config.unwrap();
        assert_eq!(
            (config.match_type, config.kills_to_win),
            (pb::MatchType::FreeForAll as i32, crate::game::KILLS_TO_WIN_FFA)
        );
        let roster = h.roster(1);
        assert_eq!(roster.len(), 2);
        assert!(roster.iter().all(|r| r.team == NO_TEAM));
    }

    #[test]
    fn the_last_player_left_wins_a_free_for_all_and_the_room_can_play_again() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create_ffa(false));
        h.send(2, join(1));
        h.send(2, ready(true));
        h.send(1, start());
        h.forget();

        h.send(2, In::LeaveRoom(pb::LeaveRoom {}));
        h.hub.tick();
        let end = match_end(h.take(1)).expect("the host is told the match is over");
        assert_eq!((end.winner, end.winner_id), (NO_TEAM, 1));

        let mut ticks = 0;
        while h.hub.rooms[&1].game.is_some() {
            h.hub.tick();
            ticks += 1;
            assert!(ticks <= 10 * TICK_RATE, "the room never left the results");
        }
        let back = states(h.take(1)).pop().unwrap();
        assert_eq!(back.room.unwrap().phase, pb::RoomPhase::Waiting as i32);
        let players: Vec<(u32, i32, u32)> = back.players.iter().map(|p| (p.id, p.team, p.slot)).collect();
        assert_eq!(players, [(1, NO_TEAM, 0)]);

        // The guest comes back for a second match, and readies again.
        h.send(2, join(1));
        h.send(2, ready(true));
        h.send(1, start());
        assert_eq!(h.roster(1).len(), 2);
        assert!(h.hub.rooms[&1].game.as_ref().unwrap().winner().is_none());
    }

    #[test]
    fn an_eleventh_player_cannot_join_a_free_for_all_room() {
        let ids: Vec<u32> = (1..=11).collect();
        let mut h = Harness::new(&ids);
        h.send(1, create_ffa(true));
        for id in 2..=10 {
            h.send(id, join(1));
        }
        let seats = &h.hub.rooms[&1].seats;
        assert_eq!(seats.bot_count(), 0, "every bot gave way");
        for id in 1..=10 {
            assert_eq!(seats.find(id), Seats::at(id as usize - 1));
        }
        h.forget();
        h.send(11, join(1));
        assert_eq!(errors(h.take(11)), ["방이 가득 찼습니다."]);
        assert_eq!(h.hub.clients[&11].room, None);
    }

    // ---------------------------------------------------------------- joining a running match

    fn create_augment(fill_bots: bool) -> In {
        In::CreateRoom(pb::CreateRoom {
            fill_bots,
            mode: pb::GameMode::Augment as i32,
            ..Default::default()
        })
    }

    fn match_start(heard: Vec<Out>) -> Option<pb::MatchStart> {
        heard.into_iter().find_map(|m| match m {
            Out::MatchStart(start) => Some(start),
            _ => None,
        })
    }

    fn roster_update(heard: Vec<Out>) -> Option<Vec<pb::RosterEntry>> {
        heard.into_iter().find_map(|m| match m {
            Out::RosterUpdate(update) => Some(update.roster),
            _ => None,
        })
    }

    #[test]
    fn a_newcomer_joins_a_running_match_in_an_open_seat_on_the_team_short_of_players() {
        let mut h = Harness::new(&[1, 2, 3, 4]);
        h.send(1, create(false));
        h.send(2, join(1));
        // Red: the host and three bots. Blue: only the guest.
        for slot in 1..4 {
            h.send(1, flip(pb::Team::Red, slot, true));
        }
        h.send(2, ready(true));
        h.send(1, start());
        assert_eq!(h.roster(1).len(), 5);
        h.forget();

        h.send(3, join(1));
        assert_eq!(h.hub.rooms[&1].seats.find(3), Some((BLUE, 1)));
        assert_eq!(h.hub.clients[&3].room, Some(1));
        assert!(h.hub.rooms[&1].members.iter().any(|m| m.id == 3 && !m.ready));
        // Straight into the match, on blue ...
        let joined = match_start(h.take(3)).expect("the newcomer is sent into the match");
        assert_eq!(joined.your_id, 3);
        assert_eq!(joined.roster.len(), 6);
        assert!(joined
            .roster
            .iter()
            .any(|r| r.id == 3 && r.team == pb::Team::Blue as i32 && !r.is_bot));
        // ... and everyone already in hears who is new, while the lobby sees one more player.
        for id in [1, 2] {
            assert_eq!(roster_update(h.take(id)), Some(joined.roster.clone()));
        }
        assert_eq!(lists(h.take(4)).pop().unwrap().rooms[0].player_count, 3);

        // They play from the next tick: alive, under spawn protection, with nothing scored yet.
        h.hub.tick();
        assert!(h.take(3).iter().any(|m| matches!(m, Out::Snapshot(_))));
        let snapshot = h.snapshot(1, 3);
        let me = snapshot.players.iter().find(|p| p.id == 3).unwrap();
        assert!(me.alive && me.protected);
        assert_eq!((me.kills, me.deaths), (0, 0));
    }

    #[test]
    fn a_running_match_whose_free_seats_all_hold_bots_takes_no_newcomer() {
        let mut h = Harness::new(&[1, 2]);
        h.send(1, create(true));
        h.send(1, start());
        h.forget();

        h.send(2, join(1));
        assert_eq!(errors(h.take(2)), ["빈 자리가 없습니다."]);
        assert_eq!(h.hub.clients[&2].room, None);
        assert_eq!(h.hub.rooms[&1].members.len(), 1);
        assert_eq!(h.hub.rooms[&1].seats.bot_count(), 9);
        assert_eq!(h.roster(1).len(), 10);
        assert!(h.take(1).is_empty(), "the room heard nothing");
    }

    #[test]
    fn nobody_joins_a_match_that_is_over_until_the_room_is_back_in_the_waiting_room() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create(false));
        h.send(2, join(1));
        // Red: the host and four bots. Blue: only the guest, whose leaving forfeits the match.
        for slot in 1..TEAM_SIZE {
            h.send(1, flip(pb::Team::Red, slot, true));
        }
        h.send(2, ready(true));
        h.send(1, start());
        h.send(2, In::LeaveRoom(pb::LeaveRoom {}));
        h.forget();

        // Blue's seats are open, but the match is decided ...
        h.send(3, join(1));
        assert_eq!(errors(h.take(3)), ["경기가 끝나는 중입니다. 잠시 후 다시 들어오세요."]);
        // ... and from the next tick it shows its results.
        h.hub.tick();
        assert!(h.hub.rooms[&1].results_left.is_some());
        h.send(3, join(1));
        assert_eq!(errors(h.take(3)), ["경기가 끝나는 중입니다. 잠시 후 다시 들어오세요."]);
        assert_eq!(h.hub.clients[&3].room, None);
        assert_eq!(h.hub.rooms[&1].members.len(), 1);

        // Back in the waiting room, a newcomer joins as usual.
        let mut ticks = 0;
        while h.hub.rooms[&1].game.is_some() {
            h.hub.tick();
            ticks += 1;
            assert!(ticks <= 10 * TICK_RATE, "the room never left the results");
        }
        h.forget();
        h.send(3, join(1));
        assert_eq!(h.hub.rooms[&1].seats.find(3), Some((BLUE, 0)));
        assert_eq!(states(h.take(3)).len(), 1);
    }

    #[test]
    fn a_newcomer_to_a_running_free_for_all_takes_the_lowest_open_seat_and_no_team() {
        let mut h = Harness::new(&[1, 2, 3, 4]);
        h.send(1, create_ffa(true));
        // The host takes the bots out of seats 7 and 3.
        h.send(1, flip(pb::Team::Unspecified, 7, false));
        h.send(1, flip(pb::Team::Unspecified, 3, false));
        h.send(1, start());
        h.forget();

        h.send(2, join(1));
        assert_eq!(h.hub.rooms[&1].seats.find(2), Seats::at(3));
        let joined = match_start(h.take(2)).expect("the newcomer is sent into the match");
        assert!(joined.roster.iter().any(|r| r.id == 2 && r.team == NO_TEAM));
        h.send(3, join(1));
        assert_eq!(h.hub.rooms[&1].seats.find(3), Seats::at(7));
        assert_eq!(h.roster(1).len(), 10);

        // Only bots' seats are left.
        h.send(4, join(1));
        assert_eq!(errors(h.take(4)), ["빈 자리가 없습니다."]);
    }

    #[test]
    fn an_augment_match_deals_a_newcomer_a_hand_while_cards_are_chosen_and_a_card_after() {
        let mut h = Harness::new(&[1, 2, 3]);
        h.send(1, create_augment(false));
        h.send(1, flip(pb::Team::Blue, 0, true));
        h.send(1, start());

        // While the cards are being chosen, a newcomer is dealt a hand like everyone else.
        h.send(2, join(1));
        let snapshot = h.snapshot(1, 2);
        assert!(snapshot.pick_left > 0.0);
        assert_eq!(snapshot.offered.len(), crate::game::augments::HAND_SIZE);
        let me = snapshot.players.iter().find(|p| p.id == 2).unwrap();
        assert_eq!(me.augment, pb::Augment::Unspecified as i32);

        // Both players choose, which ends the choosing at the next tick.
        for id in [1, 2] {
            let card = h.snapshot(1, id).offered[0];
            h.send(id, In::PickAugment(pb::PickAugment { augment: card }));
        }
        h.hub.tick();
        assert_eq!(h.snapshot(1, 1).pick_left, 0.0);

        // From then on a newcomer gets a card at once, with nothing left to choose.
        h.send(3, join(1));
        let snapshot = h.snapshot(1, 3);
        assert!(snapshot.offered.is_empty());
        let me = snapshot.players.iter().find(|p| p.id == 3).unwrap();
        assert_ne!(me.augment, pb::Augment::Unspecified as i32);
        assert!(!me.active.is_empty());
    }
}
