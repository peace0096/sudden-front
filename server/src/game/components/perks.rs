//! Augments and Big Head mode state a player carries, and the cards on offer.

use crate::pb;
use bevy_ecs::prelude::*;

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
    /// Big Head mode: the level, big_head::MIN_LEVEL to MAX_LEVEL, which sets `head_scale`; 0 in
    /// other modes. Survives respawns.
    pub level: i32,
    /// Big Head mode: ticks of the King's reign left; 0 when not the King.
    pub king_ticks: u32,
}

impl Perks {
    pub fn has(&self, augment: pb::Augment) -> bool {
        self.active.contains(&augment)
    }

    /// Big Head mode: the reigning King, who takes no harm.
    pub fn king(&self) -> bool {
        self.king_ticks > 0
    }

    /// Big Head mode: moves to `level`, the head growing or shrinking to its size.
    pub fn set_level(&mut self, level: i32) {
        self.level = level;
        self.head_scale = crate::game::big_head::head_scale(level);
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
