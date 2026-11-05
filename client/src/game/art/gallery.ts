// The ?art&candidates gallery, for choosing the look at checkpoint A. It shows every candidate that
// tools/art/stage-candidates.mjs staged into public/art/_candidates/, loaded through the art loader
// and converted to Lambert, against what it has to fit: characters stand in the server's hitboxes,
// guns share one scale, map pieces sit in the boxes the server collides with, and effect sprites
// play in a short loop. The candidates are local only (the production build drops them), so the
// page needs the Vite dev server.
//
// One offscreen WebGL renderer draws every view and copies it into the view's own 2D canvas, so
// the page holds one WebGL context however many cells it has. Animated cells redraw while on
// screen; still ones redraw when a toggle changes them.
import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import * as tex from "../textures";
import { loadArtModel, loadArtTexture } from "./loader";
import { budgets } from "./manifest";
import { toLambert } from "./materials";
import { HEAD_TOP, Viewer, el, fact, ground, hitboxFrame, lineMaterial, metres, number, stageScene, wireBox } from "./view";
import "./gallery.css";

// ------------------------------------------------------------------ the index

interface Fit {
  min: number[];
  max: number[];
  size: number[];
  cut: boolean;
}

interface CharacterFit {
  headBone: string;
  headsTall: number | null;
  headJoint: number;
  neckJoint: number | null;
  chin: number | null;
  facing: string;
  crouchClips: string[];
}

interface Candidate {
  id: string;
  category: "character" | "gun" | "map" | "fx" | "texture";
  role: string;
  title: string;
  pick: boolean;
  notes: string;
  file: string;
  bytes: number;
  pack: { title: string; author: string; spdx: string };
  triangles?: number;
  drawCalls?: number;
  joints?: number;
  clips?: { name: string; duration: number }[];
  fit?: Fit;
  character?: CharacterFit | null;
  uniform?: string[];
  length?: number;
  target?: number[];
  scale?: number;
  tile?: number;
  /** A gun whose muzzle is at its negative end: turned end for end. */
  flip?: boolean;
  /** Degrees about X, Y and Z that replace the automatic side-view turn. */
  turn?: number[];
}

const INDEX = "art/_candidates/index.json";

/** The staged list, or null when there is none: no staging yet, or a build that dropped it. */
async function loadIndex(): Promise<Candidate[] | null> {
  try {
    const response = await fetch(INDEX, { cache: "no-store" });
    // The dev server answers a missing file with the page itself, so the type is checked too.
    if (!response.ok || !(response.headers.get("content-type") ?? "").includes("json")) return null;
    const data = (await response.json()) as { candidates?: Candidate[] };
    return Array.isArray(data.candidates) ? data.candidates : null;
  } catch {
    return null;
  }
}

// ------------------------------------------------------------------ options

type Shading = "lambert" | "toon";
type Tone = "aces" | "neutral";
type TeamTint = "none" | "red" | "blue";
type Motion = "idle" | "run" | "crouch";

interface Options {
  shading: Shading;
  tone: Tone;
  team: TeamTint;
  motion: Motion;
}

const TOGGLES: { key: keyof Options; label: string; values: [string, string][] }[] = [
  { key: "shading", label: "셰이딩", values: [["lambert", "Lambert"], ["toon", "Toon 3단"]] },
  { key: "tone", label: "톤 매핑", values: [["aces", "ACES"], ["neutral", "Neutral"]] },
  { key: "team", label: "팀 색", values: [["none", "원본"], ["red", "RED"], ["blue", "BLUE"]] },
  { key: "motion", label: "동작", values: [["idle", "대기"], ["run", "달리기"], ["crouch", "앉기"]] },
];

/** The fallback figure's team colours (placeholder.ts), blended into the uniform materials. */
const TEAM_COLORS = { red: new THREE.Color(0xb4483a), blue: new THREE.Color(0x3f6fa8) };
const TEAM_MIX = 0.7;

const WEAPON_NAMES: Record<string, string> = {
  RIFLE: "소총 (M416)",
  SNIPER: "저격소총 (AWP)",
  MACHINE_GUN: "기관총 (M249)",
  SMG: "기관단총 (UZI) · 우지 아킴보는 같은 모델 두 개",
  PISTOL: "권총",
  PISTOL_PLUS: "강화 권총",
  MAGNUM: "매그넘",
  SHOTGUN: "샷건",
  MISSILE: "미사일 런처",
  KNIFE: "나이프",
  TASER: "테이저건",
  HAMMER: "뿅망치 (왕대두모드)",
};

const MAP_NAMES: Record<string, string> = {
  container: "컨테이너",
  crate: "상자",
  barrel: "드럼통",
  lamp: "가로등",
  shutter: "셔터",
  window: "창문",
  truss: "트러스",
  louver: "환기창",
  ceilingLight: "천장 조명",
  prop: "소품",
};

const FX_NAMES: Record<string, string> = {
  muzzleFlash: "총구 화염",
  smoke: "연기",
  shockwave: "충격파",
  explosion: "폭발",
  flash: "섬광",
  spark: "불꽃",
  impact: "착탄",
  decal: "탄흔·그을음",
  tracer: "예광탄",
  taser: "테이저 전류",
};

// ------------------------------------------------------------------ materials

/** Three bands of light for the Toon option. */
function toonGradient(): THREE.DataTexture {
  const gradient = new THREE.DataTexture(new Uint8Array([70, 160, 255]), 3, 1, THREE.RedFormat);
  gradient.minFilter = THREE.NearestFilter;
  gradient.magFilter = THREE.NearestFilter;
  gradient.generateMipmaps = false;
  gradient.needsUpdate = true;
  return gradient;
}

