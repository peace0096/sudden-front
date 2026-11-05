// Client-side prediction. Mirrors `step_move`, `step_vertical` and the movement block of
// `consume_inputs` in the server's game module; keep them in sync.
import type { GameConfig, MapBox } from "../gen/game_pb";

export const CMD_DT = 1 / 60;

// See CONTACT_EPS in the server's physics.rs.
const CONTACT_EPS = 1e-3;
// See FOOTING in the server's physics.rs: a box top this little above the feet is underfoot.
const FOOTING = 0.01;

export interface MoveInput {
  forward: boolean;
  back: boolean;
  left: boolean;
  right: boolean;
  yaw: number;
  /** Space held: a jump, if standing. */
  jump: boolean;
}

/** The predicted player: the feet at (x, y, z), and the vertical speed, 0 while standing. */
export interface Body {
  x: number;
  y: number;
  z: number;
  vy: number;
}

/** Whether a box is in the way of a player whose feet are at `feet` (the server's `blocks_at`). */
function blocksAt(b: MapBox, feet: number, height: number): boolean {
  return b.minY < feet + height && b.maxY > feet + FOOTING;
}

/** Whether a box lies under any of a player's footprint at (x, z). */
function under(b: MapBox, x: number, z: number, r: number): boolean {
  const e = CONTACT_EPS;
  return x + r - e > b.minX && x - r + e < b.maxX && z + r - e > b.minZ && z - r + e < b.maxZ;
}

/** What a player at (x, z) with their feet at `feet` stands or would land on (the server's `ground_height`). */
export function groundHeight(boxes: MapBox[], x: number, z: number, feet: number, r: number): number {
  let ground = 0;
  for (const b of boxes) if (b.maxY <= feet + FOOTING && b.maxY > ground && under(b, x, z, r)) ground = b.maxY;
  return ground;
}

/** The underside of the lowest box over a player's head (the server's `ceiling_height`). */
function ceilingHeight(boxes: MapBox[], x: number, z: number, feet: number, r: number, height: number): number | null {
  let top: number | null = null;
  for (const b of boxes) if (b.minY >= feet + height - FOOTING && under(b, x, z, r) && (top === null || b.minY < top)) top = b.minY;
  return top;
}

/** One command's rise or fall, after the step across (the server's `step_vertical`). */
function stepVertical(body: Body, jump: boolean, boxes: MapBox[], config: GameConfig): void {
  const r = config.playerHalfWidth;
  const ground = groundHeight(boxes, body.x, body.z, body.y, r);
  if (jump && body.vy === 0 && body.y <= ground + FOOTING) body.vy = config.jumpSpeed;
  body.vy -= config.gravity * CMD_DT;
  let next = body.y + body.vy * CMD_DT;
  if (body.vy > 0) {
    const top = ceilingHeight(boxes, body.x, body.z, body.y, r, config.playerHeight);
    if (top !== null && next + config.playerHeight > top) {
      next = top - config.playerHeight;
      body.vy = 0;
    }
  }
  if (next <= ground) {
    next = ground;
    body.vy = 0;
  }
  body.y = next;
}

/** One command: a step across, sliding along whatever is in the way at this height, then up or down. */
export function applyMove(body: Body, input: MoveInput, boxes: MapBox[], config: GameConfig, speed: number): void {
  const fwd = Number(input.forward) - Number(input.back);
  const strafe = Number(input.right) - Number(input.left);
  if (fwd !== 0 || strafe !== 0) {
    const sin = Math.sin(input.yaw);
    const cos = Math.cos(input.yaw);
    let dx = -sin * fwd + cos * strafe;
    let dz = -cos * fwd - sin * strafe;
    const scale = (speed * CMD_DT) / Math.hypot(dx, dz);
    dx *= scale;
    dz *= scale;

    const r = config.playerHalfWidth;
    const h = config.playerHeight;
    const e = CONTACT_EPS;
    const feet = body.y;
    const oldX = body.x;
    body.x += dx;
    if (dx !== 0) {
      for (const b of boxes) {
        if (blocksAt(b, feet, h) && body.z + r - e > b.minZ && body.z - r + e < b.maxZ) {
          if (dx > 0 && oldX + r <= b.minX + e && body.x + r > b.minX) body.x = b.minX - r;
          else if (dx < 0 && oldX - r >= b.maxX - e && body.x - r < b.maxX) body.x = b.maxX + r;
        }
      }
    }
    const oldZ = body.z;
    body.z += dz;
    if (dz !== 0) {
      for (const b of boxes) {
        if (blocksAt(b, feet, h) && body.x + r - e > b.minX && body.x - r + e < b.maxX) {
          if (dz > 0 && oldZ + r <= b.minZ + e && body.z + r > b.minZ) body.z = b.minZ - r;
          else if (dz < 0 && oldZ - r >= b.maxZ - e && body.z - r < b.maxZ) body.z = b.maxZ + r;
        }
      }
    }
  }
  stepVertical(body, input.jump, boxes, config);
}

/** Distance to the first box along a ray, or null. */
export function rayWorld(
  boxes: MapBox[],
  o: [number, number, number],
  d: [number, number, number],
  maxDist: number,
): number | null {
  let best: number | null = null;
  for (const b of boxes) {
    const t = rayBox(o, d, [b.minX, b.minY, b.minZ], [b.maxX, b.maxY, b.maxZ]);
    if (t !== null && t <= maxDist && (best === null || t < best)) best = t;
  }
  return best;
}

export function rayBox(
  o: [number, number, number],
  d: [number, number, number],
  min: [number, number, number],
  max: [number, number, number],
): number | null {
  let tMin = 0;
  let tMax = Infinity;
  for (let i = 0; i < 3; i++) {
    if (Math.abs(d[i]) < 1e-7) {
      if (o[i] < min[i] || o[i] > max[i]) return null;
      continue;
    }
    let t0 = (min[i] - o[i]) / d[i];
    let t1 = (max[i] - o[i]) / d[i];
    if (t0 > t1) [t0, t1] = [t1, t0];
    tMin = Math.max(tMin, t0);
    tMax = Math.min(tMax, t1);
    if (tMin > tMax) return null;
  }
  return tMin;
}
