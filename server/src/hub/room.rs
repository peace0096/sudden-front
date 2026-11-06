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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::TEAM_SIZE;

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
    fn a_running_match_seats_a_newcomer_on_the_team_with_fewer_players_then_fewer_humans_then_red()
    {
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
}
