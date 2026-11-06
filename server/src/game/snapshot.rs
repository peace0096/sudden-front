//! What a match tells its players: the roster, the start message, each tick's snapshot and the result.

use super::components::*;
use super::resources::*;
use super::*;
use crate::pb;

/// Seconds that `ticks` ticks take.
fn seconds(ticks: u32) -> f32 {
    ticks as f32 * TICK_DT
}

/// The match clock, in seconds: the ticks played, not counting those frozen for the augment pick
/// and the wait after it.
fn match_clock(tick: u32, frozen_ticks: u32) -> f32 {
    seconds(tick.saturating_sub(frozen_ticks))
}

impl Match {
    pub fn roster(&mut self) -> Vec<pb::RosterEntry> {
        let mut query = self.world.query::<&Player>();
        let mut roster: Vec<pb::RosterEntry> = query
            .iter(&self.world)
            .map(|p| pb::RosterEntry {
                id: p.id,
                name: p.name.clone(),
                team: p.team as i32,
                is_bot: p.is_bot,
            })
            .collect();
        roster.sort_by_key(|r| r.id);
        roster
    }

    pub fn start_message(&self, your_id: u32) -> pb::MatchStart {
        let mut roster: Vec<pb::RosterEntry> = self
            .entities
            .values()
            .filter_map(|e| self.world.get::<Player>(*e))
            .map(|p| pb::RosterEntry {
                id: p.id,
                name: p.name.clone(),
                team: p.team as i32,
                is_bot: p.is_bot,
            })
            .collect();
        roster.sort_by_key(|r| r.id);
        let big_head = self.world.resource::<Rules>().big_head;
        pb::MatchStart {
            your_id,
            config: Some(pb::GameConfig {
                tick_rate: TICK_RATE,
                kills_to_win: self.world.resource::<MatchState>().kills_to_win,
                player_half_width: PLAYER_HALF_WIDTH,
                player_height: PLAYER_HEIGHT,
                eye_height: EYE_HEIGHT,
                respawn_time: RESPAWN_TIME,
                weapons: weapons::all_stats(),
                draw_time: DRAW_TIME,
                crouch_eye_height: CROUCH_EYE_HEIGHT,
                crouch_speed_factor: CROUCH_SPEED_FACTOR,
                crouch_spread_factor: CROUCH_SPREAD_FACTOR,
                max_hp: self.world.resource::<Rules>().max_hp,
                mode: self.world.resource::<Rules>().mode as i32,
                start_wait: if self.world.resource::<Rules>().mode == pb::GameMode::Augment {
                    augments::AUGMENT_WAIT
                } else {
                    0.0
                },
                match_type: if self.world.resource::<Rules>().ffa {
                    pb::MatchType::FreeForAll
                } else {
                    pb::MatchType::TeamDeathmatch
                } as i32,
                jump_speed: JUMP_SPEED,
                gravity: GRAVITY,
                min_level: if big_head { big_head::MIN_LEVEL } else { 0 },
                max_level: if big_head { big_head::MAX_LEVEL } else { 0 },
                king_time: if big_head { big_head::KING_TIME } else { 0.0 },
            }),
            map: Some(self.world.resource::<Arena>().map.clone()),
            roster,
        }
    }

    /// Everything in a snapshot that's the same for every recipient.
    pub fn shared_snapshot(&mut self) -> pb::Snapshot {
        let mut query = self
            .world
            .query::<(&Player, &Pose, &Vitals, &Intent, &Weapon, &Stats, &Perks)>();
        let mut players: Vec<pb::PlayerState> = query
            .iter(&self.world)
            .map(
                |(player, pose, vitals, intent, weapon, stats, perks)| pb::PlayerState {
                    id: player.id,
                    x: pose.x,
                    y: pose.y,
                    z: pose.z,
                    vy: pose.vy,
                    yaw: pose.yaw,
                    pitch: pose.pitch,
                    hp: vitals.hp,
                    alive: vitals.alive,
                    moving: intent.moving,
                    crouching: pose.crouching,
                    reloading: weapon.reloading(),
                    protected: vitals.protect_left > 0.0,
                    respawn_in: vitals.respawn_left.max(0.0),
                    kills: stats.kills,
                    deaths: stats.deaths,
                    weapon: weapon.kind as i32,
                    augment: perks.augment as i32,
                    shield_hp: perks.shield_hp,
                    head_scale: perks.head_scale,
                    active: perks.active.iter().map(|a| *a as i32).collect(),
                    stun_left: vitals.stun_left,
                    level: perks.level,
                    king_left: seconds(perks.king_ticks),
                },
            )
            .collect();
        players.sort_by_key(|p| p.id);
        let state = self.world.resource::<MatchState>();
        pb::Snapshot {
            tick: state.tick,
            red_score: state.red_score,
            blue_score: state.blue_score,
            freeze_left: state.freeze_left,
            pick_left: state.pick_left,
            clock: match_clock(state.tick, state.frozen_ticks),
            players,
            missiles: self
                .world
                .resource::<Missiles>()
                .live
                .iter()
                .map(|m| pb::Missile {
                    id: m.id,
                    x: m.pos[0],
                    y: m.pos[1],
                    z: m.pos[2],
                    vx: m.dir[0] * systems::missile::MISSILE_SPEED,
                    vy: m.dir[1] * systems::missile::MISSILE_SPEED,
                    vz: m.dir[2] * systems::missile::MISSILE_SPEED,
                })
                .collect(),
            grenades: self
                .world
                .resource::<Grenades>()
                .live
                .iter()
                .map(|g| pb::Grenade {
                    id: g.id,
                    x: g.pos[0],
                    y: g.pos[1],
                    z: g.pos[2],
                    vx: g.vel[0],
                    vy: g.vel[1],
                    vz: g.vel[2],
                })
                .collect(),
            events: self.world.resource::<EventLog>().0.clone(),
            ..Default::default()
        }
    }