/** Keeps each mesh's Lambert material, its Toon twin and the uniforms a team colour tints. */
class Materials {
  private readonly gradient = toonGradient();
  private readonly toons = new Map<THREE.Material, THREE.MeshToonMaterial>();
  private readonly meshes: { mesh: THREE.Mesh; lambert: THREE.Material | THREE.Material[] }[] = [];
  private readonly tinted: { material: THREE.MeshLambertMaterial; base: THREE.Color }[] = [];

  /** Converts root to Lambert, gives each named uniform material its own copy, and tracks it all. */
  adopt(root: THREE.Object3D, uniform: string[] = []): void {
    toLambert(root);
    root.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) return;
      // Toon shading needs normals; Lambert draws a model without them flat either way.
      if (!object.geometry.getAttribute("normal")) object.geometry.computeVertexNormals();
      const own = (material: THREE.Material): THREE.Material => {
        if (!(material instanceof THREE.MeshLambertMaterial) || !uniform.includes(material.name)) return material;
        const copy = material.clone();
        this.tinted.push({ material: copy, base: copy.color.clone() });
        return copy;
      };
      object.material = Array.isArray(object.material) ? object.material.map(own) : own(object.material);
      this.meshes.push({ mesh: object, lambert: object.material });
    });
  }

  /** Tracks a material built here, such as a wall's, so the shading toggle reaches it. */
  track(mesh: THREE.Mesh): void {
    this.meshes.push({ mesh, lambert: mesh.material });
  }

  apply(options: Options): void {
    for (const { material, base } of this.tinted) {
      material.color.copy(base);
      if (options.team !== "none") material.color.lerp(TEAM_COLORS[options.team], TEAM_MIX);
      this.toons.get(material)?.color.copy(material.color);
    }
    const pick = (material: THREE.Material) => (options.shading === "toon" ? this.toon(material) : material);
    for (const { mesh, lambert } of this.meshes) mesh.material = Array.isArray(lambert) ? lambert.map(pick) : pick(lambert);
  }

  private toon(material: THREE.Material): THREE.Material {
    if (!(material instanceof THREE.MeshLambertMaterial)) return material;
    let toon = this.toons.get(material);
    if (!toon) {
      toon = new THREE.MeshToonMaterial({
        name: material.name,
        color: material.color,
        map: material.map,
        gradientMap: this.gradient,
        emissive: material.emissive,
        emissiveIntensity: material.emissiveIntensity,
        emissiveMap: material.emissiveMap,
        vertexColors: material.vertexColors,
        side: material.side,
        transparent: material.transparent,
        alphaTest: material.alphaTest,
        opacity: material.opacity,
        depthWrite: material.depthWrite,
      });
      this.toons.set(material, toon);
    }
    return toon;
  }
}

// ------------------------------------------------------------------ cards

const percent = (value: number) => `${value >= 0 ? "+" : "−"}${Math.abs(value * 100).toFixed(1)}%`;

function card(candidate: { title: string; pick?: boolean; notes?: string }, kind: string): { root: HTMLElement; facts: HTMLDListElement } {
  const root = el("article", `gallery-card gallery-${kind}`);
  const head = el("h3", "", candidate.title);
  if (candidate.pick) head.prepend(el("span", "gallery-pick", "추천"));
  const facts = el("dl", "gallery-facts");
  root.append(head);
  return { root, facts };
}

function finishCard(root: HTMLElement, facts: HTMLDListElement, candidate: Candidate): void {
  fact(facts, "라이선스", `${candidate.pack.spdx} · ${candidate.pack.author}`);
  fact(facts, "출처", `${candidate.pack.title} · ${(candidate.bytes / 1024).toFixed(0)} KB`);
  root.append(facts);
  if (candidate.notes) root.append(el("p", "gallery-notes", candidate.notes));
}

// ------------------------------------------------------------------ the gallery

const EYE_HEIGHT = 1.42;
const GAME_FOV = 72;

function findClip(clips: THREE.AnimationClip[], preferred: RegExp[]): THREE.AnimationClip | undefined {
  for (const pattern of preferred) {
    const clip = clips.find((candidate) => pattern.test(candidate.name));
    if (clip) return clip;
  }
  return undefined;
}

const IDLE = [/(^|\|)idle$/i, /(^|\|)idle_loop$/i, /man_idle$/i, /idle_neutral$/i, /idle/i];
const RUN = [/(^|\|)run$/i, /jog_fwd_loop$/i, /man_run$/i, /run/i, /jog|walk/i];
const CROUCH = [/crouch_idle/i, /crouch/i, /duck/i];
const GUN_POSE = [/idle_gun_pointing$/i, /idle_gun$/i, /pistol_idle_loop$/i];
const RELOAD = [/reload/i, /fire/i];

/** The page's options, the materials they switch, and the viewer that draws it all. */
class Gallery {
  private readonly viewer = new Viewer();
  private readonly materials = new Materials();
  private readonly options: Options = { shading: "lambert", tone: "aces", team: "none", motion: "idle" };

  /** How many frames have been drawn, for scripts that wait on the animation (gallery-shots.mjs). */
  frameCount(): number {
    return this.viewer.frameCount();
  }

  addCell(cell: Parameters<Viewer["addCell"]>[0]): void {
    this.viewer.addCell(cell);
  }

