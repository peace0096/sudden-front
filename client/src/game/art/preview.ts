// The ?art page: loads every file the art manifest names through the game's loader, without a
// server, converts it to Lambert as the game will, and shows it fitted the way the game will fit
// it: the soldiers in the server's hitboxes with a clip picker, every weapon at its length with its
// grip and muzzle marked, the map pieces in the boxes the server collides with (containers in
// their four colours) and the effect atlases cell by cell. A bench then puts real Characters (the
// game's remote soldier) through standing, crouching, aiming, head sizes, death, every weapon and
// Big Head mode's crowned King, and measures them against the hitboxes; a second bench puts the game's first-person view model
// through every weapon's idle, kick, reload, bolt or pump cycle, knife and hammer swing and grenade throw, and measures its
// hands on the gun, its muzzles on screen and its draw calls; a map bench builds a small map that
// holds every BoxKind and PropKind with the game's buildWorld and measures each kit copy against
// the box it fills, and the map's draw calls; and an effects bench (effects-bench.ts) runs the
// game's effects at a fixed time step and checks their draw calls, pools and marks. It lists what
// failed to load and the budgets, and publishes what it measured as window.__artReport for
// e2e/art.mjs.
// With ?art&candidates (dev server only) it adds the checkpoint A candidate gallery (gallery.ts).
import { create } from "@bufbuild/protobuf";
import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import { BoxKind, MapBoxSchema, MapDataSchema, MapId, MapPropSchema, PropKind, Team, WeaponKind, type MapData } from "../../gen/game_pb";
import { Character, type CharacterMeasure } from "../character";
import { ViewModel, type ViewModelMeasure } from "../viewmodel";
import { buildWorld } from "../world";
import { effectsBench, type EffectsBenchResult } from "./effects-bench";
import { loadArt, type ArtSet } from "./loader";
import { budgets, generatedInfo, kitArt, manifest, manifestFiles, propArt, weaponArt, type AtlasArt, type KitArt } from "./manifest";
import { toLambert } from "./materials";
import { Viewer, el, fact, ground, hitboxFrame, marker, metres, number, stageScene, wireBox } from "./view";
import "./preview.css";

/** What the page measured, for e2e/art.mjs. */
export interface ArtReport {
  failures: { file: string; reason: string }[];
  /** Every file the manifest names, against its budget; a file in no budget group has no limits. */
  files: { file: string; bytes: number | null; triangles: number | null; limitKB: number | null; limitTriangles: number | null; ok: boolean }[];
  totalBytes: number;
  downloadLimitBytes: number;
  weapons: Record<string, { file: string | null; loaded: boolean; muzzle: boolean; triangles: number }>;
  characters: Record<string, { file: string; loaded: boolean; bones: string[]; clips: string[]; headJoint: number | null; triangles: number }>;
  kits: Record<string, { file: string; size: number[] | null; measured: number[] | null }>;
  atlases: { file: string; loaded: boolean; width: number; height: number; cells: number; inside: boolean }[];
  /** The character bench: each state's measurements and draw calls. */
  bench: Record<string, BenchResult>;
  /** The first-person bench, by state. */
  firstPerson: Record<string, FirstPersonResult>;
  /** The map bench; null when the manifest has no map kits. */
  map: MapBenchResult | null;
  /** The effects bench (effects-bench.ts); null when the manifest has no effect atlas. */
  effects: EffectsBenchResult | null;
}

/** A state the bench puts a real Character in. */
interface BenchState {
  id: string;
  label: string;
  team?: Team;
  weapon?: WeaponKind;
  pitch?: number;
  crouch?: boolean;
  headScale?: number;
  alive?: boolean;
  shielded?: boolean;
  stunned?: boolean;
  /** Big Head mode's reigning King, crowned. */
  king?: boolean;
  /** Ground speed in m/s and heading in radians from the facing, right positive. */
  moving?: { speed: number; heading: number };
}

export interface BenchResult extends CharacterMeasure {
  label: string;
  weapon: string;
  pitch: number;
  crouch: boolean;
  headScale: number;
  alive: boolean;
  shielded: boolean;
  stunned: boolean;
  king: boolean;
  moving: boolean;
  /** Draw calls of the character alone. */
  calls: number;
}

function panel(title: string, note = ""): HTMLElement {
  const section = el("section", "panel");
  const head = el("div", "panel-head");
  head.append(el("h2", "", title));
  if (note) head.append(el("span", "muted", note));
  section.append(head);
  return section;
}

function rows(entries: [string, string][]): HTMLDListElement {
  const list = el("dl", "art-rows");
  for (const [term, value] of entries) list.append(el("dt", "", term), el("dd", "", value));
  return list;
}

const count = (record: object) => Object.keys(record).length;
const kb = (bytes: number) => `${(bytes / 1024).toFixed(1)} KB`;

function trianglesIn(root: THREE.Object3D): number {
  let total = 0;
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    const geometry = object.geometry as THREE.BufferGeometry;
    total += (geometry.index ? geometry.index.count : geometry.getAttribute("position").count) / 3;
  });
  return total;
}

function card(title: string, kind: string): { root: HTMLElement; facts: HTMLDListElement; canvas: HTMLCanvasElement } {
  const root = el("article", `art-card art-card-${kind}`);
  const canvas = el("canvas");
  const facts = el("dl", "art-facts");
  root.append(el("h3", "", title), canvas);
  return { root, facts, canvas };
}

// ------------------------------------------------------------------ characters

function characterCard(viewer: Viewer, team: string, file: string, gltf: GLTF, picker: HTMLSelectElement, report: ArtReport): HTMLElement {
  const { root, facts, canvas } = card(`${team.toUpperCase()} — ${file}`, "character");
  const model = cloneSkinned(gltf.scene);
  toLambert(model);
  // The rest pose, before any clip moves it: where the game fits the head to the hitbox.
  model.updateMatrixWorld(true);
  const head = model.getObjectByName("head");
  const headJoint = head ? head.getWorldPosition(new THREE.Vector3()).y : null;
  const bones: string[] = [];
  model.traverse((object) => object instanceof THREE.Bone && bones.push(object.name));
  const holder = new THREE.Group();
  holder.rotation.y = manifest.character?.forwardYaw ?? 0;
  holder.add(model);

  const scene = stageScene();
  scene.add(ground(3, 6), holder, hitboxFrame().group);
  const viewWidth = 180;
  const height = 320;
  const ortho = (x: number, z: number) => {
    const half = 1.06;
    const aspect = viewWidth / height;
    const camera = new THREE.OrthographicCamera(-half * aspect, half * aspect, half, -half, 0.1, 20);
    camera.position.set(x, 0.98, z);
    camera.lookAt(0, 0.98, 0);
    return camera;
  };
  // Players face −Z, so the front view looks from −Z.
  const threeQuarter = new THREE.PerspectiveCamera(28, viewWidth / height, 0.1, 30);
  threeQuarter.position.set(2.7, 1.9, -3.6);
  threeQuarter.lookAt(0, 0.92, 0);
  const mixer = new THREE.AnimationMixer(model);
  let playing: THREE.AnimationAction | null = null;
  viewer.addCell({
    canvas,
    width: viewWidth * 3,
    height,
    scene,
    views: [
      { camera: ortho(0, -6), x: 0, width: viewWidth },
      { camera: ortho(6, 0), x: viewWidth, width: viewWidth },
      { camera: threeQuarter, x: viewWidth * 2, width: viewWidth },
    ],
    tick: (dt) => mixer.update(dt),
    configure: () => {
      const clip = gltf.animations.find((item) => item.name === picker.value);
      const action = clip ? mixer.clipAction(clip) : null;
      if (action === playing) return;
      playing?.stop();
      action?.reset().play();
      playing = action;
    },
  });
  root.append(el("div", "art-legend", "정면 · 측면 · 3/4 — 빨강 머리 · 주황 몸통 · 파랑 다리: 서버 히트박스"));
  const triangles = trianglesIn(model);
  fact(facts, "삼각형", `${number(triangles)} / 예산 ${number(budgets.triangles.character)}`, triangles <= budgets.triangles.character ? "ok" : "over");
  fact(facts, "머리 뼈 (rest pose)", headJoint === null ? "없음" : `${headJoint.toFixed(3)} m (히트박스 목 1.50 m)`, headJoint !== null && Math.abs(headJoint - 1.5) <= 0.03 ? "ok" : "over");
  fact(facts, "뼈", `${bones.length}개`);
  fact(facts, "클립", gltf.animations.map((item) => `${item.name} ${item.duration.toFixed(2)}s`).join(" · "));
  root.append(facts);
  report.characters[team] = { file, loaded: true, bones, clips: gltf.animations.map((item) => item.name), headJoint, triangles };
  return root;
}


