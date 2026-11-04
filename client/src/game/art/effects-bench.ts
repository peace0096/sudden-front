// The ?art page's effects bench (preview.ts): the game's own Effects (effects.ts) in a small map
// built by the game's buildWorld, stepped at a fixed time step and fed a seeded random source, so
// every frame comes out the same each time. Each recipe is shown on its own at the moments worth
// seeing; every recipe at once is drawn alone to count its draw calls against the budget; a flood
// far past every pool's size checks that the pools stay at their caps; and a shot at each kind of
// face (each side of the axes, the ground, a barrel that takes no mark) measures the mark it
// leaves against that face. What it measured goes into the page's report for e2e/art.mjs.
import { create } from "@bufbuild/protobuf";
import * as THREE from "three";
import { BoxKind, MapBoxSchema, MapDataSchema, MapId, WeaponKind, type MapBox, type MapData } from "../../gen/game_pb";
import { Effects, type EffectsMeasure } from "../effects";
import { buildWorld } from "../world";
import { loadedArt } from "./loader";
import { budgets, projectileArt } from "./manifest";
import { toLambert } from "./materials";
import { el, fact, type Viewer } from "./view";

const STEP = 1 / 60;
/** The server's missile speed (systems.rs MISSILE_SPEED) and the game's trail interval (game.ts). */
const MISSILE_SPEED = 60;
const TRAIL_EVERY = 0.025;

export interface EffectsBenchResult {
  /** Draw calls of the effects alone with every recipe going at once, and the budget. */
  calls: number;
  budget: number;
  /** Each pool: live with every recipe going, after the flood, and its capacity. */
  pools: Record<string, { active: number; flooded: number; capacity: number }>;
  /**
   * Each test shot: the normal of the face it should mark (null where it should leave none), and
   * the mark it left: its normal, its centre, whether that lies on such a face, and how far off
   * the face's plane it is (m, a model's lift included).
   */
  marks: { id: string; expected: number[] | null; normal: number[] | null; centre: number[] | null; onFace: boolean; error: number }[];
  /** The frames shown, by id, with each pool's live count. */
  frames: Record<string, { time: number; counts: Record<string, number> }>;
}

/** Numbers in [0, 1) from a seed (mulberry32), the same every run. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * A wall (both sides), a concrete block (its sides and top), a container along Z, a crate, a
 * barrel and a roof slab on posts (its underside), outdoors.
 */
function benchMap(): MapData {
  const box = (kind: BoxKind, minX: number, maxX: number, minY: number, maxY: number, minZ: number, maxZ: number) =>
    create(MapBoxSchema, { kind, minX, maxX, minY, maxY, minZ, maxZ });
  return create(MapDataSchema, {
    name: "EFFECTS",
    id: MapId.SUPPLY_YARD,
    halfX: 14,
    halfZ: 14,
    boxes: [
      box(BoxKind.BRICK, -8, 8, 0, 4, -6.5, -6),
      box(BoxKind.CONCRETE, 3, 5, 0, 1.2, -3, -1),
      box(BoxKind.CONTAINER_BLUE, -7, -4.6, 0, 2.6, -4, 2),
      box(BoxKind.CRATE, 0.5, 1.7, 0, 1.2, -2, -0.8),
      box(BoxKind.BARREL, -2, -1.1, 0, 1.1, -1, -0.1),
      box(BoxKind.ROOF, 6, 10, 3, 3.3, -4, 0),
      box(BoxKind.BRICK, 6, 6.4, 0, 3, -0.4, 0),
      box(BoxKind.BRICK, 9.6, 10, 0, 3, -0.4, 0),
    ],
    props: [],
  });
}

const v = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);

/** A shot at a face, or an explosion by one, and the normal of the face it should mark (null: none). */
interface MarkShot {
  id: string;
  kind: "impact" | "blood" | "explosion";
  from?: THREE.Vector3;
  at: THREE.Vector3;
  normal: number[] | null;
}

