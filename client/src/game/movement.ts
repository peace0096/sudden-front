// Client-side prediction. Mirrors `step_move` and the movement block of
// `consume_inputs` in the server's game module; keep them in sync.
import type { GameConfig, MapBox } from "../gen/game_pb";

export const CMD_DT = 1 / 60;

// See CONTACT_EPS in the server's physics.rs.
const CONTACT_EPS = 1e-3;

export interface MoveInput {
  forward: boolean;
  back: boolean;
  left: boolean;
  right: boolean;
  yaw: number;
}

export interface Vec2 {
  x: number;
  z: number;
}

export function applyMove(pos: Vec2, input: MoveInput, boxes: MapBox[], config: GameConfig, speed: number): void {
  const fwd = Number(input.forward) - Number(input.back);
  const strafe = Number(input.right) - Number(input.left);
  if (fwd === 0 && strafe === 0) return;
  const sin = Math.sin(input.yaw);
  const cos = Math.cos(input.yaw);
  let dx = -sin * fwd + cos * strafe;
  let dz = -cos * fwd - sin * strafe;
  const scale = (speed * CMD_DT) / Math.hypot(dx, dz);
  dx *= scale;
  dz *= scale;

  const r = config.playerHalfWidth;
  const e = CONTACT_EPS;
  const oldX = pos.x;
  pos.x += dx;
  if (dx !== 0) {
    for (const b of boxes) {
      if (b.minY < config.playerHeight && pos.z + r - e > b.minZ && pos.z - r + e < b.maxZ) {
        if (dx > 0 && oldX + r <= b.minX + e && pos.x + r > b.minX) pos.x = b.minX - r;
        else if (dx < 0 && oldX - r >= b.maxX - e && pos.x - r < b.maxX) pos.x = b.maxX + r;
      }
    }
  }
  const oldZ = pos.z;
  pos.z += dz;
  if (dz !== 0) {
    for (const b of boxes) {
      if (b.minY < config.playerHeight && pos.x + r - e > b.minX && pos.x - r + e < b.maxX) {
        if (dz > 0 && oldZ + r <= b.minZ + e && pos.z + r > b.minZ) pos.z = b.minZ - r;
        else if (dz < 0 && oldZ - r >= b.maxZ - e && pos.z - r < b.maxZ) pos.z = b.maxZ + r;
      }
    }
  }
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