// ------------------------------------------------------------------ the character bench

/**
 * Degrees the knife hand's wrist may bend from straight, in both views (e2e/art.mjs holds it there):
 * the knife's tip points where the arm puts it, never where a bent wrist does.
 */
const KNIFE_WRIST_LIMIT = 20;

/** Each wrist's bend from straight, in degrees, and whether the knife hand keeps within its limit. */
function wristFact(facts: HTMLDListElement, wrist: { right: number | null; left: number | null }, weapon: WeaponKind | undefined): void {
  const degrees = (radians: number | null) => (radians === null ? "—" : `${THREE.MathUtils.radToDeg(radians).toFixed(1)}°`);
  const straight = wrist.right !== null && THREE.MathUtils.radToDeg(wrist.right) <= KNIFE_WRIST_LIMIT;
  fact(facts, "손목 꺾임 (오른 · 왼)", `${degrees(wrist.right)} · ${degrees(wrist.left)}`, weapon === WeaponKind.KNIFE ? (straight ? "ok" : "over") : undefined);
}

/** The rifle's ground speed (server weapons.rs: 5.2 × SPEED_SCALE 0.9) and half of it crouched. */
const RUN_SPEED = 4.68;
const CROUCH_SPEED = RUN_SPEED / 2;
const BENCH_DT = 1 / 30;

function benchStates(): BenchState[] {
  const states: BenchState[] = [
    { id: "stand", label: "서기" },
    { id: "crouch", label: "앉기", crouch: true },
    { id: "aimUp", label: "위로 조준 (pitch +1.45)", pitch: 1.45 },
    { id: "aimDown", label: "아래로 조준 (pitch −1.45)", pitch: -1.45 },
    { id: "crouchAimUp", label: "앉아서 위로 조준", crouch: true, pitch: 1.45 },
    { id: "crouchAimDown", label: "앉아서 아래로 조준", crouch: true, pitch: -1.45 },
    { id: "head0.5", label: "작은 머리 (0.5배)", headScale: 0.5 },
    { id: "head1", label: "머리 1배", headScale: 1 },
    { id: "head2", label: "대두 (2배)", headScale: 2 },
    { id: "dead", label: "사망", alive: false },
    { id: "shield", label: "방패병 (권총 + 방패)", weapon: WeaponKind.PISTOL, shielded: true },
    { id: "stunned", label: "마비 (테이저)", stunned: true },
    // The server's King head scale (big_head.rs HEAD_SCALES at level 3).
    { id: "king", label: "무적의 왕대두 (뿅망치 + 왕관)", weapon: WeaponKind.HAMMER, headScale: 2.6, king: true },
    { id: "blue", label: "BLUE 팀", team: Team.BLUE },
    { id: "neutral", label: "개인전 (중립 군복)", team: Team.UNSPECIFIED },
    { id: "run", label: "달리기", moving: { speed: RUN_SPEED, heading: 0 } },
    { id: "strafe", label: "오른쪽으로 달리기", moving: { speed: RUN_SPEED, heading: Math.PI / 2 } },
    { id: "diagonal", label: "대각선 앞오른쪽", moving: { speed: RUN_SPEED, heading: Math.PI / 4 } },
    { id: "runBack", label: "뒤로 달리기", moving: { speed: RUN_SPEED, heading: Math.PI } },
    { id: "crouchWalk", label: "앉아서 걷기", crouch: true, moving: { speed: CROUCH_SPEED, heading: 0 } },
    { id: "crouchStrafe", label: "앉아서 옆으로", crouch: true, moving: { speed: CROUCH_SPEED, heading: -Math.PI / 2 } },
  ];
  const kinds = Object.values(WeaponKind).filter((kind): kind is WeaponKind => typeof kind === "number" && kind !== WeaponKind.UNSPECIFIED);
  for (const kind of kinds) states.push({ id: `hold:${WeaponKind[kind]}`, label: `${WeaponKind[kind]} (${weaponArt(kind)?.hold ?? "-"})`, weapon: kind });
  return states;
}

/** Feeds a character `seconds` of frames in one state; returns where its position got to. */
function drive(character: Character, state: BenchState, seconds: number, from: { x: number; z: number; time: number }): void {
  const steps = Math.round(seconds / BENCH_DT);
  for (let i = 0; i < steps; i++) {
    from.time += BENCH_DT;
    if (state.moving) {
      from.x += Math.sin(state.moving.heading) * state.moving.speed * BENCH_DT;
      from.z -= Math.cos(state.moving.heading) * state.moving.speed * BENCH_DT;
    }
    character.update(BENCH_DT, from.x, from.z, 0, state.pitch ?? 0, !!state.moving, state.alive ?? true, false, from.time, !!state.crouch);
  }
  // Shown and measured where it stands; only the stride needed the travel.
  character.root.position.set(0, 0, 0);
}

function benchCard(viewer: Viewer, state: BenchState, report: ArtReport): HTMLElement {
  const root = el("article", "art-card art-card-bench");
  root.dataset.state = state.id;
  const canvas = el("canvas");
  root.append(el("h3", "", state.label), canvas);
  const character = new Character(state.label, state.team ?? Team.RED);
  if (state.weapon) character.setWeapon(state.weapon);
  character.setLook(state.headScale ?? 1, !!state.shielded, !!state.stunned, !!state.king);
  character.setNameVisible(true);
  const clock = { x: 0, z: 0, time: 0 };
  // Long enough for a crouch to settle and a death to finish, short of the body disappearing.
  drive(character, state, state.alive === false ? 1.6 : 1.2, clock);
  const measured = character.measure();

  // Its draw calls alone, framed whole so nothing is culled.
  const alone = new THREE.Scene();
  alone.add(character.root);
  const framing = new THREE.PerspectiveCamera(40, 1, 0.1, 30);
  framing.position.set(2.6, 1.6, -3.6);
  framing.lookAt(0, 0.9, 0);
  const calls = viewer.drawCalls(alone, framing);

  const scene = stageScene();
  const frame = hitboxFrame();
  frame.pose(!!state.crouch, state.headScale ?? 1, state.shielded ? 0 : null);
  frame.group.visible = state.alive !== false;
  scene.add(ground(3, 6), frame.group, character.root);
  const width = 200;
  const height = 300;
  // The King's crown and the name over it stand higher than any other state's head: both views
  // frame that much more, upward.
  const lift = state.king ? 0.35 : 0;
  const half = 1.15 + lift;
  const side = new THREE.OrthographicCamera((-half * width) / height, (half * width) / height, half, -half, 0.1, 20);
  side.position.set(6, 1 + lift, 0);
  side.lookAt(0, 1 + lift, 0);
  const threeQuarter = new THREE.PerspectiveCamera(30, width / height, 0.1, 30);
  threeQuarter.position.set(-2.4 * (1 + lift), 1.7 + lift, -3.6 * (1 + lift));
  threeQuarter.lookAt(0, 0.95 + lift, 0);
  const views = [
    { camera: threeQuarter, x: 0, width },
    { camera: side, x: width, width },
  ];
  // A weapon's states add two close-ups of the hands on it, from its right and from below on its
  // left, aimed a little ahead of the grip so the support hand is in view too.
  if (state.weapon !== undefined) {
    const hands = new THREE.Vector3().fromArray(measured.grip).add(new THREE.Vector3(0, 0, -0.1));
    for (const offset of [new THREE.Vector3(0.62, 0.22, -0.5), new THREE.Vector3(-0.55, -0.28, -0.55)]) {
      const closeUp = new THREE.PerspectiveCamera(32, width / height, 0.05, 10);
      closeUp.position.copy(hands).add(offset);
      closeUp.lookAt(hands);
      views.push({ camera: closeUp, x: width * views.length, width });
    }
  }
  viewer.addCell({
    canvas,
    width: width * views.length,
    height,
    scene,
    views,
    // Moving states keep moving; the rest hold the pose they were measured in.
    tick: state.moving ? () => drive(character, state, BENCH_DT, clock) : undefined,
  });

  const facts = el("dl", "art-facts");
  const neck = measured.neck[1];
  const neckGoal = state.crouch ? 1.0 : 1.5;
  fact(facts, "목 (머리 뼈)", `${neck.toFixed(3)} m, 기둥에서 ${measured.neckOffset.toFixed(3)} m`, state.alive === false || state.moving ? undefined : Math.abs(neck - neckGoal) <= 0.03 && measured.neckOffset <= 0.15 ? "ok" : "over");
  const scale = state.headScale ?? 1;
  const top = 1.5 + 0.26 * scale - (state.crouch ? 0.5 : 0);
  fact(facts, "머리 꼭대기", `${measured.headTop.toFixed(3)} m (히트박스 ${top.toFixed(3)})`, state.alive === false ? undefined : Math.abs(measured.headTop - top) <= 0.05 * scale ? "ok" : "over");
  fact(facts, "발바닥 (왼 · 오른)", `${measured.feet.map((y) => y.toFixed(3)).join(" · ")} m`, state.alive === false || state.moving ? undefined : measured.feet.every((y) => y >= -0.03 && y <= 0.05) ? "ok" : "over");
  fact(facts, "총구", `앞으로 ${measured.muzzleAhead.toFixed(2)} m · 조준과 ${THREE.MathUtils.radToDeg(measured.gunToAim).toFixed(1)}°${measured.support === null ? "" : ` · 왼손 ${(measured.support * 100).toFixed(1)} cm, 손바닥 ${measured.supportFacing?.toFixed(2)}`}`);
  wristFact(facts, measured.wrist, state.weapon);
  if (state.king) {
    // On the head (its bottom from 15 cm into it to 5 cm over it), and under the name.
    const crown = measured.crown;
    const seated = !!crown && crown.bottom >= measured.headTop - 0.15 && crown.bottom <= measured.headTop + 0.05 && measured.name > crown.top;
    fact(facts, "왕관 · 이름표", crown ? `${crown.bottom.toFixed(3)}~${crown.top.toFixed(3)} m · ${measured.name.toFixed(3)} m` : "없음", seated ? "ok" : "over");
  }
  fact(facts, "draw call", `${calls}`);
  root.append(facts);
  report.bench[state.id] = {
    ...measured,
    label: state.label,
    weapon: WeaponKind[state.weapon ?? WeaponKind.RIFLE],
    pitch: state.pitch ?? 0,
    crouch: !!state.crouch,
    headScale: scale,
    alive: state.alive !== false,
    shielded: !!state.shielded,
    stunned: !!state.stunned,
    king: !!state.king,
    moving: !!state.moving,
    calls,
  };
  return root;
}