  set(key: keyof Options, value: string): void {
    (this.options as unknown as Record<string, string>)[key] = value;
    this.viewer.setToneMapping(this.options.tone === "aces" ? THREE.ACESFilmicToneMapping : THREE.NeutralToneMapping);
    this.materials.apply(this.options);
    this.viewer.reconfigure();
  }

  get current(): Options {
    return this.options;
  }

  adopt(root: THREE.Object3D, uniform?: string[]): void {
    this.materials.adopt(root, uniform);
  }

  track(mesh: THREE.Mesh): void {
    this.materials.track(mesh);
  }

  /** Draws every cell once, on screen or not, then keeps the visible ones moving. */
  start(): void {
    this.materials.apply(this.options);
    this.viewer.start();
  }
}

// ------------------------------------------------------------------ characters

/** The model scaled so its head top is at 1.76 m, standing on the origin and facing +Z. */
function standCharacter(candidate: Candidate, gltf: GLTF): { holder: THREE.Group; model: THREE.Object3D; scale: number } {
  const fit = candidate.fit!;
  const model = cloneSkinned(gltf.scene);
  const scale = HEAD_TOP / fit.size[1];
  model.scale.setScalar(scale);
  model.position.set((-(fit.min[0] + fit.max[0]) / 2) * scale, -fit.min[1] * scale, (-(fit.min[2] + fit.max[2]) / 2) * scale);
  const holder = new THREE.Group();
  holder.add(model);
  if (candidate.character?.facing === "-Z") holder.rotation.y = Math.PI;
  return { holder, model, scale };
}

function characterCard(gallery: Gallery, candidate: Candidate, gltf: GLTF): HTMLElement {
  const { root, facts } = card(candidate, "character");
  const canvas = el("canvas");
  root.append(canvas);

  const scene = stageScene();
  scene.add(ground(3, 6));
  const { holder, model } = standCharacter(candidate, gltf);
  gallery.adopt(model, candidate.uniform);
  scene.add(holder);
  const frame = hitboxFrame();
  scene.add(frame.group);
  const info = candidate.character;
  if (info) {
    // The head bone's height in the rest pose, against the head box's base at 1.50 m.
    const marker = new THREE.LineSegments(
      new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(-0.32, info.headJoint, 0), new THREE.Vector3(0.32, info.headJoint, 0)]),
      lineMaterial(0x6be06b),
    );
    marker.renderOrder = 11;
    scene.add(marker);
  }

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
  const threeQuarter = new THREE.PerspectiveCamera(28, viewWidth / height, 0.1, 30);
  threeQuarter.position.set(2.7, 1.9, 3.6);
  threeQuarter.lookAt(0, 0.92, 0);

  const mixer = new THREE.AnimationMixer(model);
  let playing: THREE.AnimationAction | null = null;
  const clips = gltf.animations;
  gallery.addCell({
    canvas,
    width: viewWidth * 3,
    height,
    scene,
    views: [
      { camera: ortho(0, 6), x: 0, width: viewWidth },
      { camera: ortho(6, 0), x: viewWidth, width: viewWidth },
      { camera: threeQuarter, x: viewWidth * 2, width: viewWidth },
    ],
    tick: (dt) => mixer.update(dt),
    configure: () => {
      const options = gallery.current;
      const clip = findClip(clips, options.motion === "run" ? RUN : options.motion === "crouch" ? CROUCH : IDLE) ?? findClip(clips, IDLE);
      frame.crouch(options.motion === "crouch");
      const action = clip ? mixer.clipAction(clip) : null;
      if (action !== playing) {
        playing?.stop();
        action?.reset().play();
        playing = action;
      }
      shownClip.textContent = clip ? clip.name.replace(/^.*\|/, "") : "없음";
    },
  });
  root.append(el("div", "gallery-views", "정면 · 측면 · 3/4  —  빨강 머리 · 주황 몸통 · 파랑 다리 히트박스, 초록 선은 머리 관절"));

  const triangles = candidate.triangles ?? 0;
  fact(facts, "삼각형", `${number(triangles)} / 예산 ${number(budgets.triangles.character)}`, triangles > budgets.triangles.character ? "over" : "ok");
  fact(facts, "draw call", `${candidate.drawCalls} (재질별 조각)`);
  if (info) {
    const tall = info.headsTall;
    fact(facts, "등신", tall === null ? "측정 불가" : `${tall.toFixed(2)}등신`, tall !== null && tall >= 6.5 ? "ok" : "over");
    fact(facts, "높이 (1.76 m 기준)", `머리 관절 ${info.headJoint.toFixed(2)} · 턱 ${info.chin?.toFixed(2) ?? "-"} · 목 관절 ${info.neckJoint?.toFixed(2) ?? "-"} m`);
  }
  fact(facts, "뼈", `${candidate.joints}개`);
  const shownClip = el("span", "", "");
  const playingRow = el("dd", "");
  playingRow.append(shownClip);
  facts.append(el("dt", "", "재생 중"), playingRow);
  const crouch = info?.crouchClips ?? [];
  fact(facts, "앉기 클립", crouch.length ? crouch.join(", ") : "없음", crouch.length ? "ok" : "over");
  const list = el("details", "gallery-clips");
  list.append(el("summary", "", `클립 ${candidate.clips?.length ?? 0}개`), el("p", "", (candidate.clips ?? []).map((clip) => `${clip.name.replace(/^.*\|/, "")} ${clip.duration.toFixed(2)}s`).join(" · ")));
  finishCard(root, facts, candidate);
  root.append(list);
  return root;
}

