//! A bot's blackboard: what it has heard from the server, and the actions its behaviour tree runs
//! against that. Actions never touch the network: they read the state `absorb` keeps and put the
//! messages to send in `outbox`, which the connection loop (`bot.rs`) sends after each tick.

use crate::combat::{Arena, Body, Brain, Me};
use crate::pb::{self, client_message::Payload as Out, server_message::Payload as In};
use crate::plan::{room_name, Place};
use crate::rng::Rng;
use bonsai_bt::Status::{self, Failure, Running, Success};
use std::collections::HashMap;

/// How long to wait for an answer before sending a lobby request again (s).
const RESEND_AFTER: f64 = 2.0;
/// How long a leader waits for its whole group before starting with whoever came (s).
const GATHER_TIMEOUT: f64 = 30.0;
/// The server ticks that remote players are drawn behind the latest snapshot, as in the browser.
const INTERP_TICKS: u32 = 2;
/// The primaries a bot picks from for its matches (the B menu's choices).
const PRIMARIES: [pb::WeaponKind; 4] = [
    pb::WeaponKind::Rifle,
    pb::WeaponKind::Sniper,
    pb::WeaponKind::MachineGun,
    pb::WeaponKind::Smg,
];

/// The rooms a run's leaders make.
#[derive(Clone, Copy, Debug)]
pub struct RoomSettings {
    pub run: u32,
    pub mode: pb::GameMode,
    pub map: pb::MapId,
    pub match_type: pb::MatchType,
}

/// The behaviour tree's leaves (see `tree.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Succeeds once the run is told to stop.
    AwaitShutdown,
    /// Says hello with the bot's name; succeeds on Welcome.
    Hello,
    /// Succeeds for the bot that leads its group, fails for the rest.
    IsLeader,
    /// Makes the group's room; succeeds once seated in it.
    CreateRoom,
    /// Finds the group's room in the room list and joins it; succeeds once seated.
    JoinRoom,
    /// The leader: waits for the group to gather and ready up, then starts; succeeds on MatchStart.
    StartMatch,
    /// Everyone else: readies up; succeeds on MatchStart.
    ReadyUp,
    /// Plays the match; succeeds on MatchEnd.
    Fight,
    /// Waits out the results; succeeds when the room is back in the waiting room.
    AwaitLobby,
    /// Leaves the room, if in one; always succeeds.
    Leave,
}

/// A match being played.
struct Battle {
    arena: Arena,
    /// Each player's team, from the roster.
    teams: HashMap<u32, i32>,
    snapshot: Option<pb::Snapshot>,
    brain: Brain,
    picked_augment: bool,
}

pub struct Agent {
    pub name: String,
    pub place: Place,
    settings: RoomSettings,
    pub rng: Rng,
    /// Set by the run when it is time to go.
    pub shutdown: bool,
    pub my_id: Option<u32>,
    rooms: Vec<pb::RoomInfo>,
    /// The room seated in, as the last RoomState showed it.
    room: Option<pb::RoomState>,
    battle: Option<Battle>,
    /// The match ended and the room has not yet gone back to waiting.
    results: bool,
    /// When the pending lobby request goes out again (s, counting down).
    resend_in: f64,
    /// How long the leader has waited for its group (s).
    gathering: f64,
    weapon_picked: bool,
    seq: u32,
    pub outbox: Vec<Out>,
    /// Events for the run's counters, taken by the connection loop.
    pub heard: Heard,
}

/// What the last messages meant, for the run's counters.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Heard {
    pub snapshots: u64,
    /// Kills this bot made.
    pub kills: u64,
    pub errors: Vec<String>,
    pub match_started: bool,
    pub match_ended: bool,
}

impl Agent {
    pub fn new(name: String, place: Place, settings: RoomSettings, rng: Rng) -> Self {
        Self {
            name,
            place,
            settings,
            rng,
            shutdown: false,
            my_id: None,
            rooms: Vec::new(),
            room: None,
            battle: None,
            results: false,
            resend_in: 0.0,
            gathering: 0.0,
            weapon_picked: false,
            seq: 0,
            outbox: Vec::new(),
            heard: Heard::default(),
        }
    }

    pub fn in_room(&self) -> bool {
        self.room.is_some()
    }

    pub fn in_match(&self) -> bool {
        self.battle.is_some()
    }