// ------------------------------------------------------------------ the first-person bench

/** A moment the first-person bench holds a real ViewModel in. */
type FirstPersonPose = "idle" | "kick" | "reload" | "cycle" | "swing" | "throw";

interface FirstPersonState {
  id: string;
  label: string;
  weapon: WeaponKind;
  team: Team;
  pose: FirstPersonPose;
  shield: boolean;
}

export interface FirstPersonResult extends ViewModelMeasure {
  label: string;
  weapon: string;
  team: string;
  pose: FirstPersonPose;
  shield: boolean;
  /** Whether the left hand belongs on the gun: at a support point, or holding a second gun. */
  twoHanded: boolean;
  /** Draw calls of the view model alone. */
  calls: number;
}

const POSE_LABELS: Record<FirstPersonPose, string> = { idle: "대기", kick: "발사 반동", reload: "장전 중간", cycle: "노리쇠·펌프 중간", swing: "휘두르기", throw: "던지기" };
/** Guns whose shots are followed by a bolt or pump cycle: the server's hasScope and shellReload ones. */
const CYCLING = [WeaponKind.SNIPER, WeaponKind.SHOTGUN];
const FP_DT = 1 / 30;
/** The bench's cells, at the game's aspect; idle ones larger, to see how the hands hold the weapon. */
const FP_VIEW = { width: 320, height: 180 };
const FP_IDLE_VIEW = { width: 480, height: 270 };

function firstPersonStates(kinds: WeaponKind[]): FirstPersonState[] {
  const states: FirstPersonState[] = [];
  const add = (weapon: WeaponKind, pose: FirstPersonPose, team = Team.RED, shield = false) => {
    // Red is the default look; the others say theirs.
    const look = team === Team.BLUE ? { id: ":blue", label: " (BLUE)" } : team === Team.RED ? { id: "", label: "" } : { id: ":neutral", label: " (개인전)" };
    states.push({
      id: `fp:${WeaponKind[weapon]}:${pose}${look.id}${shield ? ":shield" : ""}`,
      label: `${WeaponKind[weapon]} — ${POSE_LABELS[pose]}${look.label}${shield ? " + 방패" : ""}`,
      weapon,
      team,
      pose,
      shield,
    });
  };
  for (const kind of kinds) {
    add(kind, "idle");
    if (kind === WeaponKind.KNIFE || kind === WeaponKind.HAMMER) {
      add(kind, "swing");
    } else if (kind === WeaponKind.GRENADE) {
      add(kind, "throw");
    } else {
      add(kind, "kick");
      add(kind, "reload");
    }
    if (CYCLING.includes(kind)) add(kind, "cycle");
    add(kind, "idle", Team.BLUE);
  }
  add(WeaponKind.PISTOL, "idle", Team.RED, true);
  // The free-for-all's neutral sleeves, on the weapon every player carries.
  add(WeaponKind.KNIFE, "idle", Team.UNSPECIFIED);
  return states;
}

/**
 * The hands that must show at rest: the one on the grip of a one-handed weapon or of each akimbo
 * gun, and the support hand of a two-handed gun (a long gun's right hand may sit below the view).
 */
function handsInView(weapon: WeaponKind, measured: ViewModelMeasure): (number[] | null)[] {
  const art = weaponArt(weapon);
  if (!art) return [];
  if (art.fp.handX !== undefined) return [measured.hands.right, measured.hands.left];
  return [art.fp.oneHanded ? measured.hands.right : measured.hands.left];
}

/** Drives a view model the way game.ts does, a third of a second at rest, then into `pose`. */
function holdPose(viewModel: ViewModel, pose: FirstPersonPose): void {
  for (let i = 0; i < 10; i++) viewModel.update(FP_DT, false, 0, 0, true);
  if (pose === "kick") {
    // Just after the shot: the kick near its height and the flash still up.
    viewModel.fire();
    viewModel.update(0.01, false, 0, 0, true);
  } else if (pose === "reload") {
    // 40% through: the gun dipped and the left hand as far from it as it goes.
    viewModel.update(FP_DT, false, 0.4, 0, true);
  } else if (pose === "cycle") {
    viewModel.update(FP_DT, false, 0, 0.5, true);
  } else if (pose === "swing") {
    // Halfway through the swing: the knife's slash at its widest, the hammer's bonk landing.
    viewModel.fire();
    viewModel.update(0.5 / 3.4, false, 0, 0, true);
  } else if (pose === "throw") {
    // Drawn back over the shoulder, a quarter of the way through: the grenade is still in hand.
    viewModel.throwGrenade();
    viewModel.update(0.12, false, 0, 0, true);
  }
}

function firstPersonCard(viewer: Viewer, state: FirstPersonState, report: ArtReport): HTMLElement {
  const root = el("article", state.pose === "idle" ? "art-card art-card-fp art-card-fp-idle" : "art-card art-card-fp");
  root.dataset.state = state.id;
  const canvas = el("canvas");
  root.append(el("h3", "", state.label), canvas);
  const view = state.pose === "idle" ? FP_IDLE_VIEW : FP_VIEW;
  const viewModel = new ViewModel();
  viewModel.setTeam(state.team);
  viewModel.setWeapon(state.weapon);
  viewModel.setShield(state.shield);
  viewModel.setAspect(view.width / view.height);
  holdPose(viewModel, state.pose);
  const measured = viewModel.measure();
  const calls = viewer.drawCalls(viewModel.scene, viewModel.camera);
  // The outdoor sky's colour behind it; the game draws it over the world instead.
  viewModel.scene.background = new THREE.Color(0xb8cfdc);
  viewer.addCell({ canvas, width: view.width, height: view.height, scene: viewModel.scene, views: [{ camera: viewModel.camera, x: 0, width: view.width }] });

  const art = weaponArt(state.weapon);
  const twoHanded = !!art && !art.fp.oneHanded;
  const mm = (metres: number | null) => (metres === null ? "없음" : `${(metres * 1000).toFixed(2)} mm`);
  const facts = el("dl", "art-facts");
  const held = measured.right !== null && measured.right <= 0.015 && (twoHanded ? measured.left !== null && measured.left <= 0.015 : measured.left === null);
  fact(facts, "주먹 (오른 · 왼)", `${mm(measured.right)} · ${twoHanded ? mm(measured.left) : "총에서 뗌"}`, held ? "ok" : "over");
  if (measured.supportFacing !== null) fact(facts, "받침 손바닥 (위와)", measured.supportFacing.toFixed(2), measured.supportFacing > 0.7 ? "ok" : "over");
  wristFact(facts, measured.wrist, state.weapon);
  const shown = (ndc: number[] | null) => ndc !== null && Math.abs(ndc[0]) < 1 && Math.abs(ndc[1]) < 1 && Math.abs(ndc[2]) < 1;
  const hands = [measured.hands.right, measured.hands.left].filter((ndc): ndc is number[] => ndc !== null);
  const handsShown = state.pose === "idle" ? (handsInView(state.weapon, measured).every(shown) ? "ok" : "over") : undefined;
  fact(facts, "손 (화면 좌표)", hands.map((ndc) => ndc.slice(0, 2).map((value) => value.toFixed(2)).join(", ")).join(" / ") || "없음", handsShown);
  const onScreen = measured.muzzles.every(([x, y, z]) => Math.abs(x) < 1 && Math.abs(y) < 1 && Math.abs(z) < 1);
  fact(facts, "총구 (화면 좌표)", measured.muzzles.map((muzzle) => muzzle.slice(0, 2).map((value) => value.toFixed(2)).join(", ")).join(" / "), state.pose === "idle" ? (onScreen ? "ok" : "over") : undefined);
  fact(facts, "draw call", `${calls} (팔 삼각형 ${number(measured.armTriangles)})`, calls <= 5 ? "ok" : "over");
  root.append(facts);
  report.firstPerson[state.id] = {
    ...measured,
    label: state.label,
    weapon: WeaponKind[state.weapon],
    team: state.team === Team.BLUE ? "blue" : state.team === Team.RED ? "red" : "neutral",
    pose: state.pose,
    shield: state.shield,
    twoHanded,
    calls,
  };
  return root;
}