/** The third-person body seen from the game's eye with its head bone shrunk to nothing. */
function armsCard(gallery: Gallery, candidate: Candidate, gltf: GLTF): HTMLElement {
  const { root, facts } = card({ title: `${candidate.title} — 1인칭 팔 (머리 뼈 0배)` }, "arms");
  const canvas = el("canvas");
  root.append(canvas);
  const scene = stageScene(0x8d9794);
  scene.add(ground(6, 12));
  const { holder, model } = standCharacter(candidate, gltf);
  gallery.adopt(model, candidate.uniform);
  scene.add(holder);
  const head = candidate.character ? model.getObjectByName(THREE.PropertyBinding.sanitizeNodeName(candidate.character.headBone)) : undefined;
  const camera = new THREE.PerspectiveCamera(GAME_FOV, 360 / 230, 0.03, 50);
  // Where the model's eyes are, about 10 cm above and ahead of its head bone, looking forward and
  // down 25°. In the game the body would be lowered until these eyes meet the 1.42 m camera.
  const eye = (candidate.character?.headJoint ?? EYE_HEIGHT) + 0.1;
  camera.position.set(0, eye, 0.1);
  camera.lookAt(0, eye - Math.tan(THREE.MathUtils.degToRad(25)) * 2, 2.1);
  const mixer = new THREE.AnimationMixer(model);
  const clip = findClip(gltf.animations, GUN_POSE) ?? findClip(gltf.animations, IDLE);
  if (clip) mixer.clipAction(clip).play();
  const shrink = () => head?.scale.setScalar(0.001);
  gallery.addCell({
    canvas,
    width: 360,
    height: 230,
    scene,
    views: [{ camera, x: 0, width: 360 }],
    tick: (dt) => {
      mixer.update(dt);
      shrink();
    },
  });
  fact(facts, "클립", clip ? clip.name.replace(/^.*\|/, "") : "없음");
  fact(facts, "카메라", `모델의 눈 (${eye.toFixed(2)} m), 시야각 ${GAME_FOV}° · 게임 눈높이는 ${EYE_HEIGHT} m라 몸을 그만큼 내려 맞춤`);
  fact(facts, "총", "4단계에서 손 뼈 소켓에 붙임 (이 미리보기에는 없음)");
  root.append(facts);
  return root;
}

// ------------------------------------------------------------------ guns

const GUN_VIEW = { width: 330, height: 150, metres: 1.32 };

/**
 * Turns a model so its longest axis runs along +X with the muzzle to the right. The muzzle is
 * taken to be the end further from the model's origin, which sits at the grip in most packs;
 * candidates where that is wrong say flip, or give their own turn.
 */
function alongX(fit: Fit, candidate: Candidate): { rotation: THREE.Euler; axis: number } {
  const axis = fit.size.indexOf(Math.max(...fit.size));
  if (candidate.turn) {
    const [x, y, z] = candidate.turn.map((degrees) => THREE.MathUtils.degToRad(degrees));
    return { rotation: new THREE.Euler(x, y, z), axis };
  }
  const muzzleAtMin = Math.abs(fit.min[axis]) > Math.abs(fit.max[axis]) !== (candidate.flip === true);
  const rotation = new THREE.Euler();
  if (axis === 0) rotation.y = muzzleAtMin ? Math.PI : 0;
  else if (axis === 2) rotation.y = muzzleAtMin ? -Math.PI / 2 : Math.PI / 2;
  else rotation.z = muzzleAtMin ? Math.PI / 2 : -Math.PI / 2;
  return { rotation, axis };
}

/** The gun centred on the origin, `length` metres along +X. */
function layGun(candidate: Candidate, gltf: GLTF, animated: boolean): { holder: THREE.Group; model: THREE.Object3D; nativeLength: number } {
  const fit = candidate.fit!;
  const model = animated ? cloneSkinned(gltf.scene) : gltf.scene.clone();
  const centre = [0, 1, 2].map((axis) => (fit.min[axis] + fit.max[axis]) / 2);
  model.position.set(-centre[0], -centre[1], -centre[2]);
  const { rotation, axis } = alongX(fit, candidate);
  const turn = new THREE.Group();
  turn.rotation.copy(rotation);
  turn.add(model);
  const holder = new THREE.Group();
  holder.scale.setScalar((candidate.length ?? 1) / fit.size[axis]);
  holder.add(turn);
  return { holder, model, nativeLength: fit.size[axis] };
}