const MARK_SHOTS: MarkShot[] = [
  { id: "wall +Z", kind: "impact", from: v(1, 1.5, 4), at: v(1, 1.5, -6), normal: [0, 0, 1] },
  { id: "wall −Z", kind: "impact", from: v(0, 1, -12), at: v(0, 1, -6.5), normal: [0, 0, -1] },
  { id: "block +X", kind: "impact", from: v(9, 0.6, -2), at: v(5, 0.6, -2), normal: [1, 0, 0] },
  { id: "block −X", kind: "impact", from: v(-1, 0.6, -2.5), at: v(3, 0.6, -2.5), normal: [-1, 0, 0] },
  { id: "block +Y", kind: "impact", from: v(4, 3, -2), at: v(4, 1.2, -2), normal: [0, 1, 0] },
  { id: "roof −Y", kind: "impact", from: v(8, 1.5, -2), at: v(8, 3, -2), normal: [0, -1, 0] },
  { id: "container +X", kind: "impact", from: v(-1, 1.3, 1), at: v(-4.6, 1.3, 1), normal: [1, 0, 0] },
  { id: "crate +Z", kind: "impact", from: v(1.1, 0.6, 3), at: v(1.1, 0.6, -0.8), normal: [0, 0, 1] },
  { id: "barrel", kind: "impact", from: v(-1.55, 0.6, 3), at: v(-1.55, 0.6, -0.1), normal: null },
  { id: "blood → wall", kind: "blood", from: v(-0.5, 1.45, 4), at: v(-0.5, 1.3, -5.1), normal: [0, 0, 1] },
  { id: "blast → ground", kind: "explosion", at: v(2, 0.05, 2.5), normal: [0, 1, 0] },
  { id: "blast → wall", kind: "explosion", at: v(-3, 1.6, -5.95), normal: [0, 0, 1] },
];

function fire(effects: Effects, shot: MarkShot): void {
  if (shot.kind === "impact") effects.impact(shot.at, shot.from);
  else if (shot.kind === "blood") effects.bloodPuff(shot.at, shot.from);
  else effects.explosion(shot.at);
}

/** Whether a mark's centre lies on a face with its normal (within `lift` off it), and how far off the plane. */
function onFace(boxes: MapBox[], centre: number[], normal: number[]): { onFace: boolean; error: number } {
  const axis = normal.findIndex((value) => Math.abs(value) > 0.999);
  if (axis < 0) return { onFace: false, error: Infinity };
  const sign = Math.sign(normal[axis]);
  const lift = 0.025;
  let best = axis === 1 && sign > 0 ? Math.abs(centre[1]) : Infinity;
  const low = (b: MapBox) => [b.minX, b.minY, b.minZ];
  const high = (b: MapBox) => [b.maxX, b.maxY, b.maxZ];
  for (const b of boxes) {
    const plane = sign > 0 ? high(b)[axis] : low(b)[axis];
    const inside = [0, 1, 2].every((a) => a === axis || (centre[a] >= low(b)[a] - 1e-3 && centre[a] <= high(b)[a] + 1e-3));
    if (inside) best = Math.min(best, Math.abs(centre[axis] - plane));
  }
  return { onFace: best <= lift, error: best };
}

function step(effects: Effects, seconds: number, each?: (time: number) => void): void {
  const steps = Math.round(seconds / STEP);
  for (let i = 0; i < steps; i++) {
    each?.(i * STEP);
    effects.update(STEP);
  }
}

/** A frame: what happens, from when, and where the camera stands. */
interface Frame {
  id: string;
  label: string;
  /** Seconds after the start that the frame is taken. */
  time: number;
  views: { from: THREE.Vector3; at: THREE.Vector3; fov?: number }[];
  /** Called every step (with the time) until the frame is taken. */
  play: (effects: Effects, time: number) => void;
  /** Anything else to stand in the scene. */
  props?: () => THREE.Object3D[];
}

