// What shots, hits and explosions leave in the world, in four draw calls at most however much is
// going on: smoke, dust and blood in one alpha-blended particle batch, flashes, fire and sparks in
// one additive batch, tracers in a third, and bullet holes, blood splats and scorch marks in a ring
// of decals on the map's boxes (particles.ts, decals.ts). Every pool is sized once, so nothing is
// allocated per shot; a full pool gives up its most faded particle or its oldest mark. Shapes come
// from the art manifest's effect atlases (fx); colours are set here. No effect adds a light.
import * as THREE from "three";
import { BoxKind, type MapBox } from "../gen/game_pb";
import { DecalPool, HOLES, MARKS, firstFace, nearestSurface, struckSurface, touchingSurface, type DecalInfo, type DecalSpec, type Surface } from "./decals";
import { GLOW, ParticleBatch, TracerBatch, decalAtlas, particleAtlas, particleSpec, resetSpec, type ParticleSpec, type TracerSpec, type UvRect } from "./particles";

const ALPHA_CAPACITY = 384;
const ADDITIVE_CAPACITY = 192;
const TRACER_CAPACITY = 128;
/** How far behind a wounded player a wall still catches the blood. */
const SPLAT_REACH = 1.5;
/** How far from an explosion a surface still takes its scorch. */
const SCORCH_REACH = 1.2;
/**
 * Tracers fly this fast (m/s), slower than bullets so the eye can follow them, and take at least
 * TRACER_SHORTEST seconds over a path, so a close shot's tracer still shows for a few frames.
 */
const TRACER_SPEED = 330;
const TRACER_SHORTEST = 0.07;
/**
 * How far back along the flight a missile's smoke puff reaches: well past the one before it
 * (game.ts leaves one every 25 ms, 1.5 m apart at the server's 60 m/s; a puff's soft edge shows
 * little), so the trail has no gaps.
 */
const TRAIL_REACH = 2.6;

// Particle colours, linear from sRGB; light above 1 is brighter than white.
const FLASH = new THREE.Color(0xff9a45);
const FLASH_CORE = new THREE.Color(0xffd9a0);
const SPARK = new THREE.Color(0xffb257);
const SPARK_END = new THREE.Color(0xff5a1a);
const FIRE_HOT = new THREE.Color(0xffa94d);
const FIRE_COOL = new THREE.Color(0x9c2a0c);
const TRACER = new THREE.Color(0xffd08a);
const GUN_SMOKE = new THREE.Color(0xa7a29a);
const TRAIL_SMOKE = new THREE.Color(0xd4d0c8);
const BLAST_SMOKE = new THREE.Color(0x4a4641);
const BLAST_DEBRIS = new THREE.Color(0x3d3227);
const BLOOD_MIST = new THREE.Color(0xa8140e);
const BLOOD = new THREE.Color(0x8e0a06);
// What marks multiply a surface by: display colours, not converted.
const display = (hex: number) => new THREE.Color().setHex(hex, THREE.LinearSRGBColorSpace);
const HOLE_TINT = display(0x221e1a);
const BLOOD_TINT = display(0x7c100b);
const SCORCH_TINT = display(0x26221f);

/** What a struck surface throws off: its dust and chips, how many sparks, and whether it flashes. */
interface Stuff {
  dust: THREE.Color;
  debris: THREE.Color;
  sparks: number;
  flash: boolean;
}

const BRICK: Stuff = { dust: new THREE.Color(0xb59c8a), debris: new THREE.Color(0x6e5345), sparks: 2, flash: true };
const STONE: Stuff = { dust: new THREE.Color(0xb4ada0), debris: new THREE.Color(0x625c53), sparks: 2, flash: true };
const METAL: Stuff = { dust: new THREE.Color(0x9a9791), debris: new THREE.Color(0x4e4b47), sparks: 5, flash: true };
const WOOD: Stuff = { dust: new THREE.Color(0xb09270), debris: new THREE.Color(0x5e4429), sparks: 0, flash: false };
const SAND: Stuff = { dust: new THREE.Color(0xcab58e), debris: new THREE.Color(0x7a6444), sparks: 0, flash: false };
const FLOOR: Stuff = { dust: new THREE.Color(0xa6a29a), debris: new THREE.Color(0x55514b), sparks: 1, flash: false };

