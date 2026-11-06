//! A room: its settings, its members and the seat each one holds, and its match.

use super::*;
use crate::game::Match;
use crate::pb;

pub(super) struct Member {
    pub(super) id: u32,
    pub(super) weapon: pb::WeaponKind,
    /// Pressed ready in the waiting room. The host's is never set: they start the match instead.
    pub(super) ready: bool,
}

pub(super) struct Room {
    pub(super) id: u32,
    pub(super) name: String,
    pub(super) host: u32,
    pub(super) map: pb::MapId,
    pub(super) mode: pb::GameMode,
    /// Fixed when the room is made: the seats mean something else in each.
    pub(super) match_type: pb::MatchType,
    pub(super) kills_to_win: u32,
    pub(super) seats: Seats,
    /// Everyone in the room, longest first: the first takes over as host. Each holds a seat.
    pub(super) members: Vec<Member>,
    pub(super) game: Option<Match>,
    pub(super) results_left: Option<f32>,
}

impl Room {
    pub(super) fn ffa(&self) -> bool {
        self.match_type == pb::MatchType::FreeForAll
    }

    /// Seats a newcomer on the team with fewer humans, or else on the other one. When the humans
    /// are level, a team with an open seat is preferred. A free-for-all room takes the first open
    /// seat, or else the first bot's. `false` when every seat a newcomer could take holds a human.
    pub(super) fn seat_newcomer(&mut self, id: u32) -> bool {
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
    pub(super) fn seat_mid_match(&mut self, id: u32) -> Option<pb::Team> {
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
