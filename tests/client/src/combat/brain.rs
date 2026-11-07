//! A load-test bot's fighting: what it sees in the latest snapshot becomes the `InputCmd` it sends.
//!
//! The behaviour follows the server's own bots (`server/src/game/systems/bot.rs`): spot the nearest
//! enemy in view, turn towards them with a settling aim error, strafe and fire in bursts, and
//! otherwise patrol along grid paths. Unlike a server bot it plays by a human's rules: it sees only
//! what a snapshot shows, has a human's spare ammunition, and changes weapons with the slot keys.
//! It does not throw grenades.

use super::nav::NavGrid;
use super::sight::line_clear;
use crate::pb;
use crate::rng::Rng;
use std::f32::consts::{PI, TAU};

/// Seconds between a bot's decisions: one a server tick. Each decision goes out as two commands,
/// the 60 a second the browser sends, the way the server's bots fill their input queue.
pub const STEP_DT: f32 = 1.0 / 30.0;

const SIGHT_RANGE: f32 = 46.0;
const SIGHT_HALF_ANGLE: f32 = 1.4;
/// Always noticed this close, behind or not.
const NEAR: f32 = 4.0;
const REACTION_MIN: f32 = 0.45;
const REACTION_MAX: f32 = 0.95;
const TURN_RATE: f32 = 3.2;
/// Chest height above the feet, where a bot aims.
const AIM_CHEST_Y: f32 = 1.2;
const INITIAL_AIM_ERROR: f32 = 0.085;
const AIM_SETTLE_RATE: f32 = 0.55;
const AIM_WOBBLE: f32 = 0.02;
/// The browser's own limit on looking up or down (`client/src/game/game.ts`).
const MAX_PITCH: f32 = 1.45;
/// Close enough to a waypoint to head for the next one.
const WAYPOINT_REACH: f32 = 0.6;
/// Beyond this an enemy is walked towards while shooting.
const CLOSE_IN: f32 = 28.0;
/// How long walking may make no progress before the route is dropped.
const STUCK_AFTER: f32 = 0.6;

/// The map and the rules a bot fights by, from `MatchStart`.
pub struct Arena {
    pub boxes: Vec<pb::MapBox>,
    pub nav: NavGrid,
    pub eye_height: f32,
    /// How much lower a crouching player's eye (and chest) is.
    pub crouch_drop: f32,
    /// A free-for-all: everyone else is an enemy.
    pub ffa: bool,
    pub weapons: Vec<pb::WeaponStats>,
}

impl Arena {
    pub fn new(start: &pb::MatchStart) -> Self {
        let config = start.config.clone().unwrap_or_default();
        let map = start.map.clone().unwrap_or_default();
        Self {
            nav: NavGrid::new(&map.boxes, map.half_x, map.half_z, config.player_height),
            boxes: map.boxes,
            eye_height: config.eye_height,
            crouch_drop: (config.eye_height - config.crouch_eye_height).max(0.0),
            ffa: config.match_type == pb::MatchType::FreeForAll as i32,
            weapons: config.weapons,
        }
    }

    fn stats(&self, kind: i32) -> pb::WeaponStats {
        self.weapons
            .iter()
            .find(|w| w.kind == kind)
            .cloned()
            .unwrap_or_default()
    }
}

/// Another player as the snapshot shows them, with the team the roster gives.
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub id: u32,
    pub team: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub crouching: bool,
    /// Alive and past the spawn protection, so a shot can hurt them.
    pub targetable: bool,
    /// Big Head mode's King, whom nothing hurts.
    pub king: bool,
}

/// The bot itself, from its own `PlayerState` and the snapshot's per-recipient fields.
#[derive(Clone, Debug, Default)]
pub struct Me {
    pub id: u32,
    pub team: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub alive: bool,
    pub weapon: i32,
    pub magazine: u32,
    pub reserve: u32,
    pub reloading: bool,
}