    /// Takes in one message from the server.
    pub fn absorb(&mut self, message: pb::ServerMessage) {
        let Some(payload) = message.payload else {
            return;
        };
        match payload {
            In::Welcome(welcome) => self.my_id = Some(welcome.player_id),
            In::RoomList(list) => self.rooms = list.rooms,
            In::RoomState(state) => {
                if state
                    .room
                    .as_ref()
                    .is_some_and(|r| r.phase == pb::RoomPhase::Waiting as i32)
                {
                    self.results = false;
                }
                self.room = Some(state);
                self.resend_in = 0.0;
            }
            In::LeftRoom(_) => {
                self.room = None;
                self.battle = None;
            }
            In::Error(error) => self.heard.errors.push(error.message),
            In::MatchStart(start) => {
                self.battle = Some(Battle {
                    arena: Arena::new(&start),
                    teams: start.roster.iter().map(|r| (r.id, r.team)).collect(),
                    snapshot: None,
                    brain: Brain::default(),
                    picked_augment: false,
                });
                self.results = false;
                self.heard.match_started = true;
            }
            In::RosterUpdate(update) => {
                if let Some(battle) = self.battle.as_mut() {
                    battle.teams = update.roster.iter().map(|r| (r.id, r.team)).collect();
                }
            }
            In::Snapshot(snapshot) => {
                self.heard.snapshots = self.heard.snapshots.saturating_add(1);
                let kills = snapshot
                    .events
                    .iter()
                    .filter(|e| {
                        matches!(&e.event, Some(pb::game_event::Event::Kill(kill))
                            if Some(kill.killer_id) == self.my_id)
                    })
                    .count();
                self.heard.kills = self.heard.kills.saturating_add(kills as u64);
                if let Some(battle) = self.battle.as_mut() {
                    battle.snapshot = Some(snapshot);
                }
            }
            In::MatchEnd(_) => {
                self.battle = None;
                self.results = true;
                self.heard.match_ended = true;
            }
            In::Pong(_) => {}
        }
    }

    /// Moves the bot's clocks on by `dt` seconds; once a tick, before the tree runs.
    pub fn advance(&mut self, dt: f64) {
        self.resend_in -= dt;
    }

    /// Runs one leaf of the behaviour tree, `dt` seconds after the last tick.
    pub fn act(&mut self, act: Act, dt: f64) -> Status {
        match act {
            Act::AwaitShutdown => done(self.shutdown),
            Act::Hello => {
                if self.my_id.is_some() {
                    return Success;
                }
                if self.due() {
                    self.outbox.push(Out::Hello(pb::Hello {
                        name: self.name.clone(),
                    }));
                }
                Running
            }
            Act::IsLeader => {
                if self.place.leader {
                    Success
                } else {
                    Failure
                }
            }
            Act::CreateRoom => {
                if self.in_room() {
                    return Success;
                }
                if self.due() {
                    self.outbox.push(Out::CreateRoom(pb::CreateRoom {
                        name: room_name(self.settings.run, self.place.group),
                        // A leader with nobody to follow plays the server's own bots.
                        fill_bots: self.place.size < 2,
                        map: self.settings.map as i32,
                        mode: self.settings.mode as i32,
                        match_type: self.settings.match_type as i32,
                    }));
                }
                Running
            }
            Act::JoinRoom => {
                if self.in_room() {
                    return Success;
                }
                let name = room_name(self.settings.run, self.place.group);
                let room = self.rooms.iter().find(|r| r.name == name).map(|r| r.id);
                if let Some(room_id) = room {
                    if self.due() {
                        self.outbox.push(Out::JoinRoom(pb::JoinRoom { room_id }));
                    }
                }
                Running
            }
            Act::StartMatch => {
                if self.in_match() {
                    self.gathering = 0.0;
                    return Success;
                }
                self.pick_weapon();
                self.gathering += dt;
                if self.ready_to_start() && self.due() {
                    self.outbox.push(Out::StartGame(pb::StartGame {}));
                }
                Running
            }
            Act::ReadyUp => {
                if self.in_match() {
                    return Success;
                }
                self.pick_weapon();
                let me = self.my_id;
                let ready = self
                    .room
                    .as_ref()
                    .and_then(|r| r.players.iter().find(|p| Some(p.id) == me))
                    .is_some_and(|p| p.ready);
                if self.in_room() && !ready && self.due() {
                    self.outbox
                        .push(Out::SetReady(pb::SetReady { ready: true }));
                }
                Running
            }
            Act::Fight => {
                if self.results {
                    return Success;
                }
                if !self.in_room() {
                    return Failure;
                }
                self.fight();
                Running
            }
            Act::AwaitLobby => {
                if !self.in_room() {
                    return Failure;
                }
                done(!self.results)
            }
            Act::Leave => {
                if self.in_room() {
                    self.outbox.push(Out::LeaveRoom(pb::LeaveRoom {}));
                }
                Success
            }
        }
    }