function gunCard(gallery: Gallery, candidate: Candidate, gltf: GLTF): HTMLElement {
  const { root, facts } = card(candidate, "gun");
  const canvas = el("canvas");
  root.append(canvas);
  const scene = stageScene(0xc9cfcc);
  // A 10 cm grid behind the gun for scale.
  const grid = new THREE.GridHelper(2, 20, 0x9aa19d, 0xb2b8b4);
  grid.rotation.x = Math.PI / 2;
  grid.position.z = -0.4;
  scene.add(grid);
  const animated = (candidate.clips?.length ?? 0) > 0;
  const { holder, model, nativeLength } = layGun(candidate, gltf, animated);
  gallery.adopt(model);
  scene.add(holder);
  const { width, height, metres: span } = GUN_VIEW;
  const camera = new THREE.OrthographicCamera(-span / 2, span / 2, (span * height) / width / 2, -(span * height) / width / 2, 0.1, 10);
  camera.position.set(0, 0, 3);
  let tick: ((dt: number) => void) | undefined;
  const clip = animated ? findClip(gltf.animations, RELOAD) : undefined;
  if (clip) {
    const mixer = new THREE.AnimationMixer(model);
    mixer.clipAction(clip).play();
    tick = (dt) => mixer.update(dt);
  }
  gallery.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }], tick });
  const triangles = candidate.triangles ?? 0;
  fact(facts, "길이", `${(candidate.length ?? 0).toFixed(2)} m (원본 ${nativeLength.toFixed(2)} 단위)`);
  fact(facts, "삼각형", `${number(triangles)} / 예산 ${number(budgets.triangles.gun)}`, triangles > budgets.triangles.gun ? "over" : "ok");
  fact(facts, "draw call", `${candidate.drawCalls}`);
  if (animated) fact(facts, "클립", `${(candidate.clips ?? []).map((item) => item.name.replace(/^.*\|/, "")).join(", ")}${clip ? ` (재생: ${clip.name.replace(/^.*\|/, "")})` : ""}`);
  finishCard(root, facts, candidate);
  return root;
}

// ------------------------------------------------------------------ map pieces

interface MapFit {
  /** Model size in the target's axes, metres after the uniform scale. */
  scaled: number[];
  scale: number;
  /** Per axis, how far the uniformly scaled model misses the target; null for a flat axis. */
  leftover: (number | null)[];
  turned: boolean;
}

/**
 * The single scale that best fits the model to the target box (the geometric middle of the
 * smallest and largest per-axis ratio), after turning it so the longer sides line up. Axes where
 * the target or the model is flat (a shutter or window panel) are left out of the fit.
 */
function fitToTarget(fit: Fit, target: number[]): MapFit {
  const turned = fit.size[0] >= fit.size[2] !== target[0] >= target[2];
  const size = turned ? [fit.size[2], fit.size[1], fit.size[0]] : fit.size;
  const used = [0, 1, 2].filter((axis) => target[axis] >= 0.2 && size[axis] > 1e-3);
  const ratios = used.map((axis) => target[axis] / size[axis]);
  const scale = Math.sqrt(Math.min(...ratios) * Math.max(...ratios));
  return {
    scaled: size.map((value) => value * scale),
    scale,
    leftover: [0, 1, 2].map((axis) => (used.includes(axis) ? (size[axis] * scale) / target[axis] - 1 : null)),
    turned,
  };
}

function mapCard(gallery: Gallery, candidate: Candidate, gltf: GLTF): HTMLElement {
  const { root, facts } = card(candidate, "map");
  const canvas = el("canvas");
  root.append(canvas);
  const scene = stageScene();
  const fit = candidate.fit!;
  const model = gltf.scene.clone();
  gallery.adopt(model);
  const centre = [(fit.min[0] + fit.max[0]) / 2, fit.min[1], (fit.min[2] + fit.max[2]) / 2];
  model.position.set(-centre[0], -centre[1], -centre[2]);
  const turn = new THREE.Group();
  turn.add(model);
  const holder = new THREE.Group();
  holder.add(turn);
  scene.add(holder);

  const target = candidate.target;
  let fitted: MapFit | null = null;
  let extent: number[];
  if (target) {
    fitted = fitToTarget(fit, target);
    if (fitted.turned) turn.rotation.y = Math.PI / 2;
    holder.scale.setScalar(fitted.scale);
    scene.add(wireBox(target[0], 0, target[1], target[2], 0xe7a42c));
    extent = target.map((value, axis) => Math.max(value, fitted!.scaled[axis]));
  } else {
    const scale = candidate.scale ?? 1;
    holder.scale.setScalar(scale);
    extent = fit.size.map((value) => value * scale);
  }
  const reach = Math.max(extent[0], extent[2], 1);
  scene.add(ground(Math.ceil(reach * 1.8), Math.ceil(reach * 1.8)));

  const width = 290;
  const height = 210;
  const camera = new THREE.PerspectiveCamera(32, width / height, 0.05, 400);
  const radius = 0.5 * Math.hypot(extent[0], extent[1], extent[2]);
  const distance = (radius / Math.sin(THREE.MathUtils.degToRad(32 / 2))) * 1.05;
  const direction = new THREE.Vector3(1, 0.62, 1.35).normalize();
  camera.position.copy(direction.multiplyScalar(distance)).add(new THREE.Vector3(0, extent[1] / 2, 0));
  camera.lookAt(0, extent[1] / 2, 0);
  gallery.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }] });

  fact(facts, "용도", MAP_NAMES[candidate.role] ?? candidate.role);
  if (target && fitted) {
    fact(facts, "기준 상자", `${metres(target, 1)} m (길이 × 높이 × 폭)`);
    fact(facts, "원본 크기", `${metres(fitted.turned ? [fit.size[2], fit.size[1], fit.size[0]] : fit.size)} 단위${fitted.turned ? " (90° 돌림)" : ""}`);
    const worst = Math.max(...fitted.leftover.map((value) => Math.abs(value ?? 0)));
    fact(facts, "균일 배율", `×${fitted.scale.toFixed(3)}`);
    fact(
      facts,
      "남는 차이",
      ["길이", "높이", "폭"].map((name, axis) => `${name} ${fitted!.leftover[axis] === null ? "—" : percent(fitted!.leftover[axis]!)}`).join(" · "),
      worst <= 0.05 ? "ok" : "over",
    );
  } else {
    fact(facts, "기준 상자", `없음 (×${candidate.scale ?? 1}, 1 m 격자)`);
    fact(facts, "크기", `${metres(extent)} m`);
  }
  fact(facts, "삼각형", `${number(candidate.triangles ?? 0)} · draw call ${candidate.drawCalls}`);
  finishCard(root, facts, candidate);
  return root;
}