fn wrap_angle(mut a: f32) -> f32 {
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

fn turn_towards(current: f32, target: f32, max_step: f32) -> f32 {
    current + wrap_angle(target - current).clamp(-max_step, max_step)
}

/// The yaw that looks along (dx, dz): yaw 0 looks down -z.
fn yaw_to(dx: f32, dz: f32) -> f32 {
    (-dx).atan2(-dz)
}

/// Whether `other` is someone this bot fights.
fn hostile(arena: &Arena, me: &Me, other: &Body) -> bool {
    other.id != me.id && (arena.ffa || other.team != me.team)
}

/// The slot to change to when the weapon in hand cannot shoot any more, if any: a spent taser or
/// an empty gun gives way to the primary (slot 1), then the pistol (2), then the knife (3).
fn fallback_slot(me: &Me, stats: &pb::WeaponStats) -> Option<u32> {
    if stats.melee_range > 0.0 || me.magazine > 0 || me.reloading {
        return None;
    }
    if me.weapon == pb::WeaponKind::Taser as i32 {
        return Some(1);
    }
    if me.reserve > 0 {
        return None;
    }
    Some(if me.weapon == pb::WeaponKind::Pistol as i32 {
        3
    } else {
        2
    })
}

/// A bot's memory between commands.
#[derive(Default)]
pub struct Brain {
    path: Vec<(f32, f32)>,
    path_index: usize,
    repath_in: f32,
    target: Option<u32>,
    tracked_for: f32,
    burst_left: u32,
    burst_pause: f32,
    strafe_left: bool,
    strafe_timer: f32,
    aim_error_yaw: f32,
    aim_error_pitch: f32,
    stuck_time: f32,
    last_x: f32,
    last_z: f32,
    /// Seconds until the weapon in hand can fire again, as far as the bot can tell.
    cooldown: f32,
    /// Decisions made so far, for the aim's wobble.
    steps: u32,
}

impl Brain {
    /// The next decision, `STEP_DT` after the last one. `render_tick` is the server tick the bot
    /// sees the others at; `seq` is left for the sender to number.
    pub fn think(
        &mut self,
        arena: &Arena,
        me: &Me,
        others: &[Body],
        render_tick: u32,
        rng: &mut Rng,
    ) -> pb::InputCmd {
        self.steps = self.steps.wrapping_add(1);
        self.cooldown = (self.cooldown - STEP_DT).max(0.0);
        let mut cmd = pb::InputCmd {
            yaw: me.yaw,
            pitch: me.pitch,
            render_tick,
            ..Default::default()
        };
        if !me.alive {
            self.path.clear();
            self.target = None;
            self.tracked_for = 0.0;
            return cmd;
        }
        let stats = arena.stats(me.weapon);
        if let Some(slot) = fallback_slot(me, &stats) {
            cmd.slot = slot;
        }
        // Nobody can hurt a King, so a bot goes for one only while it is a King itself.
        let king = me.weapon == pb::WeaponKind::Hammer as i32;
        let prey = |o: &&Body| hostile(arena, me, o) && o.targetable && (king || !o.king);
        let aim_y =
            |o: &Body| o.y + AIM_CHEST_Y - if o.crouching { arena.crouch_drop } else { 0.0 };

        let eye = [me.x, me.y + arena.eye_height, me.z];
        let mut best: Option<(&Body, f32)> = None;
        for other in others.iter().filter(prey) {
            let (dx, dz) = (other.x - me.x, other.z - me.z);
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > SIGHT_RANGE {
                continue;
            }
            let tracked = self.target == Some(other.id);
            let in_fov = wrap_angle(yaw_to(dx, dz) - me.yaw).abs() < SIGHT_HALF_ANGLE;
            if !(tracked || in_fov || dist < NEAR) {
                continue;
            }
            if !line_clear(&arena.boxes, eye, [other.x, aim_y(other), other.z]) {
                continue;
            }
            if best.is_none_or(|(_, d)| dist < d) {
                best = Some((other, dist));
            }
        }

        if let Some((enemy, dist)) = best {
            self.fight(arena, me, enemy, dist, aim_y(enemy), &stats, rng, &mut cmd);
        } else {
            self.target = None;
            if me.magazine < stats.magazine / 2 && me.reserve > 0 {
                cmd.reload = true;
            }
            self.patrol(arena, me, others, king, rng, &mut cmd);
        }
        self.last_x = me.x;
        self.last_z = me.z;
        cmd
    }

    #[allow(clippy::too_many_arguments)]
    fn fight(
        &mut self,
        arena: &Arena,
        me: &Me,
        enemy: &Body,
        dist: f32,
        enemy_aim_y: f32,
        stats: &pb::WeaponStats,
        rng: &mut Rng,
        cmd: &mut pb::InputCmd,
    ) {
        if self.target != Some(enemy.id) {
            self.target = Some(enemy.id);
            self.tracked_for = 0.0;
            self.aim_error_yaw = rng.range(-INITIAL_AIM_ERROR, INITIAL_AIM_ERROR);
            self.aim_error_pitch = rng.range(-INITIAL_AIM_ERROR, INITIAL_AIM_ERROR) * 0.6;
            self.burst_left = 0;
            self.burst_pause = rng.range(REACTION_MIN, REACTION_MAX);
        }
        self.tracked_for += STEP_DT;
        self.path.clear();

        let settle = (1.0 - AIM_SETTLE_RATE * STEP_DT).max(0.0);
        self.aim_error_yaw *= settle;
        self.aim_error_pitch *= settle;

        let wobble = AIM_WOBBLE * (self.steps as f32 * 0.17 + me.id as f32).sin();
        let want_yaw = yaw_to(enemy.x - me.x, enemy.z - me.z) + self.aim_error_yaw + wobble;
        let eye_y = me.y + arena.eye_height;
        let want_pitch = (enemy_aim_y - eye_y).atan2(dist) + self.aim_error_pitch;
        cmd.yaw = turn_towards(me.yaw, want_yaw, TURN_RATE * STEP_DT);
        cmd.pitch = want_pitch.clamp(-MAX_PITCH, MAX_PITCH);

        let melee = stats.melee_range > 0.0;
        if melee {
            // Straight in, until close enough to swing.
            cmd.forward = dist > stats.melee_range * 0.5;
        } else {
            // Side-step while shooting.
            self.strafe_timer -= STEP_DT;
            if self.strafe_timer <= 0.0 {
                self.strafe_timer = rng.range(0.5, 1.4);
                self.strafe_left = rng.next_f32() < 0.5;
            }
            cmd.left = self.strafe_left;
            cmd.right = !self.strafe_left;
            cmd.forward = dist > CLOSE_IN;
        }

        cmd.aim = stats.has_scope;
        let on_target = wrap_angle(want_yaw - cmd.yaw).abs() < 0.1;
        if self.burst_pause > 0.0 {
            self.burst_pause -= STEP_DT;
        } else if melee {
            cmd.fire = on_target && dist <= stats.melee_range;
        } else if on_target && me.magazine > 0 {
            if self.burst_left == 0 {
                // A bolt-action fires single aimed shots.
                self.burst_left = if stats.has_scope {
                    1
                } else {
                    (rng.index(6) as u32).saturating_add(4)
                };
            }
            cmd.fire = true;
            if self.cooldown <= 0.0 && !me.reloading {
                self.cooldown = stats.fire_interval;
                self.burst_left = self.burst_left.saturating_sub(1);
                if self.burst_left == 0 {
                    self.burst_pause = rng.range(0.35, 0.8);
                }
            }
        }
    }

    fn patrol(
        &mut self,
        arena: &Arena,
        me: &Me,
        others: &[Body],
        king: bool,
        rng: &mut Rng,
        cmd: &mut pb::InputCmd,
    ) {
        // Head for a random open spot or towards a random enemy. A King hunts: always an enemy,
        // and a fresh route every few seconds, as its prey moves.
        self.repath_in -= STEP_DT;
        let finished = self.path_index >= self.path.len();
        if finished || self.repath_in <= 0.0 {
            let enemies: Vec<&Body> = others
                .iter()
                .filter(|o| hostile(arena, me, o) && o.targetable && !o.king)
                .collect();
            let goal = if !enemies.is_empty() && (king || rng.next_f32() < 0.45) {
                let enemy = enemies[rng.index(enemies.len())];
                Some((enemy.x, enemy.z))
            } else {
                arena.nav.random_open(rng)
            };
            self.path = goal
                .map(|goal| arena.nav.path((me.x, me.z), goal))
                .unwrap_or_default();
            self.path_index = 0;
            self.repath_in = if king {
                rng.range(1.5, 3.0)
            } else {
                rng.range(5.0, 10.0)
            };
        }

        while let Some(&(wx, wz)) = self.path.get(self.path_index) {
            if (wx - me.x).powi(2) + (wz - me.z).powi(2) < WAYPOINT_REACH * WAYPOINT_REACH {
                self.path_index = self.path_index.saturating_add(1);
            } else {
                break;
            }
        }
        if let Some(&(wx, wz)) = self.path.get(self.path_index) {
            let want = yaw_to(wx - me.x, wz - me.z);
            cmd.yaw = turn_towards(me.yaw, want, TURN_RATE * 1.4 * STEP_DT);
            cmd.pitch = me.pitch * 0.9;
            cmd.forward = wrap_angle(want - cmd.yaw).abs() < 0.9;
        }

        // Anything that stops progress forces a new route.
        let moved = (me.x - self.last_x).abs() + (me.z - self.last_z).abs();
        self.stuck_time = if cmd.forward && moved < 0.01 {
            self.stuck_time + STEP_DT
        } else {
            0.0
        };
        if self.stuck_time > STUCK_AFTER {
            self.path.clear();
            self.stuck_time = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYE: f32 = 1.42;

    fn rifle() -> pb::WeaponStats {
        pb::WeaponStats {
            kind: pb::WeaponKind::Rifle as i32,
            magazine: 30,
            reserve: 90,
            fire_interval: 0.1,
            ..Default::default()
        }
    }

    fn knife() -> pb::WeaponStats {
        pb::WeaponStats {
            kind: pb::WeaponKind::Knife as i32,
            melee_range: 2.0,
            ..Default::default()
        }
    }

    fn arena(boxes: Vec<pb::MapBox>, ffa: bool) -> Arena {
        Arena {
            nav: NavGrid::new(&boxes, 20.0, 20.0, 1.8),
            boxes,
            eye_height: EYE,
            crouch_drop: 0.5,
            ffa,
            weapons: vec![rifle(), knife()],
        }
    }

    fn me() -> Me {
        Me {
            id: 1,
            team: pb::Team::Red as i32,
            alive: true,
            weapon: pb::WeaponKind::Rifle as i32,
            magazine: 30,
            reserve: 90,
            ..Default::default()
        }
    }

    /// An enemy straight ahead (down -z) `dist` metres away.
    fn enemy_ahead(dist: f32) -> Body {
        Body {
            id: 2,
            team: pb::Team::Blue as i32,
            z: -dist,
            targetable: true,
            ..Default::default()
        }
    }

    /// Commands for `seconds`, the bot standing still, as each one would leave it facing.
    fn run(
        brain: &mut Brain,
        arena: &Arena,
        me: &mut Me,
        others: &[Body],
        seconds: f32,
    ) -> Vec<pb::InputCmd> {
        let mut rng = Rng::new(5);
        let steps = (seconds / STEP_DT) as usize;
        let mut cmds = Vec::with_capacity(steps);
        for _ in 0..steps {
            let cmd = brain.think(arena, me, others, 0, &mut rng);
            me.yaw = cmd.yaw;
            me.pitch = cmd.pitch;
            cmds.push(cmd);
        }
        cmds
    }

    #[test]
    fn an_angle_wraps_within_a_half_turn_and_a_turn_takes_the_short_way() {
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-5);
        assert!((turn_towards(3.0, -3.0, 0.1) - 3.1).abs() < 1e-6);
        assert_eq!(turn_towards(0.0, 0.05, 0.1), 0.05);
    }

    #[test]
    fn the_yaw_to_a_point_ahead_is_zero_and_to_the_left_a_quarter_turn() {
        assert_eq!(yaw_to(0.0, -1.0), 0.0);
        assert!((yaw_to(-1.0, 0.0) - PI / 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_bot_shoots_an_enemy_in_view_after_reacting() {
        let arena = arena(Vec::new(), false);
        let (mut brain, mut me) = (Brain::default(), me());
        let cmds = run(&mut brain, &arena, &mut me, &[enemy_ahead(10.0)], 2.0);
        let first = cmds.iter().position(|c| c.fire).expect("it never fired");
        assert!(
            first as f32 * STEP_DT >= REACTION_MIN,
            "it fired after {} s, before it could react",
            first as f32 * STEP_DT
        );
        assert!(
            cmds.iter().all(|c| c.left != c.right),
            "it strafes one way at a time"
        );
    }

    #[test]
    fn a_bot_holds_fire_at_a_teammate_and_at_an_enemy_behind_a_wall() {
        let mut friend = enemy_ahead(10.0);
        friend.team = pb::Team::Red as i32;
        let wall = pb::MapBox {
            min_x: -5.0,
            max_x: 5.0,
            min_y: 0.0,
            max_y: 3.0,
            min_z: -6.0,
            max_z: -5.0,
            ..Default::default()
        };
        for (arena, others) in [
            (arena(Vec::new(), false), vec![friend]),
            (arena(vec![wall], false), vec![enemy_ahead(10.0)]),
        ] {
            let (mut brain, mut me) = (Brain::default(), me());
            let cmds = run(&mut brain, &arena, &mut me, &others, 2.0);
            assert!(cmds.iter().all(|c| !c.fire));
        }
    }

    #[test]
    fn in_a_free_for_all_a_bot_shoots_its_own_team_but_never_itself() {
        let mut other = enemy_ahead(10.0);
        other.team = pb::Team::Red as i32;
        let arena = arena(Vec::new(), true);
        let (mut brain, mut me) = (Brain::default(), me());
        let cmds = run(&mut brain, &arena, &mut me, &[other], 2.0);
        assert!(cmds.iter().any(|c| c.fire));

        let mut myself = enemy_ahead(10.0);
        myself.id = me.id;
        let mut brain = Brain::default();
        let cmds = run(&mut brain, &arena, &mut me, &[myself], 2.0);
        assert!(cmds.iter().all(|c| !c.fire));
    }

    #[test]
    fn a_bot_leaves_a_protected_player_or_a_king_alone() {
        let arena = arena(Vec::new(), false);
        let mut protected = enemy_ahead(10.0);
        protected.targetable = false;
        let mut king = enemy_ahead(10.0);
        king.king = true;
        for other in [protected, king] {
            let (mut brain, mut me) = (Brain::default(), me());
            let cmds = run(&mut brain, &arena, &mut me, &[other], 2.0);
            assert!(cmds.iter().all(|c| !c.fire));
        }
    }

    #[test]
    fn a_bot_turns_to_an_enemy_close_behind_it() {
        let arena = arena(Vec::new(), false);
        let (mut brain, mut me) = (Brain::default(), me());
        let mut behind = enemy_ahead(-3.0);
        behind.z = 3.0;
        let cmds = run(&mut brain, &arena, &mut me, &[behind], 2.0);
        let last = cmds.last().unwrap();
        assert!(wrap_angle(last.yaw - PI).abs() < 0.2, "facing {}", last.yaw);
    }

    #[test]
    fn a_knife_runs_in_and_swings_only_within_reach() {
        let arena = arena(Vec::new(), false);
        let (mut brain, mut me) = (Brain::default(), me());
        me.weapon = pb::WeaponKind::Knife as i32;
        let far = run(&mut brain, &arena, &mut me, &[enemy_ahead(10.0)], 2.0);
        assert!(far.iter().all(|c| !c.fire));
        assert!(far.last().unwrap().forward, "it runs at the enemy");
        let near = run(&mut brain, &arena, &mut me, &[enemy_ahead(1.5)], 2.0);
        assert!(near.iter().any(|c| c.fire));
    }

    #[test]
    fn a_bot_alone_reloads_a_half_empty_gun_and_walks_a_route() {
        let arena = arena(Vec::new(), false);
        let (mut brain, mut me) = (Brain::default(), me());
        me.magazine = 5;
        let cmds = run(&mut brain, &arena, &mut me, &[], 1.0);
        assert!(cmds[0].reload);
        assert!(cmds.iter().any(|c| c.forward), "it never set off");
    }

    #[test]
    fn an_empty_gun_changes_to_the_pistol_then_the_knife_and_a_spent_taser_to_the_primary() {
        let stats = rifle();
        let empty = Me {
            magazine: 0,
            reserve: 0,
            ..me()
        };
        assert_eq!(fallback_slot(&empty, &stats), Some(2));
        let pistol = Me {
            weapon: pb::WeaponKind::Pistol as i32,
            ..empty.clone()
        };
        assert_eq!(fallback_slot(&pistol, &stats), Some(3));
        let taser = Me {
            weapon: pb::WeaponKind::Taser as i32,
            reserve: 3,
            ..empty.clone()
        };
        assert_eq!(fallback_slot(&taser, &stats), Some(1));
        assert_eq!(fallback_slot(&me(), &stats), None, "a loaded gun stays");
        let spare = Me {
            reserve: 30,
            ..empty
        };
        assert_eq!(
            fallback_slot(&spare, &stats),
            None,
            "an empty magazine is reloaded"
        );
        assert_eq!(
            fallback_slot(&spare, &knife()),
            None,
            "a knife never runs dry"
        );
    }

    #[test]
    fn a_dead_bot_sends_an_idle_command_and_forgets_its_target() {
        let arena = arena(Vec::new(), false);
        let (mut brain, mut me) = (Brain::default(), me());
        run(&mut brain, &arena, &mut me, &[enemy_ahead(10.0)], 1.0);
        me.alive = false;
        let cmd = brain.think(&arena, &me, &[enemy_ahead(10.0)], 0, &mut Rng::new(1));
        assert!(!cmd.fire && !cmd.forward && !cmd.left && !cmd.right);
        assert_eq!(brain.target, None);
    }
}