    /// Whether a lobby request may go out (again) now; if so, the next waits `RESEND_AFTER`.
    fn due(&mut self) -> bool {
        if self.resend_in > 0.0 {
            return false;
        }
        self.resend_in = RESEND_AFTER;
        true
    }

    /// Picks a random primary for the bot's matches, once.
    fn pick_weapon(&mut self) {
        if self.weapon_picked || !self.in_room() {
            return;
        }
        self.weapon_picked = true;
        let weapon = PRIMARIES[self.rng.index(PRIMARIES.len())];
        self.outbox.push(Out::SelectWeapon(pb::SelectWeapon {
            weapon: weapon as i32,
        }));
    }

    /// The leader starts once the whole group sits in the room and everyone else is ready, or,
    /// after `GATHER_TIMEOUT`, with whoever came and readied.
    fn ready_to_start(&self) -> bool {
        let Some(room) = self.room.as_ref() else {
            return false;
        };
        let others_ready = room
            .players
            .iter()
            .filter(|p| Some(p.id) != self.my_id)
            .all(|p| p.ready);
        let gathered = room.players.len() >= self.place.size || self.gathering >= GATHER_TIMEOUT;
        let filled = room.players.len().saturating_add(room.bots.len()) >= 2;
        others_ready && gathered && filled
    }

    /// One decision from the latest snapshot, sent as the two commands of a server tick.
    fn fight(&mut self) {
        let Some(my_id) = self.my_id else {
            return;
        };
        let Some(battle) = self.battle.as_mut() else {
            return;
        };
        let Some(snapshot) = battle.snapshot.as_ref() else {
            return;
        };
        // Augment mode: take the first card offered rather than wait out the clock.
        if !battle.picked_augment {
            if let Some(&augment) = snapshot.offered.first() {
                battle.picked_augment = true;
                self.outbox
                    .push(Out::PickAugment(pb::PickAugment { augment }));
            }
        }
        let team = |id: u32| battle.teams.get(&id).copied().unwrap_or_default();
        let Some(mine) = snapshot.players.iter().find(|p| p.id == my_id) else {
            return;
        };
        let me = Me {
            id: my_id,
            team: team(my_id),
            x: mine.x,
            y: mine.y,
            z: mine.z,
            yaw: mine.yaw,
            pitch: mine.pitch,
            alive: mine.alive,
            weapon: mine.weapon,
            magazine: snapshot.magazine,
            reserve: snapshot.reserve,
            reloading: mine.reloading,
        };
        let others: Vec<Body> = snapshot
            .players
            .iter()
            .filter(|p| p.id != my_id)
            .map(|p| Body {
                id: p.id,
                team: team(p.id),
                x: p.x,
                y: p.y,
                z: p.z,
                crouching: p.crouching,
                targetable: p.alive && !p.protected,
                king: p.king_left > 0.0,
            })
            .collect();
        let render_tick = snapshot.tick.saturating_sub(INTERP_TICKS);
        let cmd = battle
            .brain
            .think(&battle.arena, &me, &others, render_tick, &mut self.rng);
        let mut cmds = [cmd, cmd];
        for cmd in &mut cmds {
            self.seq = self.seq.wrapping_add(1);
            cmd.seq = self.seq;
        }
        self.outbox.push(Out::Input(pb::InputBatch {
            cmds: cmds.to_vec(),
        }));
    }
}