function stuffOf(kind: BoxKind): Stuff {
  switch (kind) {
    case BoxKind.BRICK:
      return BRICK;
    case BoxKind.CRATE:
      return WOOD;
    case BoxKind.CONTAINER_BLUE:
    case BoxKind.CONTAINER_GRAY:
    case BoxKind.CONTAINER_RUST:
    case BoxKind.CONTAINER_PALE:
    case BoxKind.BARREL:
    case BoxKind.INVISIBLE:
      return METAL;
    default:
      return STONE;
  }
}

/** Pool fill and the marks on the map, for the ?art bench and e2e/perf.mjs. */
export interface EffectsMeasure {
  pools: Record<"alpha" | "additive" | "tracers" | "decals", { count: number; capacity: number }>;
  decals: DecalInfo[];
}

export interface EffectsOptions {
  /** The warehouse: dimmer smoke and dust, and concrete underfoot. */
  indoor?: boolean;
  /** Numbers in [0, 1) for every random choice; the ?art bench passes a seeded one. */
  random?: () => number;
}

const UP = new THREE.Vector3(0, 1, 0);
const along = new THREE.Vector3();
const facing = new THREE.Vector3();
const bounce = new THREE.Vector3();
const spot = new THREE.Vector3();
const scratch = new THREE.Vector3();

export class Effects {
  /** Everything it draws, as one group named "effects" in the scene. */
  readonly group = new THREE.Group();
  private readonly alpha: ParticleBatch;
  private readonly additive: ParticleBatch;
  private readonly tracers: TracerBatch;
  private readonly decals: DecalPool;
  private readonly random: () => number;
  /** Unlit smoke and dust against the lit world: brighter outdoors, dimmer in the warehouse. */
  private readonly shade: number;
  private readonly ground: Stuff;
  private readonly cells: {
    muzzle: UvRect[];
    smoke: UvRect[];
    fire: UvRect[];
    spark: UvRect;
    flare: UvRect;
    impact: UvRect;
    dirt: UvRect;
    ring: UvRect;
    scorch: UvRect;
    blood: UvRect[];
    hole: UvRect;
  };
  private readonly spec = particleSpec();
  private readonly mark: DecalSpec = { cell: GLOW, atlas: 0, size: 1, rotation: 0, r: 1, g: 1, b: 1, alpha: 1, lane: HOLES };
  private readonly streak: TracerSpec = {
    speed: TRACER_SPEED,
    length: 6,
    shortest: TRACER_SHORTEST,
    width: 0.05,
    r: TRACER.r * 2,
    g: TRACER.g * 2,
    b: TRACER.b * 2,
    alpha: 0.9,
  };

  constructor(
    scene: THREE.Scene,
    private readonly boxes: MapBox[] = [],
    options: EffectsOptions = {},
  ) {
    this.random = options.random ?? Math.random;
    this.shade = options.indoor ? 0.62 : 1.2;
    this.ground = options.indoor ? FLOOR : SAND;
    const particles = particleAtlas();
    const decals = decalAtlas();
    this.cells = {
      muzzle: ["muzzle1", "muzzle2", "muzzle3"].map((name) => particles.cell(name)),
      smoke: ["smoke1", "smoke2", "smoke3"].map((name) => particles.cell(name)),
      fire: ["fire", "flame"].map((name) => particles.cell(name)),
      spark: particles.cell("spark"),
      flare: particles.cell("flare"),
      impact: particles.cell("impact"),
      dirt: particles.cell("dirt"),
      ring: particles.cell("ring"),
      scorch: particles.cell("scorch"),
      blood: ["bloodA", "bloodB", "bloodC"].map((name) => decals.cell(name)),
      hole: decals.cell("bulletHole"),
    };
    this.alpha = new ParticleBatch(particles, { capacity: ALPHA_CAPACITY, additive: false, renderOrder: 1 });
    this.tracers = new TracerBatch(particles, { capacity: TRACER_CAPACITY, additive: true, pixels: [2.2, 6], renderOrder: 2 });
    this.additive = new ParticleBatch(particles, { capacity: ADDITIVE_CAPACITY, additive: true, pixels: [1.5, 1e5], renderOrder: 3 });
    this.decals = new DecalPool(decals, particles);
    this.group.name = "effects";
    this.group.add(this.decals.mesh, this.alpha.mesh, this.tracers.mesh, this.additive.mesh);
    scene.add(this.group);
  }