// ------------------------------------------------------------------ weapons

const WEAPON_SPAN = 1.32;

function weaponCard(viewer: Viewer, kind: WeaponKind, gltf: GLTF | undefined, report: ArtReport): HTMLElement {
  const art = weaponArt(kind);
  const name = WeaponKind[kind];
  const { root, facts, canvas } = card(`${name} — ${art?.file ?? "없음"}`, "weapon");
  report.weapons[name] = { file: art?.file ?? null, loaded: !!gltf, muzzle: false, triangles: 0 };
  if (!art || !gltf) {
    root.append(el("p", "art-failures", "모델 없음"));
    return root;
  }
  const model = gltf.scene.clone();
  toLambert(model);
  const muzzle = model.getObjectByName("muzzle");
  const scene = stageScene(0xc9cfcc);
  // A 10 cm grid behind the gun, seen from its right side with the barrel pointing right.
  const grid = new THREE.GridHelper(2, 20, 0x9aa19d, 0xb2b8b4);
  grid.rotation.z = Math.PI / 2;
  grid.position.x = -0.4;
  scene.add(grid, model, marker(0x2f9e44, 0.05));
  const box = new THREE.Box3().setFromObject(model);
  const centre = box.getCenter(new THREE.Vector3());
  if (muzzle) {
    const dot = marker(0xe03131, 0.06);
    muzzle.getWorldPosition(dot.position);
    scene.add(dot);
  }
  const width = 330;
  const height = 150;
  const half = WEAPON_SPAN / 2;
  const camera = new THREE.OrthographicCamera(-half, half, (half * height) / width, (-half * height) / width, 0.1, 10);
  camera.position.set(3, centre.y, centre.z);
  camera.lookAt(0, centre.y, centre.z);
  viewer.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }] });
  root.append(el("div", "art-legend", "초록: 손잡이(원점) · 빨강: muzzle 노드 · 격자 10 cm"));
  const triangles = trianglesIn(model);
  const bytes = generatedInfo(art.file)?.bytes ?? 0;
  fact(facts, "길이", `${(box.max.z - box.min.z).toFixed(2)} m, 총구 ${muzzle ? muzzle.position.toArray().map((v) => v.toFixed(3)).join(", ") : "없음"}`, muzzle ? "ok" : "over");
  fact(facts, "삼각형", `${number(triangles)} / 예산 ${number(budgets.triangles.gun)}`, triangles <= budgets.triangles.gun ? "ok" : "over");
  fact(facts, "파일", `${kb(bytes)} / 예산 ${budgets.fileKB.gun} KB`, bytes <= budgets.fileKB.gun * 1024 ? "ok" : "over");
  fact(facts, "자세 · 1인칭", `${art.hold}${kind === WeaponKind.AKIMBO_SMG ? " (2정)" : ""} · 배율 ${art.fp.scale ?? 1} · 반동 ${art.fp.kickScale}`);
  root.append(facts);
  report.weapons[name] = { file: art.file, loaded: true, muzzle: !!muzzle, triangles };
  return root;
}

function projectileCard(viewer: Viewer, file: string, gltf: GLTF): HTMLElement {
  const { root, facts, canvas } = card(`MISSILE 탄 — ${file}`, "weapon");
  const model = gltf.scene.clone();
  toLambert(model);
  const scene = stageScene(0xc9cfcc);
  const grid = new THREE.GridHelper(2, 20, 0x9aa19d, 0xb2b8b4);
  grid.rotation.z = Math.PI / 2;
  grid.position.x = -0.4;
  scene.add(grid, model, marker(0x2f9e44, 0.05));
  const width = 330;
  const height = 150;
  const half = WEAPON_SPAN / 2;
  const camera = new THREE.OrthographicCamera(-half, half, (half * height) / width, (-half * height) / width, 0.1, 10);
  camera.position.set(3, 0, 0);
  camera.lookAt(0, 0, 0);
  viewer.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }] });
  root.append(el("div", "art-legend", "+Z(왼쪽)를 향함 · 초록: 원점"));
  fact(facts, "삼각형", number(trianglesIn(model)));
  fact(facts, "파일", kb(generatedInfo(file)?.bytes ?? 0));
  root.append(facts);
  return root;
}

// ------------------------------------------------------------------ map kits

function kitCard(viewer: Viewer, name: string, kit: KitArt, gltf: GLTF, report: ArtReport): HTMLElement {
  const { root, facts, canvas } = card(`${name} — ${kit.file}`, "kit");
  const model = gltf.scene.clone();
  toLambert(model);
  if (kit.tint && kit.tintNode) {
    // What an instance colour does at runtime: multiplies the tinted node's white material.
    model.getObjectByName(kit.tintNode)?.traverse((object) => {
      if (!(object instanceof THREE.Mesh) || !(object.material instanceof THREE.MeshLambertMaterial)) return;
      object.material = object.material.clone();
      (object.material as THREE.MeshLambertMaterial).color.set(kit.tint!);
    });
  }
  const box = new THREE.Box3().setFromObject(model);
  const measured = box.getSize(new THREE.Vector3()).toArray();
  const scene = stageScene();
  scene.add(model);
  const extent = kit.size ?? measured;
  if (kit.size) scene.add(wireBox(kit.size[0], 0, kit.size[1], kit.size[2], 0xe7a42c));
  const reach = Math.max(extent[0], extent[2], 1);
  scene.add(ground(Math.ceil(reach * 1.8), Math.ceil(reach * 1.8)));
  const width = 290;
  const height = 210;
  const camera = new THREE.PerspectiveCamera(32, width / height, 0.05, 400);
  const radius = 0.5 * Math.hypot(extent[0], extent[1], extent[2]);
  const distance = (radius / Math.sin(THREE.MathUtils.degToRad(16))) * 1.05;
  camera.position.copy(new THREE.Vector3(1, 0.62, 1.35).normalize().multiplyScalar(distance)).add(new THREE.Vector3(0, extent[1] / 2, 0));
  camera.lookAt(0, extent[1] / 2, 0);
  viewer.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }] });
  fact(facts, "크기", `${metres(measured)} m${kit.size ? ` (서버 상자 ${metres(kit.size, 1)})` : ""}`);
  if (kit.tint) fact(facts, "색", `${kit.tint} → ${kit.tintNode}`);
  fact(facts, "삼각형", number(trianglesIn(model)));
  fact(facts, "파일", kb(generatedInfo(kit.file)?.bytes ?? 0));
  root.append(facts);
  report.kits[name] = { file: kit.file, size: kit.size ?? null, measured };
  return root;
}

// ------------------------------------------------------------------ the map bench

export interface MapBenchResult {
  /** The BoxKind and PropKind names the bench map holds. */
  boxKinds: string[];
  propKinds: string[];
  /** Draw calls of the world alone with all of it in view, no shadow pass; and their budget. */
  calls: number;
  budget: number;
  /** Kinds in the map with nothing drawn for them. */
  undrawn: string[];
  /**
   * Every box drawn with a kit model: [minX, minY, minZ, maxX, maxY, maxZ] of the box and of the
   * copy that fills it, measured from the copy's vertices, and the largest difference in metres.
   * `colours` are the copy's instance colours ("#rrggbb"), and `tinted` says whether they are the
   * kit's tint (for a kit without one: whether it has none).
   */
  kits: { kind: string; alongZ: boolean; box: number[]; bounds: number[] | null; error: number; colours: string[]; tinted: boolean }[];
  /**
   * Every leaning container, measured in the frame of the box it replaces: a box centred on the
   * prop, turned by (0, rotation, 0.34). Its extents there, and their largest difference from
   * that box's in metres.
   */
  tilted: { rotation: number; bounds: number[] | null; error: number }[];
  /** Lamps in the map, and copies of the lamp model drawn. */
  lamps: { props: number; copies: number };
  /**
   * Each textured prop plane kind, drawn alone under flat white light and read back once the page
   * has drawn it: whether its material has a map, its UV range [minU, maxU, minV, maxV], the
   * spread (standard deviation) of luma in the texture and on screen, and how well the two
   * correlate pixel by pixel. A plane that lost its texture or UVs draws one flat colour.
   */
  planes: { kind: string; map: boolean; uv: number[]; textureSpread: number; drawnSpread: number; correlation: number }[];
}