// ------------------------------------------------------------------ surfaces

function surfaceCard(gallery: Gallery, title: string, texture: THREE.Texture, tile: number, detail: string, candidate?: Candidate): HTMLElement {
  const { root, facts } = card({ title, pick: candidate?.pick }, "surface");
  const canvas = el("canvas");
  root.append(canvas);
  const scene = stageScene();
  texture.wrapS = THREE.RepeatWrapping;
  texture.wrapT = THREE.RepeatWrapping;
  // Two repeats across a wall twice the tile wide, as the map boxes tile it in metres.
  const wall = new THREE.Mesh(new THREE.BoxGeometry(tile * 2, tile, 0.4), new THREE.MeshLambertMaterial({ map: texture }));
  const uv = wall.geometry.getAttribute("uv") as THREE.BufferAttribute;
  for (let i = 0; i < uv.count; i++) uv.setX(i, uv.getX(i) * 2);
  wall.position.y = tile / 2;
  scene.add(wall);
  gallery.track(wall);
  scene.add(ground(tile * 3, 6));
  const width = 250;
  const height = 170;
  const camera = new THREE.PerspectiveCamera(36, width / height, 0.1, 100);
  camera.position.set(tile * 0.9, tile * 0.75, tile * 2.0);
  camera.lookAt(0, tile * 0.5, 0);
  gallery.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }] });
  fact(facts, "반복", `${tile} m마다 한 번`);
  fact(facts, "원본", detail);
  if (candidate) finishCard(root, facts, candidate);
  else root.append(facts);
  return root;
}

// ------------------------------------------------------------------ effects

/** Draws a decoded sprite into a small 2D canvas; the loader flipped it for WebGL, so it is flipped back. */
function spriteCard(candidate: Candidate, texture: THREE.Texture): HTMLElement {
  const root = el("figure", "gallery-sprite");
  const canvas = el("canvas");
  canvas.width = 96;
  canvas.height = 96;
  const context = canvas.getContext("2d")!;
  context.fillStyle = "#1b1d18";
  context.fillRect(0, 0, 96, 96);
  context.save();
  context.translate(0, 96);
  context.scale(1, -1);
  context.drawImage(texture.image as CanvasImageSource, 0, 0, 96, 96);
  context.restore();
  root.append(canvas, el("figcaption", "", `${candidate.file.split("/").pop()} · ${FX_NAMES[candidate.role] ?? candidate.role}`));
  return root;
}

interface Particle {
  sprite: THREE.Sprite;
  age: number;
  life: number;
  velocity: THREE.Vector3;
  grow: number;
  fade: number;
}