  /**
   * A streak along a shot's path, flying from `from` to `to` and stopping there, from `startAt`
   * metres along it (the local player's own shots start out past the camera).
   */
  tracer(from: THREE.Vector3, to: THREE.Vector3, startAt = 0): void {
    this.streak.length = this.rand(5, 8);
    this.tracers.add(from, to, startAt, this.streak);
  }

  /**
   * A shot striking the map at `position`, fired from `from`: dust and chips off the surface,
   * sparks and a flash off stone and metal, and a bullet hole. Without `from` the face is the one
   * the point lies on. A knife (`melee`) only raises dust, and a spark or two off metal.
   */
  impact(position: THREE.Vector3, from?: THREE.Vector3, melee = false): void {
    const dir = this.direction(from, position);
    const surface = dir ? struckSurface(this.boxes, position, dir) : touchingSurface(this.boxes, position);
    const normal = this.normalOf(surface, dir);
    const stuff = surface ? (surface.box ? stuffOf(surface.box.kind) : this.ground) : STONE;
    const at = spot.copy(position).addScaledVector(normal, 0.04);

    for (let i = 0; i < (melee ? 1 : 2); i++) {
      const p = this.particle(this.pick(this.cells.smoke), at, this.rand(0.55, 0.85), this.rand(0.1, 0.14), this.rand(0.45, 0.65));
      this.scatter(p, normal, this.rand(0.5, 1.1), 0.45);
      p.vy += 0.15;
      p.drag = 3.2;
      p.gravity = 0.25;
      p.spin = this.rand(-1.2, 1.2);
      p.fadeOut = 1.6;
      this.tint(p, stuff.dust, 0.55);
      this.alpha.spawn(p);
    }
    if (!melee) {
      const chips = this.particle(this.cells.dirt, at, 0.42, 0.16, 0.3);
      this.scatter(chips, normal, 1.6, 0.5);
      chips.vy += 1.2;
      chips.gravity = 9;
      chips.drag = 1.2;
      chips.fadeOut = 2;
      this.tint(chips, stuff.debris, 0.9);
      this.alpha.spawn(chips);
    }
    if (stuff.flash && !melee) {
      const pop = this.particle(this.cells.impact, at, 0.05, 0.24, 0.3);
      this.light(pop, SPARK, 1.6, 0.9);
      this.additive.spawn(pop);
    }
    const sparks = melee ? (stuff === METAL ? 2 : 0) : stuff.sparks;
    // Glancing off the face the way the shot came.
    const off = dir ? bounce.copy(dir).reflect(normal).add(normal).normalize() : normal;
    for (let i = 0; i < sparks; i++) {
      const p = this.particle(GLOW, at, this.rand(0.14, 0.3), 0.025);
      this.scatter(p, off, this.rand(4, stuff === METAL ? 11 : 7), 0.7);
      p.gravity = 9.8;
      p.drag = 1.4;
      p.stretch = 0.035;
      this.light(p, SPARK, 1.7, 1, SPARK_END, 1);
      this.additive.spawn(p);
    }
    if (surface?.box && !melee) this.leave(surface, HOLES, this.cells.hole, 0, this.rand(0.085, 0.115), HOLE_TINT, 0.92);
  }