/** The leaning box TILTED_CONTAINER drew before kit models: as tall as a container, one end tipped up 0.34 rad. */
const TILTED_BOX_HEIGHT = 2.6;
const TILTED_BOX_TIP = 0.34;
const KIT_BOX_KINDS = [BoxKind.CONTAINER_BLUE, BoxKind.CONTAINER_GRAY, BoxKind.CONTAINER_RUST, BoxKind.CONTAINER_PALE, BoxKind.CRATE, BoxKind.BARREL];
/** Props drawn as a plane with a texture painted in code. */
const PLANE_KINDS = [PropKind.SHUTTER, PropKind.BILLBOARD, PropKind.WINDOW, PropKind.LOUVER];
/** The plane check's views: this tall, each as wide as its plane's proportions ask. */
const PLANE_VIEW_HEIGHT = 72;
/** And framed on this middle part of the plane, so no edge or background gets in. */
const PLANE_FRAMING = 0.9;

/** A small outdoor map with every BoxKind and PropKind: containers along both axes and stacked, two leaning ones. */
function benchMap(): MapData {
  const box = (kind: BoxKind, minX: number, maxX: number, minY: number, maxY: number, minZ: number, maxZ: number) =>
    create(MapBoxSchema, { kind, minX, maxX, minY, maxY, minZ, maxZ });
  const prop = (kind: PropKind, x: number, y: number, z: number, rotation: number, width = 0, height = 0, tilt = 0) =>
    create(MapPropSchema, { kind, x, y, z, rotation, width, height, tilt });
  return create(MapDataSchema, {
    name: "BENCH",
    id: MapId.SUPPLY_YARD,
    halfX: 16,
    halfZ: 11,
    boxes: [
      // A back wall with a dock in front, and a roof on two posts.
      box(BoxKind.BRICK, -15, 15, 0, 4, -10.5, -10),
      box(BoxKind.CONCRETE, -5, 5, 0, 1, -10, -8.4),
      box(BoxKind.ROOF, -15, -7, 3.6, 3.9, -10, -3),
      box(BoxKind.BRICK, -15, -14.6, 0, 3.6, -3.4, -3),
      box(BoxKind.BRICK, -7.4, -7, 0, 3.6, -3.4, -3),
      // Containers along X, one stacked on another, and along Z.
      box(BoxKind.CONTAINER_BLUE, -2, 4, 0, 2.6, -3, -0.6),
      box(BoxKind.CONTAINER_RUST, -2, 4, 2.6, 5.2, -3, -0.6),
      box(BoxKind.CONTAINER_GRAY, 7, 9.4, 0, 2.6, -4, 2),
      box(BoxKind.CONTAINER_PALE, 11, 13.4, 0, 2.6, 2, 8),
      box(BoxKind.CRATE, -6, -4.8, 0, 1.2, 3, 4.2),
      box(BoxKind.CRATE, -4.8, -3.6, 0, 1.2, 3, 4.2),
      box(BoxKind.CRATE, -5.4, -4.2, 1.2, 2.4, 3, 4.2),
      box(BoxKind.BARREL, -5.5, -4.6, 0, 1.1, 6.5, 7.4),
      box(BoxKind.BARREL, -4.4, -3.5, 0, 1.1, 7.1, 8),
      // The first leaning container's footprint.
      box(BoxKind.INVISIBLE, -1, 5, 0, 4.2, 5.8, 9.2),
    ],
    props: [
      prop(PropKind.SHUTTER, -11, 1.8, -9.97, 0, 3, 2.4),
      prop(PropKind.BILLBOARD, 0, 2.7, -9.96, 0, 6, 1.2),
      prop(PropKind.LOUVER, 6, 3.2, -9.97, 0, 2.5, 0.9),
      prop(PropKind.WINDOW, 10, 2.2, -9.97, 0, 3, 1.8),
      prop(PropKind.TRUSS, -11, 3.2, -6.5, 0, 7, 0.4),
      prop(PropKind.CEILING_LIGHT, -9, 3.45, -6.5, 0),
      prop(PropKind.FLOOR_LINE, 0, 0.015, -7.9, 0, 10, 0.16),
      prop(PropKind.FLOOR_LINE, -11, 0.015, -2.4, Math.PI / 2, 3, 0.16),
      prop(PropKind.LAMP, 14.5, 0, -3, 0),
      prop(PropKind.LAMP, -14.5, 0, 1, Math.PI),
      prop(PropKind.TILTED_CONTAINER, 2, 1.7, 7.5, 0, 6, 2.4, TILTED_BOX_TIP),
      prop(PropKind.TILTED_CONTAINER, -11, 1.7, 7.5, Math.PI, 6, 2.4, TILTED_BOX_TIP),
    ],
  });
}

/** The world-space vertices of every copy in a kit's instanced meshes (named "<file>#…"), by copy. */
function kitCopies(world: THREE.Object3D, file: string): { points: THREE.Vector3[]; meshes: { mesh: THREE.InstancedMesh; index: number }[] }[] {
  const copies: { points: THREE.Vector3[]; meshes: { mesh: THREE.InstancedMesh; index: number }[] }[] = [];
  const matrix = new THREE.Matrix4();
  world.updateMatrixWorld(true);
  world.traverse((object) => {
    if (!(object instanceof THREE.InstancedMesh) || !object.name.startsWith(`${file}#`)) return;
    const position = object.geometry.getAttribute("position");
    for (let i = 0; i < object.count; i++) {
      object.getMatrixAt(i, matrix);
      matrix.premultiply(object.matrixWorld);
      const copy = (copies[i] ??= { points: [], meshes: [] });
      copy.meshes.push({ mesh: object, index: i });
      for (let v = 0; v < position.count; v++) copy.points.push(new THREE.Vector3().fromBufferAttribute(position, v).applyMatrix4(matrix));
    }
  });
  return copies;
}

const extents = (box: THREE.Box3) => [...box.min.toArray(), ...box.max.toArray()];
const largestGap = (a: number[], b: number[]) => Math.max(...a.map((value, i) => Math.abs(value - b[i])));

/** The copy whose bounds centre is nearest a point. */
function nearestCopy<T extends { bounds: THREE.Box3 }>(copies: T[], point: THREE.Vector3): T | undefined {
  let best: T | undefined;
  let distance = Infinity;
  for (const copy of copies) {
    const d = copy.bounds.getCenter(new THREE.Vector3()).distanceTo(point);
    if (d < distance) [best, distance] = [copy, d];
  }
  return best;
}

/** A canvas's pixels, whatever kind of context drew it (a cell's is a bitmap renderer). */
function pixelsOf(canvas: HTMLCanvasElement): ImageData {
  const copy = el("canvas");
  copy.width = canvas.width;
  copy.height = canvas.height;
  const context = copy.getContext("2d", { willReadFrequently: true })!;
  context.drawImage(canvas, 0, 0);
  return context.getImageData(0, 0, copy.width, copy.height);
}

/** Luma 0..1 of every pixel, from the sRGB values as stored. */
function lumaOf(image: ImageData): Float64Array {
  const luma = new Float64Array(image.width * image.height);
  for (let i = 0; i < luma.length; i++) luma[i] = (0.2126 * image.data[i * 4] + 0.7152 * image.data[i * 4 + 1] + 0.0722 * image.data[i * 4 + 2]) / 255;
  return luma;
}

function spread(values: number[]): number {
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  return Math.sqrt(values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / values.length);
}

/** Pearson correlation; 0 when either side is flat. */
function correlation(a: number[], b: number[]): number {
  const ma = a.reduce((sum, value) => sum + value, 0) / a.length;
  const mb = b.reduce((sum, value) => sum + value, 0) / b.length;
  let ab = 0;
  let aa = 0;
  let bb = 0;
  for (let i = 0; i < a.length; i++) {
    ab += (a[i] - ma) * (b[i] - mb);
    aa += (a[i] - ma) ** 2;
    bb += (b[i] - mb) ** 2;
  }
  return aa > 0 && bb > 0 ? ab / Math.sqrt(aa * bb) : 0;
}