fn done(finished: bool) -> Status {
    if finished {
        Success
    } else {
        Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::place;

    const SETTINGS: RoomSettings = RoomSettings {
        run: 7,
        mode: pb::GameMode::Normal,
        map: pb::MapId::SupplyYard,
        match_type: pb::MatchType::TeamDeathmatch,
    };

    fn agent(index: usize, count: usize) -> Agent {
        Agent::new(
            format!("bot_{index}"),
            place(index, count),
            SETTINGS,
            Rng::new(1),
        )
    }

    fn message(payload: In) -> pb::ServerMessage {
        pb::ServerMessage {
            payload: Some(payload),
        }
    }

    fn room_state(phase: pb::RoomPhase, players: &[(u32, bool)]) -> In {
        In::RoomState(pb::RoomState {
            room: Some(pb::RoomInfo {
                id: 3,
                name: room_name(SETTINGS.run, 0),
                phase: phase as i32,
                ..Default::default()
            }),
            players: players
                .iter()
                .map(|&(id, ready)| pb::RoomPlayer {
                    id,
                    ready,
                    ..Default::default()
                })
                .collect(),
            bots: Vec::new(),
        })
    }

    fn welcomed(index: usize, count: usize, id: u32) -> Agent {
        let mut agent = agent(index, count);
        agent.absorb(message(In::Welcome(pb::Welcome {
            player_id: id,
            name: agent.name.clone(),
        })));
        agent
    }

    #[test]
    fn hello_is_sent_once_until_the_welcome_and_then_succeeds() {
        let mut agent = agent(0, 1);
        assert_eq!(agent.act(Act::Hello, 0.1), Running);
        assert_eq!(agent.act(Act::Hello, 0.1), Running);
        assert_eq!(agent.outbox.len(), 1, "no repeat before the resend time");
        assert!(matches!(&agent.outbox[0], Out::Hello(h) if h.name == "bot_0"));
        agent.absorb(message(In::Welcome(pb::Welcome {
            player_id: 9,
            name: String::new(),
        })));
        assert_eq!(agent.act(Act::Hello, 0.1), Success);
        assert_eq!(agent.my_id, Some(9));
    }

    #[test]
    fn only_the_first_bot_of_a_group_leads() {
        assert_eq!(agent(0, 20).act(Act::IsLeader, 0.0), Success);
        assert_eq!(agent(1, 20).act(Act::IsLeader, 0.0), Failure);
        assert_eq!(agent(10, 20).act(Act::IsLeader, 0.0), Success);
    }

    #[test]
    fn a_leader_makes_its_groups_room_with_the_runs_settings() {
        let mut leader = welcomed(0, 10, 1);
        assert_eq!(leader.act(Act::CreateRoom, 0.1), Running);
        let Some(Out::CreateRoom(create)) = leader.outbox.pop() else {
            panic!("no CreateRoom");
        };
        assert_eq!(create.name, room_name(SETTINGS.run, 0));
        assert!(!create.fill_bots, "a full group needs no server bots");
        leader.absorb(message(room_state(pb::RoomPhase::Waiting, &[(1, false)])));
        assert_eq!(leader.act(Act::CreateRoom, 0.1), Success);
    }

    #[test]
    fn a_leader_alone_fills_its_room_with_server_bots() {
        let mut leader = welcomed(0, 1, 1);
        leader.act(Act::CreateRoom, 0.1);
        assert!(matches!(leader.outbox.pop(), Some(Out::CreateRoom(c)) if c.fill_bots));
    }

    #[test]
    fn a_member_waits_for_its_groups_room_to_be_listed_then_joins_it() {
        let mut member = welcomed(1, 10, 2);
        assert_eq!(member.act(Act::JoinRoom, 0.1), Running);
        assert!(member.outbox.is_empty(), "no room to join yet");
        member.absorb(message(In::RoomList(pb::RoomList {
            rooms: vec![
                pb::RoomInfo {
                    id: 4,
                    name: room_name(SETTINGS.run + 1, 0),
                    ..Default::default()
                },
                pb::RoomInfo {
                    id: 5,
                    name: room_name(SETTINGS.run, 0),
                    ..Default::default()
                },
            ],
        })));
        member.act(Act::JoinRoom, 0.1);
        assert!(matches!(member.outbox.pop(), Some(Out::JoinRoom(j)) if j.room_id == 5));
        member.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, false)],
        )));
        assert_eq!(member.act(Act::JoinRoom, 0.1), Success);
    }

    #[test]
    fn a_member_readies_up_and_succeeds_when_the_match_starts() {
        let mut member = welcomed(1, 2, 2);
        member.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, false)],
        )));
        assert_eq!(member.act(Act::ReadyUp, 0.1), Running);
        assert!(member
            .outbox
            .iter()
            .any(|m| matches!(m, Out::SetReady(r) if r.ready)));
        assert!(member
            .outbox
            .iter()
            .any(|m| matches!(m, Out::SelectWeapon(_))));
        member.absorb(message(In::MatchStart(pb::MatchStart::default())));
        assert_eq!(member.act(Act::ReadyUp, 0.1), Success);
    }

    #[test]
    fn a_leader_starts_only_when_its_whole_group_is_in_and_ready() {
        let mut leader = welcomed(0, 3, 1);
        let starts = |leader: &mut Agent| {
            leader.outbox.clear();
            leader.resend_in = 0.0;
            leader.act(Act::StartMatch, 0.1);
            leader.outbox.iter().any(|m| matches!(m, Out::StartGame(_)))
        };
        leader.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, true)],
        )));
        assert!(!starts(&mut leader), "one of the group is missing");
        leader.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, true), (3, false)],
        )));
        assert!(!starts(&mut leader), "one is not ready");
        leader.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, true), (3, true)],
        )));
        assert!(starts(&mut leader));
    }

    #[test]
    fn a_leader_gives_up_waiting_for_stragglers_after_a_while() {
        let mut leader = welcomed(0, 10, 1);
        leader.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, true)],
        )));
        leader.act(Act::StartMatch, GATHER_TIMEOUT + 1.0);
        assert!(leader.outbox.iter().any(|m| matches!(m, Out::StartGame(_))));
    }

    #[test]
    fn a_fight_sends_two_numbered_commands_a_tick_and_ends_with_the_match() {
        let mut bot = welcomed(1, 2, 2);
        bot.absorb(message(room_state(
            pb::RoomPhase::InGame,
            &[(1, true), (2, true)],
        )));
        bot.absorb(message(In::MatchStart(pb::MatchStart {
            config: Some(pb::GameConfig {
                eye_height: 1.42,
                player_height: 1.8,
                ..Default::default()
            }),
            map: Some(pb::MapData {
                half_x: 10.0,
                half_z: 10.0,
                ..Default::default()
            }),
            ..Default::default()
        })));
        assert_eq!(bot.act(Act::Fight, 0.03), Running);
        assert!(
            bot.outbox.is_empty(),
            "nothing to go on before the first snapshot"
        );
        bot.absorb(message(In::Snapshot(pb::Snapshot {
            tick: 10,
            offered: vec![pb::Augment::Shield as i32],
            players: vec![pb::PlayerState {
                id: 2,
                alive: true,
                ..Default::default()
            }],
            ..Default::default()
        })));
        bot.act(Act::Fight, 0.03);
        bot.act(Act::Fight, 0.03);
        let batches: Vec<&pb::InputBatch> = bot
            .outbox
            .iter()
            .filter_map(|m| match m {
                Out::Input(batch) => Some(batch),
                _ => None,
            })
            .collect();
        let seqs: Vec<u32> = batches
            .iter()
            .flat_map(|b| b.cmds.iter().map(|c| c.seq))
            .collect();
        assert_eq!(seqs, [1, 2, 3, 4]);
        assert!(batches
            .iter()
            .all(|b| b.cmds[0].render_tick == 10 - INTERP_TICKS));
        let picks = bot
            .outbox
            .iter()
            .filter(|m| matches!(m, Out::PickAugment(_)))
            .count();
        assert_eq!(picks, 1, "the first card, once");

        bot.absorb(message(In::MatchEnd(pb::MatchEnd::default())));
        assert_eq!(bot.act(Act::Fight, 0.03), Success);
        assert_eq!(
            bot.act(Act::AwaitLobby, 0.03),
            Running,
            "the results are showing"
        );
        bot.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, false)],
        )));
        assert_eq!(bot.act(Act::AwaitLobby, 0.03), Success);
    }

    #[test]
    fn leaving_sends_leave_room_only_from_a_room() {
        let mut bot = welcomed(1, 2, 2);
        assert_eq!(bot.act(Act::Leave, 0.0), Success);
        assert!(bot.outbox.is_empty());
        bot.absorb(message(room_state(
            pb::RoomPhase::Waiting,
            &[(1, false), (2, false)],
        )));
        bot.act(Act::Leave, 0.0);
        assert!(matches!(bot.outbox.pop(), Some(Out::LeaveRoom(_))));
    }
}
