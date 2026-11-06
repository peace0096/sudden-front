//! The match's mode and the rules that follow from it.

use crate::pb;
use bevy_ecs::prelude::*;

/// What differs between game modes and match types.
#[derive(Resource)]
pub struct Rules {
    pub mode: pb::GameMode,
    pub max_hp: i32,
    /// Free-for-all: every player's team is `Unspecified` and everyone fights everyone.
    pub ffa: bool,
    /// Big Head mode: kills and deaths move levels and head sizes, and the top level crowns the King.
    pub big_head: bool,
}

impl Rules {
    /// Whether players on these teams can hurt each other. Callers still rule out hurting
    /// yourself: in a free-for-all everyone is on the same (no) team.
    pub fn hostile(&self, a: pb::Team, b: pb::Team) -> bool {
        self.ffa || a != b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::MAX_HP;

    const TEAMS: [pb::Team; 3] = [pb::Team::Unspecified, pb::Team::Red, pb::Team::Blue];

    fn rules(ffa: bool) -> Rules {
        Rules {
            mode: pb::GameMode::Normal,
            max_hp: MAX_HP,
            ffa,
            big_head: false,
        }
    }

    #[test]
    fn in_a_team_match_only_the_other_team_is_hostile() {
        let rules = rules(false);
        assert!(rules.hostile(pb::Team::Red, pb::Team::Blue));
        assert!(rules.hostile(pb::Team::Blue, pb::Team::Red));
        assert!(!rules.hostile(pb::Team::Red, pb::Team::Red));
        assert!(!rules.hostile(pb::Team::Blue, pb::Team::Blue));
    }

    #[test]
    fn in_a_free_for_all_everyone_is_hostile_though_nobody_has_a_team() {
        let rules = rules(true);
        for a in TEAMS {
            for b in TEAMS {
                assert!(rules.hostile(a, b), "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn hostility_goes_both_ways() {
        for ffa in [false, true] {
            let rules = rules(ffa);
            for a in TEAMS {
                for b in TEAMS {
                    assert_eq!(rules.hostile(a, b), rules.hostile(b, a), "{a:?} and {b:?}");
                }
            }
        }
    }
}
