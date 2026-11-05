use super::nav::NavGrid;
use crate::pb;
use bevy_ecs::prelude::*;
use std::collections::VecDeque;

#[derive(Component)]
pub struct Player {
    pub id: u32,
    pub name: String,
    pub team: pb::Team,
    pub is_bot: bool,
}

#[derive(Component, Clone, Copy, Default)]
pub struct Pose {
    pub x: f32,
    /// Feet height above the floor: a box's top while standing on it, more in the air.
    pub y: f32,
    pub z: f32,
    /// Vertical speed, m/s up; 0 while standing.
    pub vy: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub crouching: bool,
}

#[derive(Component)]
pub struct Vitals {
    pub hp: i32,
    pub alive: bool,
    pub respawn_left: f32,
    pub protect_left: f32,
    /// Seconds of paralysis left: no moving, firing, reloading or switching.
    pub stun_left: f32,
}

#[derive(Component)]
pub struct Weapon {
    /// The weapon in hand; `magazine`, `reserve` and the timers belong to it.
    pub kind: pb::WeaponKind,
    /// What slot 1 holds for this life.
    pub primary: pb::WeaponKind,
    /// What slot 2 holds.
    pub sidearm: pb::WeaponKind,
    /// Index of the slot in hand, and the ammo left in the holstered ones. Slot 4's (index 3)
    /// magazine is the grenades left.
    pub active_slot: usize,
    pub holstered: [(u32, u32); 4],
    /// Index of the slot in hand before the grenade was drawn: a throw goes back to it.
    pub last_slot: usize,
    /// Seconds the scope has been steady; see `SCOPE_READY_TIME`.
    pub aim_time: f32,
    pub magazine: u32,
    pub reserve: u32,
    pub cooldown: f32,
    pub reload_left: f32,
    /// Extra cone angle from sustained fire, in radians.
    pub bloom: f32,
    /// Seconds until bloom starts recovering; each shot restarts this.
    pub bloom_hold: f32,
    /// Rounds still to come in the upgraded pistol's burst, and where it aims.
    pub burst_left: u32,
    pub burst_fire: Option<FireIntent>,
}

/// The weapon a player will get at their next spawn.
#[derive(Component)]
pub struct Loadout(pub pb::WeaponKind);

impl Weapon {
    pub fn new(kind: pb::WeaponKind) -> Self {
        Self::equipped(kind, &[])
    }

    /// A fresh weapon set for a player with these augments.
    pub fn equipped(kind: pb::WeaponKind, active: &[pb::Augment]) -> Self {
        let kind = super::weapons::primary_for(kind, active);
        let stats = super::weapons::stats(kind);
        let sidearm = super::weapons::sidearm_for(active);
        let side = super::weapons::stats(sidearm);
        let grenades = super::weapons::stats(pb::WeaponKind::Grenade).magazine;
        Self {
            kind,
            primary: kind,
            sidearm,
            active_slot: 0,
            holstered: [
                (stats.magazine, stats.reserve),
                (side.magazine, side.reserve),
                (0, 0),
                (grenades, 0),
            ],
            last_slot: 0,
            aim_time: 0.0,
            magazine: stats.magazine,
            reserve: stats.reserve,
            cooldown: 0.0,
            reload_left: 0.0,
            bloom: 0.0,
            bloom_hold: 0.0,
            burst_left: 0,
            burst_fire: None,
        }
    }

    /// Replaces what slot 1 holds, with a full load.
    pub fn set_primary(&mut self, kind: pb::WeaponKind) {
        let stats = super::weapons::stats(kind);
        self.primary = kind;
        if self.active_slot == 0 {
            self.kind = kind;
            self.magazine = stats.magazine;
            self.reserve = stats.reserve;
            self.reload_left = 0.0;
            self.aim_time = 0.0;
            self.bloom = 0.0;
            self.bloom_hold = 0.0;
            self.burst_left = 0;
            self.cooldown = self.cooldown.max(super::DRAW_TIME);
        } else {
            self.holstered[0] = (stats.magazine, stats.reserve);
        }
    }

    /// Takes a slot (1 primary, 2 pistol, 3 knife, 4 grenade) in hand. Ammo stays with each
    /// weapon, a reload in progress is abandoned, and the new weapon must be drawn. The grenade
    /// only comes out while one is left.
    pub fn switch_to(&mut self, slot: u32) {
        let (index, kind) = match slot {
            1 => (0, self.primary),
            2 => (1, self.sidearm),
            3 => (2, pb::WeaponKind::Knife),
            4 if self.grenades() > 0 => (GRENADE_SLOT, pb::WeaponKind::Grenade),
            _ => return,
        };
        if index == self.active_slot {
            return;
        }
        if index == GRENADE_SLOT {
            self.last_slot = self.active_slot;
        }
        self.holstered[self.active_slot] = (self.magazine, self.reserve);
        (self.magazine, self.reserve) = self.holstered[index];
        self.active_slot = index;
        self.kind = kind;
        self.reload_left = 0.0;
        self.aim_time = 0.0;
        self.bloom = 0.0;
        self.bloom_hold = 0.0;
        self.burst_left = 0;
        self.cooldown = self.cooldown.max(super::DRAW_TIME);
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }

    /// Grenades left to throw, in hand or not.
    pub fn grenades(&self) -> u32 {
        if self.active_slot == GRENADE_SLOT {
            self.magazine
        } else {
            self.holstered[GRENADE_SLOT].0
        }
    }

    /// The slot (1 to 3) a throw goes back to.
    pub fn slot_before_grenade(&self) -> u32 {
        self.last_slot as u32 + 1
    }
}

