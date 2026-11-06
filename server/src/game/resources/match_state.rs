//! The match clock, score, phases and result.

use crate::pb;
use bevy_ecs::prelude::*;

/// Who won a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winner {
    Team(pb::Team),
    /// The id of the player who won a free-for-all.
    Player(u32),
}

#[derive(Resource, Default)]
pub struct MatchState {
    pub tick: u32,
    pub red_score: u32,
    pub blue_score: u32,
    pub winner: Option<Winner>,
    pub kills_to_win: u32,
    /// Seconds left to choose a card, during which `freeze_left` waits.
    pub pick_left: f32,
    /// Seconds until play starts.
    pub freeze_left: f32,
    /// Ticks spent before play started.
    pub frozen_ticks: u32,
}

impl MatchState {
    /// Adds a kill to a team's score and ends the match when the team reaches the target. Team
    /// matches only: a free-for-all keeps no team score.
    pub fn score_kill(&mut self, team: pb::Team) {
        let score = if team == pb::Team::Red {
            &mut self.red_score
        } else {
            &mut self.blue_score
        };
        *score += 1;
        if *score >= self.kills_to_win && self.winner.is_none() {
            self.winner = Some(Winner::Team(team));
        }
    }
}