/**
 * Every textured plane kind's first copy in the world, alone in a cell under flat white light,
 * framed on its middle part; and the measurement to take from that cell once it is drawn: each
 * view's pixels against the texture averaged over the area each pixel covers, as a GPU's mipmaps do.
 */
function planeCheck(viewer: Viewer, world: THREE.Object3D): { canvas: HTMLCanvasElement; measure: () => MapBenchResult["planes"] } {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x000000);
  // Physically based lighting divides a Lambert surface's light by π; this gives back its colour.
  scene.add(new THREE.AmbientLight(0xffffff, Math.PI));
  const views: { camera: THREE.Camera; x: number; width: number }[] = [];
  const planes: { kind: PropKind; mesh: THREE.InstancedMesh | null; x: number; width: number }[] = [];
  let x = 0;
  PLANE_KINDS.forEach((kind, i) => {
    const mesh = world.getObjectByName(PropKind[kind]);
    if (!(mesh instanceof THREE.InstancedMesh)) {
      planes.push({ kind, mesh: null, x, width: 0 });
      return;
    }
    const placed = new THREE.Matrix4();
    mesh.getMatrixAt(0, placed);
    const size = new THREE.Vector3();
    placed.decompose(new THREE.Vector3(), new THREE.Quaternion(), size);
    // The same geometry and material as the world's, as an instance like the world's, facing +Z.
    const copy = new THREE.InstancedMesh(mesh.geometry, mesh.material, 1);
    copy.setMatrixAt(0, new THREE.Matrix4().compose(new THREE.Vector3(i * 40, 0, 0), new THREE.Quaternion(), size));
    copy.computeBoundingSphere();
    scene.add(copy);
    const half = PLANE_FRAMING / 2;
    const camera = new THREE.OrthographicCamera(-size.x * half, size.x * half, size.y * half, -size.y * half, 0.1, 10);
    camera.position.set(i * 40, 0, 5);
    const width = Math.round((PLANE_VIEW_HEIGHT * size.x) / size.y);
    views.push({ camera, x, width });
    planes.push({ kind, mesh, x, width });
    x += width;
  });
  const canvas = el("canvas");
  viewer.addCell({ canvas, width: x, height: PLANE_VIEW_HEIGHT, scene, views });

  const measure = (): MapBenchResult["planes"] => {
    const drawn = pixelsOf(canvas);
    const drawnLuma = lumaOf(drawn);
    const ratio = drawn.width / x;
    const height = drawn.height;
    return planes.map(({ kind, mesh, x: left, width }) => {
      const material = mesh?.material as THREE.MeshLambertMaterial | undefined;
      const uv = mesh?.geometry.getAttribute("uv");
      const us = uv ? Array.from({ length: uv.count }, (_, i) => uv.getX(i)) : [];
      const vs = uv ? Array.from({ length: uv.count }, (_, i) => uv.getY(i)) : [];
      const image = material?.map?.image as HTMLCanvasElement | undefined;
      const result = { kind: PropKind[kind], map: !!material?.map, uv: us.length ? [Math.min(...us), Math.max(...us), Math.min(...vs), Math.max(...vs)] : [], textureSpread: 0, drawnSpread: 0, correlation: 0 };
      if (!mesh || !image) return result;
      const texture = pixelsOf(image);
      const textureLuma = lumaOf(texture);
      // Summed-area table, for the texture's mean over any rectangle.
      const tw = texture.width;
      const th = texture.height;
      const sums = new Float64Array((tw + 1) * (th + 1));
      for (let ty = 0; ty < th; ty++) {
        for (let tx = 0; tx < tw; tx++) sums[(ty + 1) * (tw + 1) + tx + 1] = textureLuma[ty * tw + tx] + sums[ty * (tw + 1) + tx + 1] + sums[(ty + 1) * (tw + 1) + tx] - sums[ty * (tw + 1) + tx];
      }
      const area = (x0: number, y0: number, x1: number, y1: number) =>
        (sums[y1 * (tw + 1) + x1] - sums[y0 * (tw + 1) + x1] - sums[y1 * (tw + 1) + x0] + sums[y0 * (tw + 1) + x0]) / ((x1 - x0) * (y1 - y0));
      const onScreen: number[] = [];
      const inTexture: number[] = [];
      const x0 = Math.round(left * ratio);
      const w = Math.round(width * ratio);
      // Two pixels in from the view's edges; screen rows run down like the canvas's.
      for (let py = 2; py < height - 2; py++) {
        for (let px = 2; px < w - 2; px++) {
          onScreen.push(drawnLuma[py * drawn.width + x0 + px]);
          const at = (s: number) => (1 - PLANE_FRAMING) / 2 + PLANE_FRAMING * s;
          const u0 = Math.floor(at(px / w) * tw);
          const u1 = Math.max(u0 + 1, Math.ceil(at((px + 1) / w) * tw));
          const v0 = Math.floor(at(py / height) * th);
          const v1 = Math.max(v0 + 1, Math.ceil(at((py + 1) / height) * th));
          inTexture.push(area(u0, v0, Math.min(u1, tw), Math.min(v1, th)));
        }
      }
      return { ...result, textureSpread: spread(inTexture), drawnSpread: spread(onScreen), correlation: correlation(onScreen, inTexture) };
    });
  };
  return { canvas, measure };
}