/// Index of the grenade's slot (4) in `Weapon::holstered`.
pub const GRENADE_SLOT: usize = 3;

/// The chosen augment and the state it keeps across a match.
#[derive(Component, Default)]
pub struct Perks {
    /// The card chosen.
    pub augment: pb::Augment,
    /// What it grants: itself, or the two Double Time drew.
    pub active: Vec<pb::Augment>,
    pub shield_hp: i32,
    /// Size of the head and its hitbox; 1 is normal. Survives respawns.
    pub head_scale: f32,
    /// Who has hurt this player in this life.
    pub damaged_by: Vec<Entity>,
    /// Seconds since the last shot or hit.
    pub idle: f32,
    /// Fractional HP from Rest, carried between ticks.
    pub regen: f32,
}

impl Perks {
    pub fn has(&self, augment: pb::Augment) -> bool {
        self.active.contains(&augment)
    }
}

/// Cards on the table for a player who hasn't chosen yet.
#[derive(Component, Default)]
pub struct AugmentOffer {
    pub offered: Vec<pb::Augment>,
    pub seen: Vec<pb::Augment>,
    /// Which of the cards has been swapped; each can be, once.
    pub refreshed: [bool; 3],
}

#[derive(Component, Default)]
pub struct Inputs {
    pub queue: VecDeque<pb::InputCmd>,
    pub last_seq: u32,
    pub budget: u32,
}

/// What the player's commands asked for this tick.
#[derive(Component, Default)]
pub struct Intent {
    pub fire: Option<FireIntent>,
    pub reload: bool,
    pub moving: bool,
    pub aim: bool,
}

#[derive(Clone, Copy)]
pub struct FireIntent {
    pub yaw: f32,
    pub pitch: f32,
    pub render_tick: u32,
}

#[derive(Component, Default)]
pub struct Stats {
    pub kills: u32,
    pub deaths: u32,
}

/// Recent positions for rewinding targets under lag compensation.
#[derive(Component, Default)]
pub struct History {
    /// (tick, x, feet height, z, crouching, yaw).
    pub samples: VecDeque<(u32, f32, f32, f32, bool, f32)>,
}

impl History {
    /// (x, feet height, z, crouching, yaw) as of `tick`.
    pub fn at(&self, tick: u32) -> Option<(f32, f32, f32, bool, f32)> {
        self.samples
            .iter()
            .rev()
            .find(|(t, ..)| *t <= tick)
            .map(|(_, x, y, z, crouching, yaw)| (*x, *y, *z, *crouching, *yaw))
    }
}

#[derive(Component, Default)]
pub struct BotBrain {
    pub path: Vec<(f32, f32)>,
    pub path_index: usize,
    pub repath_in: f32,
    pub target: Option<Entity>,
    pub tracked_for: f32,
    pub burst_left: u32,
    pub burst_pause: f32,
    pub strafe_left: bool,
    pub strafe_timer: f32,
    pub aim_error_yaw: f32,
    pub aim_error_pitch: f32,
    pub stuck_time: f32,
    pub last_x: f32,
    pub last_z: f32,
    /// Taking out the grenade to throw it at the target, or about to.
    pub throwing: bool,
}

#[derive(Resource)]
pub struct Arena {
    pub map: pb::MapData,
    pub nav: NavGrid,
    pub spawns: Vec<(f32, f32)>,
    /// Where anyone may spawn in a free-for-all, in world coordinates.
    pub ffa_spawns: Vec<(f32, f32)>,
    pub lanes: Vec<(f32, f32)>,
}

/// What differs between game modes and match types.
#[derive(Resource)]
pub struct Rules {
    pub mode: pb::GameMode,
    pub max_hp: i32,
    /// Free-for-all: every player's team is `Unspecified` and everyone fights everyone.
    pub ffa: bool,
}

impl Rules {
    /// Whether players on these teams can hurt each other. Callers still rule out hurting
    /// yourself: in a free-for-all everyone is on the same (no) team.
    pub fn hostile(&self, a: pb::Team, b: pb::Team) -> bool {
        self.ffa || a != b
    }
}

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

/// A missile in flight.
pub struct Missile {
    pub id: u32,
    pub owner: Entity,
    pub owner_id: u32,
    pub team: pb::Team,
    pub pos: [f32; 3],
    pub dir: [f32; 3],
    pub life: f32,
}

#[derive(Resource, Default)]
pub struct Missiles {
    pub next_id: u32,
    pub live: Vec<Missile>,
}

/// A thrown grenade: flying, bouncing, rolling or lying still until its fuse runs out.
pub struct Grenade {
    pub id: u32,
    pub owner: Entity,
    pub owner_id: u32,
    pub team: pb::Team,
    /// The centre of the grenade, which keeps `GRENADE_RADIUS` off every surface.
    pub pos: [f32; 3],
    /// m/s; all 0 once it lies still.
    pub vel: [f32; 3],
    /// Ticks until it goes off: whole ticks, so the fuse is exact.
    pub fuse_ticks: u32,
}

#[derive(Resource, Default)]
pub struct Grenades {
    pub next_id: u32,
    pub live: Vec<Grenade>,
}

#[derive(Resource, Default)]
pub struct EventLog(pub Vec<pb::GameEvent>);

impl EventLog {
    pub fn push(&mut self, event: pb::game_event::Event) {
        self.0.push(pb::GameEvent { event: Some(event) });
    }
}

/// Small seeded xorshift so matches and tests are reproducible.
#[derive(Resource)]
pub struct GameRng(pub u64);

impl GameRng {
    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    pub fn index(&mut self, len: usize) -> usize {
        ((self.next_f32() * len as f32) as usize).min(len.saturating_sub(1))
    }
}