    pub fn fill_personal(&self, id: u32, snapshot: &mut pb::Snapshot) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        if let Some(weapon) = self.world.get::<Weapon>(entity) {
            snapshot.magazine = weapon.magazine;
            snapshot.reserve = weapon.reserve;
            snapshot.reload_remaining = weapon.reload_left;
            snapshot.primary_weapon = weapon.primary as i32;
            snapshot.sidearm_weapon = weapon.sidearm as i32;
            snapshot.grenades_left = weapon.grenades();
        }
        if let Some(loadout) = self.world.get::<Loadout>(entity) {
            snapshot.selected_weapon = loadout.0 as i32;
        }
        if let Some(offer) = self.world.get::<AugmentOffer>(entity) {
            snapshot.offered = offer.offered.iter().map(|a| *a as i32).collect();
            snapshot.refreshable = offer
                .offered
                .iter()
                .zip(offer.refreshed)
                .map(|(_, used)| !used)
                .collect();
        }
        if let Some(inputs) = self.world.get::<Inputs>(entity) {
            snapshot.last_input_seq = inputs.last_seq;
        }
    }

    /// The result, once there is a winner: a team, or in a free-for-all a player.
    pub fn end_message(&mut self, return_in: f32) -> pb::MatchEnd {
        let (red_score, blue_score, winner) = {
            let state = self.world.resource::<MatchState>();
            (state.red_score, state.blue_score, state.winner)
        };
        let (winner, winner_id) = match winner {
            Some(Winner::Team(team)) => (team, 0),
            Some(Winner::Player(id)) => (pb::Team::Unspecified, id),
            None => (pb::Team::Unspecified, 0),
        };
        let mut query = self.world.query::<(&Player, &Stats, &Perks)>();
        let mut scoreboard: Vec<pb::ScoreEntry> = query
            .iter(&self.world)
            .map(|(player, stats, perks)| pb::ScoreEntry {
                id: player.id,
                name: player.name.clone(),
                team: player.team as i32,
                kills: stats.kills,
                deaths: stats.deaths,
                is_bot: player.is_bot,
                level: perks.level,
            })
            .collect();
        scoreboard.sort_by_key(|s| (std::cmp::Reverse(s.kills), s.deaths));
        pb::MatchEnd {
            winner: winner as i32,
            red_score,
            blue_score,
            scoreboard,
            return_in,
            winner_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn the_clock_counts_only_the_ticks_played_after_the_freeze() {
        assert_eq!(seconds(TICK_RATE), 1.0);
        assert_eq!(match_clock(TICK_RATE * 3, TICK_RATE), 2.0);
        assert_eq!(match_clock(5, 10), 0.0);
    }

    #[test]
    fn a_free_for_all_tells_its_players_there_are_no_teams() {
        let start = ffa_with_bots(3, pb::GameMode::Normal).start_message(SHOOTER);
        let config = start.config.unwrap();
        assert_eq!(
            (config.match_type, config.kills_to_win),
            (pb::MatchType::FreeForAll as i32, KILLS_TO_WIN_FFA)
        );
        assert_eq!(start.roster.len(), 5);
        assert!(start.roster.iter().all(|r| r.team == NO_TEAM as i32));

        let config = duel().start_message(SHOOTER).config.unwrap();
        assert_eq!(
            (config.match_type, config.kills_to_win),
            (pb::MatchType::TeamDeathmatch as i32, KILLS_TO_WIN)
        );
    }
}