/** A short loop: a burst from the rifle with flash, tracer and smoke, sparks where it lands. */
function effectsDemo(gallery: Gallery, sprites: Map<string, THREE.Texture>, rifle: { candidate: Candidate; gltf: GLTF } | null): HTMLElement {
  const { root, facts } = card({ title: "총구 화염 · 연기 · 불꽃 데모" }, "fx-demo");
  const canvas = el("canvas");
  root.append(canvas);
  const scene = stageScene(0x55605c);
  scene.add(ground(8, 16));
  const muzzle = new THREE.Vector3(-0.35, 1.2, 0);
  if (rifle) {
    const { holder, model } = layGun(rifle.candidate, rifle.gltf, false);
    gallery.adopt(model);
    holder.position.set(muzzle.x - (rifle.candidate.length ?? 0.84) / 2, muzzle.y - 0.03, 0);
    scene.add(holder);
  }
  const plate = new THREE.Mesh(new THREE.BoxGeometry(0.1, 1.4, 1.4), new THREE.MeshLambertMaterial({ color: 0x77706a }));
  plate.position.set(2.6, 1.0, 0);
  scene.add(plate);
  gallery.track(plate);
  const impact = new THREE.Vector3(2.54, 1.2, 0);

  const material = (name: string, color: number, additive: boolean, opacity = 1) =>
    new THREE.SpriteMaterial({
      map: sprites.get(name) ?? null,
      color,
      transparent: true,
      opacity,
      depthWrite: false,
      blending: additive ? THREE.AdditiveBlending : THREE.NormalBlending,
    });
  const flashes = ["muzzle_01", "muzzle_02", "muzzle_03", "muzzle_04", "muzzle_05"].map((name) => material(name, 0xffd08a, true));
  const flash = new THREE.Sprite(flashes[0]);
  flash.center.set(0.5, 0.15);
  flash.scale.set(0.34, 0.34, 1);
  // The flash sprites point up; turned to point along the barrel.
  flashes.forEach((item) => (item.rotation = -Math.PI / 2));
  flash.position.copy(muzzle).add(new THREE.Vector3(0.02, 0, 0));
  scene.add(flash);
  const tracer = new THREE.Sprite(material("trace_01", 0xffe2a0, true, 0.9));
  tracer.material.rotation = Math.PI / 2;
  tracer.scale.set(0.05, impact.x - muzzle.x, 1);
  tracer.position.set((muzzle.x + impact.x) / 2, muzzle.y, 0);
  scene.add(tracer);

  const live: Particle[] = [];
  const spawn = (name: string, color: number, additive: boolean, at: THREE.Vector3, velocity: THREE.Vector3, size: number, life: number, grow: number, opacity = 1) => {
    const sprite = new THREE.Sprite(material(name, color, additive, opacity));
    sprite.position.copy(at);
    sprite.scale.setScalar(size);
    sprite.material.rotation = Math.random() * Math.PI * 2;
    scene.add(sprite);
    live.push({ sprite, age: 0, life, velocity, grow, fade: opacity });
  };

  const SHOT = 0.1;
  const BURST = 8;
  const CYCLE = 1.6;
  let lastShot = -1;
  const tick = (dt: number, time: number) => {
    const phase = time % CYCLE;
    const shot = Math.floor(phase / SHOT);
    const firing = shot < BURST;
    const shotIndex = Math.floor(time / CYCLE) * BURST + shot;
    const fresh = firing && shotIndex !== lastShot;
    flash.visible = firing && phase % SHOT < 0.06;
    tracer.visible = firing && phase % SHOT < 0.03;
    if (fresh) {
      lastShot = shotIndex;
      flash.material = flashes[shotIndex % flashes.length];
      spawn(["smoke_01", "smoke_04", "smoke_07"][shotIndex % 3], 0xb9b4ab, false, muzzle.clone(), new THREE.Vector3(0.25, 0.35, 0), 0.18, 1.3, 0.75, 0.45);
      spawn("scorch_01", 0xffe0a0, true, impact.clone(), new THREE.Vector3(), 0.3, 0.09, 0.4);
      spawn("dirt_01", 0x8a7f6c, false, impact.clone(), new THREE.Vector3(-0.4, 0.3, 0), 0.25, 0.45, 0.5, 0.8);
      for (let i = 0; i < 7; i++) {
        const velocity = new THREE.Vector3(-1.2 - Math.random() * 1.8, 0.5 + Math.random() * 2.2, (Math.random() - 0.5) * 2.4);
        spawn(i % 2 ? "star_06" : "circle_05", 0xffc46a, true, impact.clone(), velocity, 0.07, 0.35 + Math.random() * 0.2, -0.05);
      }
    }
    for (let i = live.length - 1; i >= 0; i--) {
      const particle = live[i];
      particle.age += dt;
      const t = particle.age / particle.life;
      if (t >= 1) {
        scene.remove(particle.sprite);
        particle.sprite.material.dispose();
        live.splice(i, 1);
        continue;
      }
      particle.velocity.y -= particle.grow < 0 ? 9.8 * dt : 0;
      particle.sprite.position.addScaledVector(particle.velocity, dt);
      particle.sprite.scale.multiplyScalar(1 + Math.max(particle.grow, 0) * dt * 2);
      particle.sprite.material.opacity = particle.fade * (1 - t);
    }
  };
  const width = 560;
  const height = 250;
  const camera = new THREE.PerspectiveCamera(30, width / height, 0.05, 50);
  camera.position.set(1.0, 1.6, 5.4);
  camera.lookAt(1.0, 1.15, 0);
  gallery.addCell({ canvas, width, height, scene, views: [{ camera, x: 0, width }], tick });
  fact(facts, "총구 화염", "muzzle_01–05를 발사마다 바꿔 가산 혼합 (0.1초 간격 8발)");
  fact(facts, "연기", "smoke_01·04·07, 일반 혼합, 1.3초 동안 커지며 사라짐");
  fact(facts, "착탄", "scorch_01 섬광, dirt_01 흙먼지, star_06·circle_05 불꽃 7개(중력)");
  fact(facts, "예광탄", "trace_01을 총구에서 표적까지 늘림");
  root.append(facts);
  return root;
}

// ------------------------------------------------------------------ page

function section(title: string, note: string, id: string): { root: HTMLElement; grid: HTMLElement } {
  const root = el("section", "panel gallery-section");
  root.dataset.section = id;
  const head = el("div", "panel-head");
  head.append(el("h2", "", title), el("span", "muted", note));
  const grid = el("div", "gallery-grid");
  root.append(head, grid);
  return { root, grid };
}

function toolbar(gallery: Gallery): HTMLElement {
  const bar = el("div", "gallery-toolbar panel");
  for (const toggle of TOGGLES) {
    const group = el("div", "gallery-toggle");
    group.append(el("span", "muted", toggle.label));
    const buttons: HTMLButtonElement[] = [];
    for (const [value, label] of toggle.values) {
      const button = el("button", "", label);
      button.type = "button";
      button.dataset.option = toggle.key;
      button.dataset.value = value;
      button.setAttribute("aria-pressed", String(gallery.current[toggle.key] === value));
      button.addEventListener("click", () => {
        gallery.set(toggle.key, value);
        for (const other of buttons) other.setAttribute("aria-pressed", String(other === button));
      });
      buttons.push(button);
      group.append(button);
    }
    bar.append(group);
  }
  return bar;
}

async function settle<T>(items: Candidate[], load: (item: Candidate) => Promise<T>): Promise<Map<Candidate, T | Error>> {
  const results = await Promise.all(
    items.map(async (item): Promise<[Candidate, T | Error]> => {
      try {
        return [item, await load(item)];
      } catch (error) {
        return [item, error instanceof Error ? error : new Error(String(error))];
      }
    }),
  );
  return new Map(results);
}

