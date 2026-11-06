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