  /**
   * A hit on a player at `position`, shot from `from`: a mist of blood, drops flung on along the
   * shot, and a splat on a wall close behind.
   */
  bloodPuff(position: THREE.Vector3, from?: THREE.Vector3): void {
    const dir = this.direction(from, position);
    const forward = dir ?? UP;
    for (let i = 0; i < 2; i++) {
      const p = this.particle(this.pick(this.cells.smoke), position, this.rand(0.32, 0.45), this.rand(0.14, 0.18), this.rand(0.5, 0.65));
      this.scatter(p, forward, this.rand(0.5, 1), 0.6);
      p.drag = 4;
      p.gravity = 0.8;
      p.spin = this.rand(-1, 1);
      p.fadeOut = 1.3;
      this.tint(p, BLOOD_MIST, 0.8);
      this.alpha.spawn(p);
    }
    for (let i = 0; i < 8; i++) {
      const p = this.particle(GLOW, position, this.rand(0.3, 0.5), this.rand(0.045, 0.06));
      this.scatter(p, forward, this.rand(1.5, 4), 0.7);
      p.vy += 0.8;
      p.gravity = 9.8;
      p.drag = 0.6;
      p.stretch = 0.045;
      p.fadeOut = 3;
      this.tint(p, BLOOD, 1);
      this.alpha.spawn(p);
    }
    if (!dir) return;
    const wall = firstFace(this.boxes, position, dir, SPLAT_REACH);
    if (wall) this.leave(wall, MARKS, this.pick(this.cells.blood), 0, this.rand(0.55, 0.85), BLOOD_TINT, 0.9);
  }

  /**
   * A shot leaving a muzzle at `position`: the flame out along `direction` (when given), a star of
   * light seen from any side, and a wisp of smoke.
   */
  muzzleFlash(position: THREE.Vector3, direction?: THREE.Vector3): void {
    const dir = direction && direction.lengthSq() > 1e-8 ? along.copy(direction).normalize() : null;
    if (dir) {
      const flame = this.particle(this.pick(this.cells.muzzle), position, 0.05, 0.26);
      flame.length = this.rand(0.4, 0.55);
      flame.ax = dir.x;
      flame.ay = dir.y;
      flame.az = dir.z;
      this.light(flame, FLASH, 1.6);
      this.additive.spawn(flame);
    }
    const star = this.particle(this.cells.spark, position, 0.045, this.rand(0.32, 0.42));
    this.light(star, FLASH_CORE, 1.3);
    this.additive.spawn(star);
    const smoke = this.particle(this.pick(this.cells.smoke), position, this.rand(0.45, 0.6), 0.18, 0.55);
    if (dir) this.scatter(smoke, dir, 0.7, 0.3);
    smoke.vy += 0.3;
    smoke.drag = 2.5;
    smoke.gravity = -0.2;
    smoke.spin = this.rand(-1, 1);
    smoke.fadeOut = 1.5;
    this.tint(smoke, GUN_SMOKE, 0.22);
    this.alpha.spawn(smoke);
  }