/** Shots of an automatic weapon from `shooter`, every `every` s, at points spread over a target area. */
function burst(effects: Effects, time: number, shooter: THREE.Vector3, every: number, phase: number, target: (n: number) => THREE.Vector3): void {
  const shot = Math.round((time - phase) / STEP);
  const interval = Math.round(every / STEP);
  if (time < phase || shot % interval !== 0) return;
  const end = target(shot / interval);
  const direction = end.clone().sub(shooter);
  effects.muzzleFlash(shooter, direction);
  effects.tracer(shooter, end);
  effects.impact(end, shooter);
}

const wallPoint = (n: number, x: number, spread: number) => v(x + Math.sin(n * 2.4) * spread, 1.5 + Math.cos(n * 1.7) * 0.6, -6);

function frames(): Frame[] {
  const muzzle = v(-1, 1.45, 3.5);
  const aim = v(0.25, -0.02, -1).normalize();
  const shotFrom = v(0.5, 1.6, 3);
  const brickHits = [v(0.2, 1.65, -6), v(0.65, 1.35, -6), v(0.95, 1.8, -6)];
  const metalFrom = v(-1, 1.4, 1.5);
  const metalHits = [v(-4.6, 1.5, 0.6), v(-4.6, 1.2, 1), v(-4.6, 1.7, 1.3)];
  const hitPlayer = v(-0.5, 1.3, -5.1);
  const hitFrom = v(-0.5, 1.45, 4);
  const blast = v(2, 0.05, 2.5);
  const wallBlast = v(-3, 1.6, -5.95);
  const missileFrom = v(-12, 1.8, 3.2);
  const missileHeading = v(1, 0.05, -0.25).normalize();
  const salvo = (effects: Effects, time: number, from: THREE.Vector3, hits: THREE.Vector3[]) => {
    hits.forEach((hit, i) => {
      if (Math.round(time / STEP) === i) effects.impact(hit, from);
    });
  };
  const blastAt = (effects: Effects, time: number) => {
    if (time === 0) effects.explosion(blast);
  };
  const blastView = [{ from: v(2.6, 2.1, 8.6), at: v(2, 1.3, 2.5) }];
  return [
    {
      id: "muzzle",
      label: "총구 화염 (원격 사수, 1프레임 뒤) — 옆 · 앞",
      time: STEP,
      views: [
        { from: muzzle.clone().add(v(2.2, 0.15, -0.4)), at: muzzle.clone().addScaledVector(aim, 0.3), fov: 40 },
        { from: muzzle.clone().addScaledVector(aim, 2.6).add(v(0.9, 0.25, 0)), at: muzzle, fov: 40 },
      ],
      play: (effects, time) => {
        if (time === 0) effects.muzzleFlash(muzzle, aim);
      },
    },
    {
      id: "impact",
      label: "벽돌 벽 탄착: 불꽃 · 섬광 · 먼지 · 탄흔 (0.04초)",
      time: 0.04,
      views: [{ from: v(2, 1.9, -3.6), at: v(0.6, 1.55, -6) }],
      play: (effects, time) => salvo(effects, time, shotFrom, brickHits),
    },
    {
      id: "impactLater",
      label: "같은 탄착, 0.4초 뒤: 먼지가 퍼지고 탄흔이 남음",
      time: 0.4,
      views: [{ from: v(2, 1.9, -3.6), at: v(0.6, 1.55, -6) }],
      play: (effects, time) => salvo(effects, time, shotFrom, brickHits),
    },
    {
      id: "impactMetal",
      label: "컨테이너(금속) 탄착: 불꽃이 더 많음 (0.04초)",
      time: 0.04,
      views: [{ from: v(-2.4, 1.8, 3.4), at: v(-4.6, 1.4, 0.9) }],
      play: (effects, time) => salvo(effects, time, metalFrom, metalHits),
    },
    {
      id: "ground",
      label: "땅과 나무 상자 탄착: 흙먼지 · 나뭇조각, 불꽃 없음 (0.15초)",
      time: 0.15,
      views: [{ from: v(3.6, 1.7, 2.6), at: v(1.4, 0.4, -0.6) }],
      play: (effects, time) => {
        if (time !== 0) return;
        effects.impact(v(2.2, 0, 0.6), v(2.6, 1.6, 4));
        effects.impact(v(2.6, 0, 1.0), v(2.6, 1.6, 4));
        effects.impact(v(1.1, 0.7, -0.8), v(1.4, 1.6, 4));
        effects.impact(v(0.8, 0.45, -0.8), v(1.4, 1.6, 4));
      },
    },
    {
      id: "blood",
      label: "피격: 피 안개 · 핏방울, 뒤 0.9 m 벽에 핏자국 (0.05초)",
      time: 0.05,
      views: [{ from: v(1.4, 1.6, -2.8), at: v(-0.5, 1.25, -5.6) }],
      play: (effects, time) => {
        if (time === 0) effects.bloodPuff(hitPlayer, hitFrom);
      },
    },
    {
      id: "bloodLater",
      label: "같은 피격, 0.4초 뒤",
      time: 0.4,
      views: [{ from: v(1.4, 1.6, -2.8), at: v(-0.5, 1.25, -5.6) }],
      play: (effects, time) => {
        if (time === 0) effects.bloodPuff(hitPlayer, hitFrom);
      },
    },
    { id: "blast1", label: "폭발 0.03초: 섬광", time: 0.03, views: blastView, play: blastAt },
    { id: "blast2", label: "폭발 0.12초: 불덩이 · 충격파 · 불꽃", time: 0.12, views: blastView, play: blastAt },
    { id: "blast3", label: "폭발 0.35초: 불덩이가 식고 연기", time: 0.35, views: blastView, play: blastAt },
    { id: "blast4", label: "폭발 0.9초: 검은 연기", time: 0.9, views: blastView, play: blastAt },
    { id: "blast5", label: "폭발 2초: 연기가 걷히고 그을음", time: 2, views: blastView, play: blastAt },
    {
      id: "wallBlast",
      label: "벽 앞 폭발 0.25초 (벽에서 튀어나옴)",
      time: 0.25,
      views: [{ from: v(-0.6, 2, -0.8), at: v(-3, 1.6, -6) }],
      play: (effects, time) => {
        if (time === 0) effects.explosion(wallBlast);
      },
    },
    {
      id: "wallBlastLater",
      label: "같은 폭발 3초 뒤: 벽의 그을음",
      time: 3,
      views: [{ from: v(-0.6, 2, -0.8), at: v(-3, 1.6, -6) }],
      play: (effects, time) => {
        if (time === 0) effects.explosion(wallBlast);
      },
    },
    {
      id: "tracers",
      label: "예광탄 연사: 사수 4명이 0.05~0.08초마다, 옆에서 (0.5초)",
      time: 0.5,
      views: [{ from: v(7.5, 1.9, 4.5), at: v(-1, 1.5, 1.5), fov: 70 }],
      play: (effects, time) => {
        burst(effects, time, v(-3, 1.5, 10), 0.067, 0, (n) => wallPoint(n, -3, 1));
        burst(effects, time, v(0, 1.4, 11), 0.05, 0.017, (n) => wallPoint(n, 0, 1.5));
        burst(effects, time, v(2, 1.6, 9.5), 0.083, 0.033, (n) => wallPoint(n, 2, 1));
        burst(effects, time, v(-1.5, 1.5, 12.5), 0.05, 0.008, (n) => wallPoint(n, -1, 2));
      },
    },
    {
      id: "missile",
      label: "미사일 연기 (0.3초 비행)",
      time: 0.3,
      views: [{ from: v(-2.5, 2.1, 8.5), at: v(-1.5, 2.1, 1.5), fov: 72 }],
      play: (effects, time) => {
        // As game.ts lays them: one every TRAIL_EVERY of flight, where the missile was then.
        for (let due = Math.ceil(time / TRAIL_EVERY) * TRAIL_EVERY; due < time + STEP - 1e-9; due += TRAIL_EVERY) {
          effects.missileTrail(missileFrom.clone().addScaledVector(missileHeading, MISSILE_SPEED * due), missileHeading.clone().multiplyScalar(MISSILE_SPEED));
        }
      },
      props: () => {
        const art = projectileArt(WeaponKind.MISSILE);
        const gltf = art && loadedArt()?.models.get(art.file);
        if (!art || !gltf) return [];
        const model = gltf.scene.clone();
        toLambert(model);
        model.position.fromArray(art.placement?.position ?? [0, 0, 0]);
        model.rotation.fromArray([...(art.placement?.rotation ?? [0, 0, 0]), "XYZ"] as [number, number, number, THREE.EulerOrder]);
        model.scale.setScalar(art.placement?.scale ?? 1);
        const holder = new THREE.Group();
        holder.add(model);
        holder.position.copy(missileFrom).addScaledVector(missileHeading, MISSILE_SPEED * 0.3);
        holder.lookAt(holder.position.clone().add(missileHeading));
        return [holder];
      },
    },
    {
      id: "marks",
      label: "자국 시험 사격 전체, 3초 뒤: 면마다 탄흔, 벽의 핏자국, 땅과 벽의 그을음 (드럼통에는 없음)",
      time: 3,
      views: [
        { from: v(7, 3.4, 5), at: v(0.5, 0.9, -2.5) },
        { from: v(2.2, 1.5, -9.2), at: v(0, 1, -6.5), fov: 45 },
      ],
      play: (effects, time) => {
        if (time === 0) for (const shot of MARK_SHOTS) fire(effects, shot);
      },
    },
  ];
}

