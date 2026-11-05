// Client copy of the server's hitboxes: `hitboxes` and `ray_shield` in server/src/game/systems.rs,
// CROUCH_DROP in server/src/game/mod.rs. The server judges every hit; models are fitted to these.
// Heights are metres above the feet and half-widths are measured from the player's centre. The
// boxes are axis-aligned and never turn with the player.

/** A crouch lowers every hitbox top, and the head with it, by this much. */
export const CROUCH_DROP = 0.5;

/** The head box starts at the neck; the head-size augments scale its height and width by headScale. */
export const HEAD_BASE = 1.5;
export const HEAD_HEIGHT = 0.26;
export const HEAD_HALF_WIDTH = 0.125;
/** The largest head the Big Head augment grows (server augments.rs BIG_HEAD_MAX): the character's bounds hold it. */
export const HEAD_SCALE_MAX = 3;

export const BODY_BOTTOM = 0.85;
export const BODY_TOP = 1.5;
export const BODY_HALF_WIDTH = 0.28;

export const LEGS_BOTTOM = 0;
export const LEGS_TOP = 0.85;
export const LEGS_HALF_WIDTH = 0.22;

// A shield bearer's shield, in their own frame where they face -Z: a slab from SHIELD_NEAR to
// SHIELD_FAR in front of them, from the floor to SHIELD_TOP (lowered by CROUCH_DROP in a crouch).
export const SHIELD_HALF_WIDTH = 0.45;
export const SHIELD_NEAR = 0.55;
export const SHIELD_FAR = 0.65;
export const SHIELD_TOP = 1.75;