  /**
   * A missile going off: a flash and a shock wave, a fireball thrown out and rising, sparks and
   * clods flung away from what it hit, black smoke welling up after, and a scorch mark on that.
   */
  explosion(position: THREE.Vector3): void {
    const surface = nearestSurface(this.boxes, position, SCORCH_REACH);
    // Thrown away from the surface it went off against; up when in the open.
    const away = surface ? facing.set(0, 0, 0).setComponent(surface.axis, surface.sign) : facing.copy(UP);
    if (surface) this.leave(surface, MARKS, this.cells.scorch, 1, this.rand(2.4, 3), SCORCH_TINT, 0.88);

    const flash = this.particle(this.cells.flare, position, 0.16, 5.5, 6.5);
    flash.fadeOut = 0.7;
    this.light(flash, FIRE_HOT, 1.5);
    this.additive.spawn(flash);
    const core = this.particle(this.cells.spark, position, 0.1, 3.2, 3.8);
    this.light(core, FLASH_CORE, 1.4);
    this.additive.spawn(core);
    const ring = this.particle(this.cells.ring, position, 0.3, 0.8, 8);
    this.light(ring, FLASH_CORE, 0.9, 0.5);
    this.additive.spawn(ring);
    // The fireball is drawn after the smoke, so it glows through it; gently, so that layered
    // flames stay orange against a bright sky. It cools to dark red.
    for (let i = 0; i < 7; i++) {
      const p = this.particle(this.pick(this.cells.fire), this.jitter(position, 0.4), this.rand(0.4, 0.65), this.rand(0.8, 1.1), this.rand(2.4, 3.2));
      this.scatter(p, away, this.rand(1.5, 4.5), 1.2);
      p.drag = 3.5;
      p.gravity = -1.5;
      p.spin = this.rand(-1.5, 1.5);
      p.fadeOut = 1.4;
      this.light(p, FIRE_HOT, 1.15, 0.8, FIRE_COOL, 0.5);
      this.additive.spawn(p);
    }
    for (let i = 0; i < 16; i++) {
      const p = this.particle(GLOW, position, this.rand(0.45, 0.85), 0.035);
      this.scatter(p, away, this.rand(7, 15), 1.3);
      p.gravity = 9.8;
      p.drag = 0.9;
      p.stretch = 0.04;
      this.light(p, SPARK, 1.8, 1, SPARK_END, 1);
      this.additive.spawn(p);
    }
    for (let i = 0; i < 3; i++) {
      const p = this.particle(this.cells.dirt, position, 0.7, 0.6, 1);
      this.scatter(p, away, this.rand(3, 6), 1.1);
      p.vy += 2;
      p.gravity = 9.8;
      p.drag = 0.5;
      p.fadeOut = 2;
      this.tint(p, BLAST_DEBRIS, 0.9);
      this.alpha.spawn(p);
    }
    for (let i = 0; i < 10; i++) {
      const at = this.jitter(position, 0.6).addScaledVector(away, 0.3);
      const p = this.particle(this.pick(this.cells.smoke), at, this.rand(1.8, 2.8), this.rand(1, 1.4), this.rand(3.4, 4.6));
      this.scatter(p, away, this.rand(0.8, 2.4), 1);
      p.vy += 0.9;
      p.drag = 1.4;
      p.gravity = -0.35;
      p.spin = this.rand(-0.4, 0.4);
      p.fadeIn = 0.08;
      p.fadeOut = 1.6;
      this.tint(p, BLAST_SMOKE, 0.72);
      this.alpha.spawn(p);
    }
  }

  /** A missile in flight at `position` moving along `velocity`: its rocket flame and the smoke it leaves. */
  missileTrail(position: THREE.Vector3, velocity?: THREE.Vector3): void {
    const back = velocity && velocity.lengthSq() > 1e-8 ? along.copy(velocity).normalize().negate() : null;
    const tail = spot.copy(position);
    if (back) tail.addScaledVector(back, 0.45);
    const flame = this.particle(this.pick(this.cells.muzzle), tail, 0.06, 0.3);
    if (back) {
      flame.length = this.rand(0.45, 0.6);
      flame.ax = back.x;
      flame.ay = back.y;
      flame.az = back.z;
    }
    this.light(flame, FIRE_HOT, 1.8);
    this.additive.spawn(flame);
    const smoke = this.particle(this.pick(this.cells.smoke), tail, this.rand(1.4, 2), 0.35, this.rand(1.3, 1.7));
    if (back) {
      smoke.length = TRAIL_REACH;
      smoke.ax = back.x;
      smoke.ay = back.y;
      smoke.az = back.z;
    }
    this.scatter(smoke, back ?? UP, 1, 0.4);
    smoke.vy += 0.15;
    smoke.drag = 1.8;
    smoke.gravity = -0.12;
    smoke.spin = this.rand(-0.6, 0.6);
    smoke.fadeIn = 0.04;
    smoke.fadeOut = 1.4;
    this.tint(smoke, TRAIL_SMOKE, 0.85);
    this.alpha.spawn(smoke);
  }

  update(dt: number): void {
    this.alpha.update(dt);
    this.additive.update(dt);
    this.tracers.update(dt);
  }

  measure(): EffectsMeasure {
    const pool = (batch: { count: number; capacity: number }) => ({ count: batch.count, capacity: batch.capacity });
    return {
      pools: { alpha: pool(this.alpha), additive: pool(this.additive), tracers: pool(this.tracers), decals: pool(this.decals) },
      decals: this.decals.list(),
    };
  }