function mapBench(viewer: Viewer): { card: HTMLElement; result: MapBenchResult; afterDraw: () => void } {
  const root = el("article", "art-card art-card-map");
  const canvas = el("canvas");
  root.append(el("h3", "", "모든 BoxKind · PropKind를 담은 작은 맵 — 게임의 buildWorld"), canvas);
  const map = benchMap();
  const scene = new THREE.Scene();
  const world = buildWorld(scene, map);

  // Draw calls with everything in view from above.
  const above = new THREE.PerspectiveCamera(60, 1, 0.1, 1000);
  above.position.set(0, 60, 0.001);
  above.lookAt(0, 0, 0);
  const calls = viewer.drawCalls(scene, above);
  const undrawn: string[] = [];
  const named = new Set<string>();
  world.traverse((object) => {
    if (object instanceof THREE.Mesh || object instanceof THREE.Line) named.add(object.name);
  });

  const kits: MapBenchResult["kits"] = [];
  const copiesByFile = new Map<string, { bounds: THREE.Box3; points: THREE.Vector3[]; meshes: { mesh: THREE.InstancedMesh; index: number }[] }[]>();
  const copiesOf = (file: string) => {
    if (!copiesByFile.has(file)) copiesByFile.set(file, kitCopies(world, file).map((copy) => ({ ...copy, bounds: new THREE.Box3().setFromPoints(copy.points) })));
    return copiesByFile.get(file)!;
  };
  for (const b of map.boxes.filter((item) => KIT_BOX_KINDS.includes(item.kind))) {
    const art = kitArt(BoxKind[b.kind]);
    const box = [b.minX, b.minY, b.minZ, b.maxX, b.maxY, b.maxZ];
    const centre = new THREE.Vector3((b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2, (b.minZ + b.maxZ) / 2);
    const copy = art && nearestCopy(copiesOf(art.file), centre);
    const bounds = copy ? extents(copy.bounds) : null;
    const tint = art?.tint ? new THREE.Color(art.tint).toArray() : null;
    // Instance colours are stored as 32-bit floats.
    const colours = copy?.meshes.filter(({ mesh }) => mesh.instanceColor).map(({ mesh, index }) => mesh.getColorAt(index, new THREE.Color()).toArray()) ?? [];
    kits.push({
      kind: BoxKind[b.kind],
      alongZ: b.maxZ - b.minZ > b.maxX - b.minX + 0.01,
      box,
      bounds,
      error: bounds ? largestGap(bounds, box) : Infinity,
      colours: colours.map((colour) => `#${new THREE.Color().fromArray(colour).getHexString()}`),
      tinted: tint ? colours.length > 0 && colours.every((colour) => largestGap(colour, tint) < 1e-4) : colours.length === 0,
    });
  }

  const tilted: MapBenchResult["tilted"] = [];
  const tiltedArt = propArt(PropKind[PropKind.TILTED_CONTAINER]);
  for (const p of map.props.filter((item) => item.kind === PropKind.TILTED_CONTAINER)) {
    const at = new THREE.Vector3(p.x, p.y, p.z);
    const copy = tiltedArt && nearestCopy(copiesOf(tiltedArt.file), at);
    const frame = new THREE.Matrix4().compose(at, new THREE.Quaternion().setFromEuler(new THREE.Euler(0, p.rotation, TILTED_BOX_TIP)), new THREE.Vector3(1, 1, 1)).invert();
    const bounds = copy ? extents(new THREE.Box3().setFromPoints(copy.points.map((point) => point.clone().applyMatrix4(frame)))) : null;
    const box = [-p.width / 2, -TILTED_BOX_HEIGHT / 2, -p.height / 2, p.width / 2, TILTED_BOX_HEIGHT / 2, p.height / 2];
    tilted.push({ rotation: p.rotation, bounds, error: bounds ? largestGap(bounds, box) : Infinity });
  }

  const lampArt = propArt(PropKind[PropKind.LAMP]);
  const lamps = { props: map.props.filter((item) => item.kind === PropKind.LAMP).length, copies: lampArt ? copiesOf(lampArt.file).length : 0 };
  const boxKinds = [...new Set(map.boxes.map((item) => item.kind))];
  const propKinds = [...new Set(map.props.map((item) => item.kind))];
  // Kit copies are checked one by one above and invisible boxes draw nothing; world.ts names the
  // one mesh it draws for each other kind after the kind.
  for (const kind of boxKinds) {
    if (kind !== BoxKind.INVISIBLE && !KIT_BOX_KINDS.includes(kind) && !named.has(BoxKind[kind])) undrawn.push(BoxKind[kind]);
  }
  for (const kind of propKinds) {
    if (kind !== PropKind.LAMP && kind !== PropKind.TILTED_CONTAINER && !named.has(PropKind[kind])) undrawn.push(PropKind[kind]);
  }

  // The server's boxes over the world, as the kit cards show them.
  for (const b of map.boxes) {
    const wire = wireBox(b.maxX - b.minX, b.minY, b.maxY, b.maxZ - b.minZ, b.kind === BoxKind.INVISIBLE ? 0xff3df2 : 0xe7a42c);
    wire.position.x = (b.minX + b.maxX) / 2;
    wire.position.z = (b.minZ + b.maxZ) / 2;
    scene.add(wire);
  }
  const width = 470;
  const height = 320;
  const overview = new THREE.PerspectiveCamera(46, width / height, 0.1, 1000);
  overview.position.set(-15, 14, 21);
  overview.lookAt(0.5, 0.5, 0);
  const close = new THREE.PerspectiveCamera(40, width / height, 0.1, 1000);
  close.position.set(13, 5.5, 15);
  close.lookAt(2.5, 1.6, 2.5);
  viewer.addCell({ canvas, width: width * 2, height, scene, views: [{ camera: overview, x: 0, width }, { camera: close, x: width, width }] });
  root.append(el("div", "art-legend", "주황 선: 서버 충돌 상자 · 자홍: 보이지 않는 상자(기울어진 컨테이너의 바닥 면적)"));

  const cm = (metres: number) => (Number.isFinite(metres) ? `${(metres * 100).toFixed(2)} cm` : "없음");
  const worstKit = Math.max(...kits.map((kit) => kit.error));
  const worstTilt = Math.max(...tilted.map((item) => item.error));
  const facts = el("dl", "art-facts");
  fact(facts, "draw call", `${calls} / 예산 ${budgets.drawCalls.map} (전부 보이게, 그림자 패스 없음)`, calls <= budgets.drawCalls.map ? "ok" : "over");
  fact(facts, "키트 상자", `${kits.length}개 (Z축으로 돌린 것 ${kits.filter((kit) => kit.alongZ).length}개), 상자와 최대 차이 ${cm(worstKit)}`, worstKit <= 0.01 ? "ok" : "over");
  fact(facts, "컨테이너 색", kits.every((kit) => kit.tinted) ? "모두 manifest 색" : `틀림: ${kits.filter((kit) => !kit.tinted).map((kit) => kit.kind).join(", ")}`, kits.every((kit) => kit.tinted) ? "ok" : "over");
  fact(facts, "기울어진 컨테이너", `${tilted.length}개, 예전 상자와 최대 차이 ${cm(worstTilt)}`, worstTilt <= 0.01 ? "ok" : "over");
  fact(facts, "가로등", `모델 ${lamps.copies}개 / 소품 ${lamps.props}개`, lamps.copies === lamps.props ? "ok" : "over");
  fact(facts, "그려지지 않은 종류", undrawn.join(", ") || "없음", undrawn.length ? "over" : "ok");
  root.append(facts);

  const planes = planeCheck(viewer, world);
  root.append(planes.canvas, el("div", "art-legend", "텍스처 평면 소품(셔터 · 광고판 · 창문 · 환기창)을 하나씩 흰 빛 아래에서: 화면과 텍스처의 상관을 잼"));
  const planeFacts = el("dl", "art-facts");
  root.append(planeFacts);
  const result: MapBenchResult = {
    boxKinds: boxKinds.map((kind) => BoxKind[kind]),
    propKinds: propKinds.map((kind) => PropKind[kind]),
    calls,
    budget: budgets.drawCalls.map,
    undrawn,
    kits,
    tilted,
    lamps,
    planes: [],
  };
  // The planes are measured from their cell, so only once the page has drawn it.
  const afterDraw = () => {
    result.planes = planes.measure();
    for (const plane of result.planes) {
      const textured = plane.map && plane.correlation >= 0.8;
      fact(planeFacts, plane.kind, `상관 ${plane.correlation.toFixed(3)} · 대비 ${plane.drawnSpread.toFixed(3)} (텍스처 ${plane.textureSpread.toFixed(3)}) · UV ${plane.uv.map((v) => v.toFixed(2)).join("~")}`, textured ? "ok" : "over");
    }
  };
  return { card: root, result, afterDraw };
}

// ------------------------------------------------------------------ effects

function atlasCard(title: string, atlas: AtlasArt, texture: THREE.Texture | undefined, report: ArtReport): HTMLElement {
  const root = el("article", "art-card art-card-atlas");
  root.append(el("h3", "", `${title} — ${atlas.file}`));
  const image = texture?.image as { width: number; height: number } | undefined;
  const width = image?.width ?? 0;
  const height = image?.height ?? 0;
  const cells = Object.entries(atlas.cells);
  const inside = cells.every(([, [x, y, w, h]]) => x >= 0 && y >= 0 && x + w <= width && y + h <= height);
  report.atlases.push({ file: atlas.file, loaded: !!texture, width, height, cells: cells.length, inside });
  if (!texture || !width) {
    root.append(el("p", "art-failures", "이미지 없음"));
    return root;
  }
  const size = 512;
  const scale = size / Math.max(width, height);
  const canvas = el("canvas");
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  const context = canvas.getContext("2d")!;
  context.fillStyle = "#1b1d18";
  context.fillRect(0, 0, canvas.width, canvas.height);
  // The loader flipped the image for WebGL; it is flipped back to draw it.
  context.save();
  context.translate(0, canvas.height);
  context.scale(1, -1);
  context.drawImage(texture.image as CanvasImageSource, 0, 0, canvas.width, canvas.height);
  context.restore();
  context.strokeStyle = "rgba(231, 164, 44, 0.8)";
  context.fillStyle = "#e7a42c";
  context.font = "12px sans-serif";
  for (const [name, [x, y, w, h]] of cells) {
    context.strokeRect(x * scale + 0.5, y * scale + 0.5, w * scale - 1, h * scale - 1);
    context.fillText(name, x * scale + 4, y * scale + 14);
  }
  const facts = el("dl", "art-facts");
  fact(facts, "크기", `${width} × ${height} px, 칸 ${cells.length}개`, inside ? "ok" : "over");
  fact(facts, "파일", kb(generatedInfo(atlas.file)?.bytes ?? 0));
  root.append(canvas, facts);
  return root;
}

// ------------------------------------------------------------------ budgets

function fileBudgets(art: ArtSet): Pick<ArtReport, "files" | "totalBytes" | "downloadLimitBytes"> {
  const characterFiles = new Set(Object.values(manifest.character?.files ?? {}));
  const weaponFiles = new Set(Object.values(manifest.weapons).map((weapon) => weapon.file));
  const files = manifestFiles().map((file) => {
    const bytes = generatedInfo(file)?.bytes ?? null;
    const model = art.models.get(file);
    const triangles = model ? trianglesIn(model.scene) : null;
    const group = characterFiles.has(file) ? "character" : weaponFiles.has(file) ? "gun" : null;
    const limitKB = group ? budgets.fileKB[group] : null;
    const limitTriangles = group ? budgets.triangles[group] : null;
    const ok = (limitKB === null || (bytes !== null && bytes <= limitKB * 1024)) && (limitTriangles === null || (triangles !== null && triangles <= limitTriangles));
    return { file, bytes, triangles, limitKB, limitTriangles, ok };
  });
  return { files, totalBytes: files.reduce((sum, file) => sum + (file.bytes ?? 0), 0), downloadLimitBytes: budgets.downloadMB * 1024 * 1024 };
}

// ------------------------------------------------------------------ page

function section(id: string, title: string, note: string): { root: HTMLElement; grid: HTMLElement; head: HTMLElement } {
  const root = panel(title, note);
  root.classList.add("art-section");
  root.dataset.section = id;
  const grid = el("div", "art-grid");
  root.append(grid);
  return { root, grid, head: root.querySelector(".panel-head")! };
}

export async function showArtPreview(root: HTMLElement): Promise<void> {
  const page = el("div", "lobby art-preview");
  // e2e/art.mjs waits for "ready".
  page.dataset.state = "loading";
  const frame = el("div", "lobby-frame");
  const brand = el("header", "brand");
  brand.append(el("h1", "", "SUDDEN FRONT"), el("p", "", "에셋 미리보기"));
  frame.append(brand);
  page.append(frame);
  root.append(page);

  const kinds = Object.values(WeaponKind).filter((kind): kind is WeaponKind => typeof kind === "number" && kind !== WeaponKind.UNSPECIFIED);
  const weapons = kinds.filter((kind) => weaponArt(kind));
  const files = manifestFiles();
  const contents = panel("매니페스트");
  contents.append(
    rows([
      ["무기", `${weapons.length}/${kinds.length}${weapons.length ? ` · ${weapons.map((kind) => WeaponKind[kind]).join(", ")}` : ""}`],
      ["캐릭터", manifest.character ? Object.values(manifest.character.files).join(", ") : "없음"],
      ["1인칭 팔", manifest.arms ? `캐릭터 파일의 팔 삼각형만 (쇄골 아래), ${manifest.arms.scale}배` : "없음"],
      ["투사체", `${count(manifest.projectiles)}개`],
      ["맵", `표면 ${count(manifest.map.surfaces)} · 키트 ${count(manifest.map.kits)} · 소품 ${count(manifest.map.props)}`],
      ["이펙트", manifest.fx ? `${manifest.fx.file}${manifest.fx.decals ? `, ${manifest.fx.decals.file}` : ""}` : "없음"],
      ["파일", files.length ? `${files.length}개 · ${files.join(", ")}` : "없음"],
    ]),
  );

  const failed = panel("불러오기 실패");
  const status = el("p", "muted", "불러오는 중…");
  failed.append(status);

  const limits = panel("예산");
  limits.append(
    rows([
      ["draw call", `보통 ${budgets.drawCalls.typical} · 최대 ${budgets.drawCalls.max} · 맵 고정 물체 ${budgets.drawCalls.map}`],
      [
        "삼각형",
        `캐릭터 ${number(budgets.triangles.character)} · 1인칭 팔 ${number(budgets.triangles.arms)} · 총기 ${number(budgets.triangles.gun)} · 화면 ${number(budgets.triangles.scene)}`,
      ],
      ["셰이더 프로그램", `${budgets.programs}`],
      ["텍스처 메모리", `${budgets.textureMB} MB`],
      ["다운로드", `${budgets.downloadMB} MB`],
      ["파일 크기", `캐릭터 ${budgets.fileKB.character} KB · 총기 ${budgets.fileKB.gun} KB`],
    ]),
  );
  contents.dataset.section = "manifest";
  failed.dataset.section = "failures";
  limits.dataset.section = "budgets";
  frame.append(contents, failed, limits);

  const art = await loadArt();
  if (art.failures.length === 0) {
    status.textContent = "없음";
  } else {
    const list = el("ul", "art-failures");
    for (const failure of art.failures) list.append(el("li", "art-failure", `${failure.file} — ${failure.reason}`));
    status.replaceWith(list);
  }

  if (new URLSearchParams(location.search).has("candidates")) {
    // The candidates are local only: a production build drops public/art/_candidates/, and with
    // this condition false it leaves out the gallery code as well.
    if (import.meta.env.DEV) {
      page.classList.add("art-gallery-page");
      page.dataset.state = "ready";
      const { showGallery } = await import("./gallery");
      await showGallery(frame);
    } else {
      frame.append(el("p", "panel muted", "후보 갤러리는 개발 서버(npm run dev)에서만 열립니다."));
      page.dataset.state = "ready";
    }
    return;
  }

  page.classList.add("art-sheet");
  const report: ArtReport = { failures: art.failures.map((failure) => ({ ...failure })), ...fileBudgets(art), weapons: {}, characters: {}, kits: {}, atlases: [], bench: {}, firstPerson: {}, map: null, effects: null };
  const viewer = new Viewer();

  const used = report.files.reduce((sum, file) => sum + (file.bytes ?? 0), 0);
  limits.append(
    rows([
      ["지금 파일", `${files.length}개 · ${kb(used)} / ${budgets.downloadMB} MB`],
      ["예산 초과", report.files.filter((file) => !file.ok).map((file) => file.file).join(", ") || "없음"],
    ]),
  );

  const characters = section("characters", "캐릭터", "rest pose의 머리 뼈를 히트박스 목(1.50 m)에 맞춤");
  const picker = el("select");
  const character = manifest.character;
  const firstModel = character ? art.models.get(character.files.red) : undefined;
  for (const clip of firstModel?.animations ?? []) picker.append(new Option(clip.name, clip.name, clip.name === character?.clips.idle, clip.name === character?.clips.idle));
  picker.addEventListener("change", () => viewer.reconfigure());
  characters.head.append(picker);
  if (character) {
    for (const [team, file] of Object.entries(character.files)) {
      const gltf = art.models.get(file);
      if (gltf) characters.grid.append(characterCard(viewer, team, file, gltf, picker, report));
      else report.characters[team] = { file, loaded: false, bones: [], clips: [], headJoint: null, triangles: 0 };
    }
  }

  const bench = section("bench", "캐릭터 시험대", "게임의 원격 캐릭터(Character)를 상태별로 움직여 히트박스와 비교 · 3/4 · 측면");
  if (character) for (const state of benchStates()) bench.grid.append(benchCard(viewer, state, report));

  const firstPerson = section("firstPerson", "1인칭 시험대", "게임의 1인칭 ViewModel을 무기·동작별로: 손과 손잡이·받침점 사이 거리, 총구 화면 좌표, draw call");
  if (manifest.arms && weapons.length) for (const state of firstPersonStates(kinds)) firstPerson.grid.append(firstPersonCard(viewer, state, report));

  const weaponSection = section("weapons", "무기", `측면, 같은 축척 (가로 ${WEAPON_SPAN} m) · 총구는 −Z(오른쪽)`);
  for (const kind of kinds) {
    const file = weaponArt(kind)?.file;
    weaponSection.grid.append(weaponCard(viewer, kind, file ? art.models.get(file) : undefined, report));
  }
  for (const [name, projectile] of Object.entries(manifest.projectiles)) {
    const gltf = projectile && art.models.get(projectile.file);
    if (gltf) weaponSection.grid.append(projectileCard(viewer, projectile.file, gltf));
    else if (projectile) report.failures.push({ file: projectile.file, reason: `${name} projectile did not load` });
  }

  const kits = section("kits", "맵 키트", "주황 선: 서버 충돌 상자 · 컨테이너는 인스턴스 색으로 4색");
  for (const [name, kit] of [...Object.entries(manifest.map.kits), ...Object.entries(manifest.map.props)]) {
    const gltf = kit && art.models.get(kit.file);
    if (kit && gltf) kits.grid.append(kitCard(viewer, name, kit, gltf, report));
  }

  const mapSection = section("map", "맵 시험대", "게임의 buildWorld로 지은 작은 맵: 키트 인스턴스와 서버 충돌 상자의 차이, 맵 고정 물체의 draw call · 개요 · 컨테이너");
  let measureMap = () => {};
  if (Object.keys(manifest.map.kits).length) {
    const built = mapBench(viewer);
    mapSection.grid.append(built.card);
    report.map = built.result;
    measureMap = built.afterDraw;
  }

  const effects = section("effects", "이펙트", "아틀라스 칸 (흰색 + 알파, 색은 코드가 입힘) · 게임의 Effects를 고정 시간 간격으로 돌린 장면과 검사");
  if (manifest.fx) {
    effects.grid.append(atlasCard("파티클", manifest.fx, art.textures.get(manifest.fx.file), report));
    if (manifest.fx.decals) effects.grid.append(atlasCard("데칼", manifest.fx.decals, art.textures.get(manifest.fx.decals.file), report));
    const built = effectsBench(viewer);
    effects.grid.append(...built.cards);
    report.effects = built.result;
  }

  frame.append(characters.root, bench.root, firstPerson.root, weaponSection.root, kits.root, mapSection.root, effects.root);
  // Draws every cell once before it returns.
  viewer.start();
  measureMap();
  (window as unknown as { __artReport: ArtReport }).__artReport = report;
  (window as unknown as { __artGallery: { frames: () => number } }).__artGallery = { frames: () => viewer.frameCount() };
  page.dataset.state = "ready";
}