function failed(candidate: Candidate, error: Error): HTMLElement {
  const root = el("article", "gallery-card gallery-failed");
  root.append(el("h3", "", candidate.title), el("p", "", `불러오지 못함: ${candidate.file} — ${error.message}`));
  return root;
}

/** Adds the candidate gallery to the ?art page; `data-gallery` on it reads "ready" when drawn. */
export async function showGallery(frame: HTMLElement): Promise<void> {
  const host = el("div", "gallery");
  host.dataset.gallery = "loading";
  frame.append(host);
  const candidates = await loadIndex();
  if (!candidates) {
    host.append(el("p", "panel muted", "후보가 없습니다. tools/art에서 fetch.mjs와 stage-candidates.mjs를 실행한 뒤 Vite 개발 서버(npm run dev)로 여세요."));
    host.dataset.gallery = "empty";
    return;
  }

  const gallery = new Gallery();
  host.append(toolbar(gallery));
  const of = (category: Candidate["category"]) => candidates.filter((candidate) => candidate.category === category);
  const models = await settle(
    candidates.filter((candidate) => candidate.file.endsWith(".glb")),
    (candidate) => loadArtModel(candidate.file),
  );
  const images = await settle(
    candidates.filter((candidate) => candidate.file.endsWith(".webp")),
    (candidate) => loadArtTexture(candidate.file),
  );
  const show = <T>(results: Map<Candidate, T | Error>, candidate: Candidate, build: (value: T) => HTMLElement): HTMLElement => {
    const result = results.get(candidate);
    if (result === undefined) return failed(candidate, new Error("not loaded"));
    return result instanceof Error ? failed(candidate, result) : build(result);
  };

  const characters = section("캐릭터", "머리 꼭대기 1.76 m로 맞춤 · 서버 히트박스 와이어프레임", "characters");
  for (const candidate of of("character")) characters.grid.append(show(models, candidate, (gltf) => characterCard(gallery, candidate, gltf)));

  const arms = section("1인칭 팔", "3인칭 캐릭터를 재사용하는 방식: 머리 뼈를 0배로 줄이고 게임 눈높이에서 봄", "arms");
  for (const id of ["swat", "man", "ual1-mannequin"]) {
    const candidate = candidates.find((item) => item.id === id);
    if (candidate) arms.grid.append(show(models, candidate, (gltf) => armsCard(gallery, candidate, gltf)));
  }

  const guns = section("총기", `측면, 모두 같은 축척 (가로 ${GUN_VIEW.metres} m, 격자 10 cm) · 역할별 목표 길이로 맞춤`, "guns");
  const roles = [...new Set(of("gun").map((candidate) => candidate.role))];
  for (const role of Object.keys(WEAPON_NAMES).filter((name) => roles.includes(name))) {
    const group = el("div", "gallery-role");
    group.append(el("h4", "", `${role} — ${WEAPON_NAMES[role]}`));
    const row = el("div", "gallery-grid");
    for (const candidate of of("gun").filter((item) => item.role === role)) row.append(show(models, candidate, (gltf) => gunCard(gallery, candidate, gltf)));
    group.append(row);
    guns.grid.append(group);
  }

  const map = section("맵 키트", "주황 선: 서버 충돌 상자 크기 · 모델은 균일 배율 하나로 맞춤", "map");
  for (const candidate of of("map")) map.grid.append(show(models, candidate, (gltf) => mapCard(gallery, candidate, gltf)));

  const surfaces = section("표면 텍스처", "벽 상자에 지금과 같은 미터 단위로 반복 · 앞 3개는 지금의 코드 텍스처", "surfaces");
  surfaces.grid.append(
    surfaceCard(gallery, "벽돌 (지금, 코드)", tex.brickTexture(), 3, "textures.ts brickTexture()"),
    surfaceCard(gallery, "콘크리트 (지금, 코드)", tex.concreteTexture(), 3, "textures.ts concreteTexture()"),
    surfaceCard(gallery, "지붕 (지금, 코드)", tex.roofTexture(), 4, "textures.ts roofTexture()"),
  );
  for (const candidate of of("texture")) {
    surfaces.grid.append(show(images, candidate, (texture) => surfaceCard(gallery, candidate.title, texture, candidate.tile ?? 3, `${candidate.file.split("/").pop()}`, candidate)));
  }

  const effects = section("이펙트", "Kenney Particle·Splat Pack 스프라이트 (흰색, 코드에서 색을 입힘)", "effects");
  const sprites = new Map<string, THREE.Texture>();
  const sheet = el("div", "gallery-sprites");
  for (const candidate of of("fx")) {
    sheet.append(
      show(images, candidate, (texture) => {
        sprites.set(candidate.file.split("/").pop()!.replace(/\.webp$/, ""), texture);
        return spriteCard(candidate, texture);
      }),
    );
  }
  const rifleCandidate = candidates.find((candidate) => candidate.category === "gun" && candidate.role === "RIFLE" && candidate.pick);
  const rifleModel = rifleCandidate ? models.get(rifleCandidate) : undefined;
  const rifle = rifleCandidate && rifleModel && !(rifleModel instanceof Error) ? { candidate: rifleCandidate, gltf: rifleModel } : null;
  effects.grid.append(effectsDemo(gallery, sprites, rifle), sheet);

  host.append(characters.root, arms.root, guns.root, map.root, surfaces.root, effects.root);
  gallery.start();
  (window as unknown as { __artGallery: { frames: () => number } }).__artGallery = { frames: () => gallery.frameCount() };
  host.dataset.gallery = "ready";
}