  dispose(): void {
    this.group.removeFromParent();
    this.alpha.dispose();
    this.additive.dispose();
    this.tracers.dispose();
    this.decals.dispose();
  }

  private rand(min: number, max: number): number {
    return min + (max - min) * this.random();
  }

  private pick(cells: readonly UvRect[]): UvRect {
    return cells[Math.min(cells.length - 1, Math.floor(this.random() * cells.length))];
  }

  /** The unit direction from `from` to `to`, in a scratch vector; null without `from`. */
  private direction(from: THREE.Vector3 | undefined, to: THREE.Vector3): THREE.Vector3 | null {
    if (!from) return null;
    along.subVectors(to, from);
    return along.lengthSq() > 1e-8 ? along.normalize() : null;
  }

  /** The surface's outward normal, else back the way the shot came, else up; in a scratch vector. */
  private normalOf(surface: Surface | null, dir: THREE.Vector3 | null): THREE.Vector3 {
    if (surface) return facing.set(0, 0, 0).setComponent(surface.axis, surface.sign);
    return dir ? facing.copy(dir).negate() : facing.copy(UP);
  }

  /** A point within `reach` of `centre`, in a scratch vector. */
  private jitter(centre: THREE.Vector3, reach: number): THREE.Vector3 {
    return scratch.set(this.rand(-1, 1), this.rand(-1, 1), this.rand(-1, 1)).multiplyScalar(reach).add(centre);
  }

  /** The scratch spec set to a particle of `cell` at `at`, turned at random, growing from `size` to `endSize`. */
  private particle(cell: UvRect, at: THREE.Vector3, life: number, size: number, endSize = size): ParticleSpec {
    const p = resetSpec(this.spec, cell, at.x, at.y, at.z, life);
    p.size = size;
    p.endSize = endSize;
    p.rotation = this.random() * Math.PI * 2;
    return p;
  }

  /** Sends a particle off along the unit `dir` at `speed`, spread off it by up to `spread` (a share of `dir`). */
  private scatter(p: ParticleSpec, dir: THREE.Vector3, speed: number, spread: number): void {
    // A random direction: uniform on the sphere.
    const z = this.rand(-1, 1);
    const turn = this.random() * Math.PI * 2;
    const r = Math.sqrt(1 - z * z);
    const x = dir.x + r * Math.cos(turn) * spread;
    const y = dir.y + r * Math.sin(turn) * spread;
    const w = dir.z + z * spread;
    const length = Math.hypot(x, y, w) || 1;
    p.vx = (x / length) * speed;
    p.vy = (y / length) * speed;
    p.vz = (w / length) * speed;
  }

  /** Light for the additive batch: `colour` times `k`, fading to `end` times `endK`. */
  private light(p: ParticleSpec, colour: THREE.Color, k: number, alpha = 1, end = colour, endK = k): void {
    p.r = colour.r * k;
    p.g = colour.g * k;
    p.b = colour.b * k;
    p.endR = end.r * endK;
    p.endG = end.g * endK;
    p.endB = end.b * endK;
    p.alpha = alpha;
  }

  /** A colour for the alpha batch, shaded for the map's light. */
  private tint(p: ParticleSpec, colour: THREE.Color, alpha: number): void {
    p.r = p.endR = colour.r * this.shade;
    p.g = p.endG = colour.g * this.shade;
    p.b = p.endB = colour.b * this.shade;
    p.alpha = alpha;
  }

  private leave(surface: Surface, lane: number, cell: UvRect, atlas: number, size: number, tint: THREE.Color, alpha: number): void {
    const mark = this.mark;
    mark.lane = lane;
    mark.cell = cell;
    mark.atlas = atlas;
    mark.size = size;
    mark.rotation = this.random() * Math.PI * 2;
    mark.r = tint.r;
    mark.g = tint.g;
    mark.b = tint.b;
    mark.alpha = alpha;
    this.decals.add(surface, mark);
  }
}
