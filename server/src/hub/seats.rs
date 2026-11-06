//! A room's seats: five a team, each held by a human, a bot or nobody.

use super::*;
use crate::game::TEAM_SIZE;

/// Who holds a seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Seat {
    Open,
    Bot,
    Human(u32),
}

/// A room's seats: five on each team, indexed `[red, blue][slot]`. A seat keeps its position,
/// so when someone leaves it just becomes open and nobody shifts. A free-for-all room has no
/// teams and numbers the same seats 0 to 9 instead: seat `i` is `[i / 5][i % 5]`.
#[derive(Debug)]
pub(super) struct Seats(pub(super) [[Seat; TEAM_SIZE]; 2]);

// A free-for-all room's seats are exactly the two teams' seats.
const _: () = assert!(MAX_PLAYERS == 2 * TEAM_SIZE);

impl Seats {
    /// Every seat starts as a bot, or open.
    pub(super) fn new(bots: bool) -> Self {
        Self([[if bots { Seat::Bot } else { Seat::Open }; TEAM_SIZE]; 2])
    }

    /// `(team, slot)` of the seat a free-for-all room numbers `index`, if there is one.
    pub(super) fn at(index: usize) -> Option<(usize, usize)> {
        (index < MAX_PLAYERS).then_some((index / TEAM_SIZE, index % TEAM_SIZE))
    }

    /// Every seat as `(team, slot, seat)`, red first, which is also the order of the
    /// free-for-all numbering.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, usize, Seat)> + '_ {
        self.0.iter().enumerate().flat_map(|(team, seats)| {
            seats
                .iter()
                .enumerate()
                .map(move |(slot, seat)| (team, slot, *seat))
        })
    }

    pub(super) fn find(&self, id: u32) -> Option<(usize, usize)> {
        self.iter()
            .find(|(_, _, seat)| *seat == Seat::Human(id))
            .map(|(team, slot, _)| (team, slot))
    }

    pub(super) fn humans(&self, team: usize) -> usize {
        self.0[team]
            .iter()
            .filter(|seat| matches!(seat, Seat::Human(_)))
            .count()
    }

    pub(super) fn bots(&self, team: usize) -> usize {
        self.0[team]
            .iter()
            .filter(|seat| **seat == Seat::Bot)
            .count()
    }

    pub(super) fn bot_count(&self) -> usize {
        self.bots(0) + self.bots(1)
    }

    pub(super) fn has_open(&self, team: usize) -> bool {
        self.0[team].contains(&Seat::Open)
    }

    /// The seat a newcomer to `team` would take, and what it holds now: the first open seat, or
    /// else the first bot's. `None` when the team is all humans.
    pub(super) fn free_seat(&self, team: usize) -> Option<(usize, Seat)> {
        let seats = &self.0[team];
        [Seat::Open, Seat::Bot]
            .into_iter()
            .find_map(|kind| Some((seats.iter().position(|seat| *seat == kind)?, kind)))
    }

    /// Seats `id` on `team`; a bot in the seat gives it up. `None` when the team is all humans.
    pub(super) fn sit(&mut self, team: usize, id: u32) -> Option<usize> {
        let (slot, _) = self.free_seat(team)?;
        self.0[team][slot] = Seat::Human(id);
        Some(slot)
    }

    /// Seats `id` in the first open seat of `team`; no bot gives way. `None` when it has none.
    pub(super) fn sit_open(&mut self, team: usize, id: u32) -> Option<usize> {
        let slot = self.0[team].iter().position(|seat| *seat == Seat::Open)?;
        self.0[team][slot] = Seat::Human(id);
        Some(slot)
    }

    /// Seats `id` in a free-for-all room: the first open seat by number, or else the first bot's,
    /// which the bot gives up. Returns the seat's number; `None` when every seat holds a human.
    pub(super) fn sit_first(&mut self, id: u32) -> Option<usize> {
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
    pub(super) fn switch(&mut self, id: u32) -> bool {
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
    pub(super) fn leave(&mut self, id: u32) {
        if let Some((team, slot)) = self.find(id) {
            self.0[team][slot] = Seat::Open;
        }
    }

    /// Puts a bot in a seat or takes it out. `false` when nothing changed: there is no such seat,
    /// a human holds it, or it already was so.
    pub(super) fn set_bot(&mut self, team: usize, slot: usize, bot: bool) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    const RED: usize = 0;
    const BLUE: usize = 1;

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
    fn free_for_all_seats_are_numbered_red_then_blue() {
        assert_eq!(Seats::at(0), Some((RED, 0)));
        assert_eq!(Seats::at(4), Some((RED, 4)));
        assert_eq!(Seats::at(7), Some((BLUE, 2)));
        assert_eq!(Seats::at(9), Some((BLUE, 4)));
        assert_eq!(Seats::at(10), None);
    }
}