function counts(measure: EffectsMeasure): Record<string, number> {
  return Object.fromEntries(Object.entries(measure.pools).map(([name, pool]) => [name, pool.count]));
}

export function effectsBench(viewer: Viewer): { cards: HTMLElement[]; result: EffectsBenchResult } {
  const map = benchMap();
  const stage = new THREE.Scene();
  const world = buildWorld(stage, map);
  world.updateMatrixWorld(true);
  const fog = stage.fog;
  const cards: HTMLElement[] = [];
  const result: EffectsBenchResult = { calls: 0, budget: budgets.drawCalls.effects, pools: {}, marks: [], frames: {} };

  const width = 480;
  const height = 300;
  frames().forEach((frame, index) => {
    const root = el("article", "art-card art-card-effect");
    root.dataset.frame = frame.id;
    const canvas = el("canvas");
    root.append(el("h3", "", frame.label), canvas);
    const scene = new THREE.Scene();
    scene.fog = fog;
    // One world for every frame: each takes it in as it is drawn.
    scene.onBeforeRender = () => {
      if (world.parent !== scene) scene.add(world);
    };
    const effects = new Effects(scene, map.boxes, { random: seeded(index + 1) });
    for (const prop of frame.props?.() ?? []) scene.add(prop);
    step(effects, frame.time, (time) => frame.play(effects, time));
    const measured = counts(effects.measure());
    result.frames[frame.id] = { time: frame.time, counts: measured };
    const viewWidth = width / frame.views.length;
    viewer.addCell({
      canvas,
      width,
      height,
      scene,
      views: frame.views.map((view, i) => {
        const camera = new THREE.PerspectiveCamera(view.fov ?? 50, viewWidth / height, 0.05, 1000);
        camera.position.copy(view.from);
        camera.lookAt(view.at);
        return { camera, x: i * viewWidth, width: viewWidth };
      }),
    });
    const facts = el("dl", "art-facts");
    fact(facts, "살아 있는 수", `연기·먼지 ${measured.alpha} · 빛 ${measured.additive} · 예광탄 ${measured.tracers} · 자국 ${measured.decals}`);
    root.append(facts);
    cards.push(root);
  });

  // Every recipe at once, drawn alone: one call for each pool with something in it.
  const together = new THREE.Scene();
  const all = new Effects(together, map.boxes, { random: seeded(99) });
  all.muzzleFlash(v(-1, 1.45, 3.5), v(0.25, 0, -1));
  all.tracer(v(-1, 1.45, 3.5), v(1, 1.5, -6));
  all.impact(v(1, 1.5, -6), v(-1, 1.45, 3.5));
  all.bloodPuff(v(-0.5, 1.3, -5.1), v(-0.5, 1.45, 4));
  all.explosion(v(2, 0.05, 2.5));
  all.missileTrail(v(-3, 1.8, 4), v(40, 0, -40));
  all.update(STEP);
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.05, 1000);
  camera.position.set(0, 3, 10);
  camera.lookAt(0, 1, 0);
  result.calls = viewer.drawCalls(together, camera);
  const active = all.measure().pools;

  // A flood far past every pool's size.
  const flood = new Effects(new THREE.Scene(), map.boxes, { random: seeded(7) });
  const random = seeded(8);
  const along = (min: number, max: number) => min + (max - min) * random();
  for (let i = 0; i < 400; i++) {
    const hit = v(along(-7.5, 7.5), along(0.3, 3.7), -6);
    const from = v(along(-6, 6), 1.5, along(3, 10));
    flood.impact(hit, from);
    flood.tracer(from, hit);
    flood.muzzleFlash(from, hit.clone().sub(from));
  }
  for (let i = 0; i < 60; i++) flood.explosion(v(along(-6, 6), 0.05, along(0, 8)));
  for (let i = 0; i < 100; i++) flood.bloodPuff(v(along(-6, 6), 1.3, -5.2), v(along(-6, 6), 1.5, 6));
  for (let i = 0; i < 100; i++) flood.missileTrail(v(along(-6, 6), 2, along(0, 8)), v(0, 0, -60));
  flood.update(STEP);
  const flooded = flood.measure().pools;
  for (const [name, pool] of Object.entries(active)) {
    result.pools[name] = { active: pool.count, flooded: flooded[name as keyof typeof flooded].count, capacity: pool.capacity };
  }

  // The marks each test shot leaves, one Effects apiece so each mark is its own.
  for (const shot of MARK_SHOTS) {
    const effects = new Effects(new THREE.Scene(), map.boxes, { random: seeded(3) });
    fire(effects, shot);
    const mark = effects.measure().decals[0] ?? null;
    const fit = mark ? onFace(map.boxes, mark.centre, mark.normal) : { onFace: false, error: Infinity };
    result.marks.push({ id: shot.id, expected: shot.normal, normal: mark?.normal ?? null, centre: mark?.centre ?? null, ...fit });
  }

  const summary = el("article", "art-card art-card-effect");
  summary.append(el("h3", "", "이펙트 검사"));
  const facts = el("dl", "art-facts");
  fact(facts, "draw call (모든 이펙트 동시)", `${result.calls} / 예산 ${result.budget}`, result.calls <= result.budget ? "ok" : "over");
  for (const [name, pool] of Object.entries(result.pools)) {
    fact(facts, `풀 ${name}`, `동시 ${pool.active} · 넘치게 넣은 뒤 ${pool.flooded} / ${pool.capacity}`, pool.flooded <= pool.capacity ? "ok" : "over");
  }
  for (const mark of result.marks) {
    const shown = mark.normal ? `법선 ${mark.normal.map((value) => value.toFixed(2)).join(", ")} · 면에서 ${(mark.error * 100).toFixed(2)} cm` : "자국 없음";
    const right = mark.expected === null ? mark.normal === null : !!mark.normal && mark.onFace && mark.expected.every((value, i) => Math.abs(value - mark.normal![i]) < 1e-3);
    fact(facts, mark.id, shown, right ? "ok" : "over");
  }
  summary.append(facts);
  cards.unshift(summary);
  return { cards, result };
}
