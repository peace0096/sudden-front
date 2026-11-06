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
        *score = score.saturating_add(1);
        if *score >= self.kills_to_win && self.winner.is_none() {
            self.winner = Some(Winner::Team(team));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pb::Team::{Blue, Red};

    /// A team match played to `kills`.
    fn race_to(kills: u32) -> MatchState {
        MatchState {
            kills_to_win: kills,
            ..Default::default()
        }
    }

    #[test]
    fn a_kill_scores_for_the_killers_team_only() {
        let mut state = race_to(5);
        state.score_kill(Red);
        assert_eq!((state.red_score, state.blue_score), (1, 0));
        state.score_kill(Blue);
        state.score_kill(Blue);
        assert_eq!((state.red_score, state.blue_score), (1, 2));
        assert_eq!(state.winner, None);
    }

    #[test]
    fn the_first_team_to_reach_the_target_wins_and_stays_the_winner() {
        let mut state = race_to(2);
        state.score_kill(Blue);
        state.score_kill(Red);
        assert_eq!(state.winner, None, "one kill short each");
        state.score_kill(Blue);
        assert_eq!(state.winner, Some(Winner::Team(Blue)));
        // Kills landing after the end, on the same tick, still count but leave the result alone.
        state.score_kill(Red);
        state.score_kill(Red);
        assert_eq!(state.red_score, 3);
        assert_eq!(state.winner, Some(Winner::Team(Blue)));
    }

    #[test]
    fn a_result_already_decided_is_not_overturned_by_a_kill() {
        let mut state = MatchState {
            winner: Some(Winner::Team(Red)),
            ..race_to(1)
        };
        state.score_kill(Blue);
        assert_eq!(state.blue_score, 1);
        assert_eq!(state.winner, Some(Winner::Team(Red)));
    }

    #[test]
    fn a_score_never_wraps_around() {
        let mut state = MatchState {
            blue_score: u32::MAX,
            ..race_to(u32::MAX)
        };
        state.score_kill(Blue);
        assert_eq!(state.blue_score, u32::MAX);
        assert_eq!(state.winner, Some(Winner::Team(Blue)));
    }
}
