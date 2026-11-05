// A remote player: the rigged soldier from the art manifest with its gun in the right hand, and the
// parts drawn by code (name tag, blob shadow, shield, taser sparks). The lower body plays the
// stride clips an AnimationMixer blends by ground speed and direction, the upper body the weapon's
// hold, and art/rig.ts adds the aim, crouch, hold offsets and head size on top, fitted to the
// server's hitboxes (art/hitbox.ts). Faces −Z at zero yaw, like the camera. Out of the camera's
// view (setCharacterView) the clips and rig wait, and catch up when the model is next in view. If
// the art failed to load, a placeholder figure (art/placeholder.ts) stands in with the same API and
// no animation.
import * as THREE from "three";
import { RoundedBoxGeometry } from "three/examples/jsm/geometries/RoundedBoxGeometry.js";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import { Team, WeaponKind } from "../gen/game_pb";
import { BODY_TOP, CROUCH_DROP, HEAD_BASE, HEAD_HEIGHT, HEAD_SCALE_MAX, SHIELD_FAR, SHIELD_HALF_WIDTH, SHIELD_NEAR, SHIELD_TOP } from "./art/hitbox";
import { loadedArt } from "./art/loader";
import { characterArt, place, teamFile, weaponArt, type CharacterArt, type HoldArt } from "./art/manifest";
import { toLambert } from "./art/materials";
import { placeholderFigure, placeholderGun } from "./art/placeholder";
import { Rig, type Side } from "./art/rig";
import { nameTexture } from "./textures";

/**
 * A name tag's colour: the team's, and the enemy red without a team, since in a free-for-all match
 * every other player is an enemy.
 */
export function teamColor(team: Team): string {
  return team === Team.BLUE ? "#5aa8ff" : "#ff5a4a";
}

/** Seconds one clip takes to blend into the next. */
const FADE = 0.15;
/** Ground speed (m/s) over which standing blends into striding. */
const STRIDE_SPEED: [number, number] = [0.2, 0.7];
/** Ground speed over which a walk blends into a run, standing up; a crouch always runs, lower. */
const WALK_SPEED: [number, number] = [1.6, 2.6];
/** Playback speed limits for the stride clips; past them the feet slide a little. */
const TIME_SCALE: [number, number] = [0.6, 1.4];
/** In a full crouch: the torso's forward lean (radians) and the share of the stride's foot lift kept. */
const CROUCH_LEAN = 0.3;
const CROUCH_LIFT = 0.4;
/** A shot's kick in radians for kickScale 1, the most it builds up to, and how fast it settles. */
const KICK = 0.05;
const KICK_MAX = 0.14;
const KICK_SETTLE = 14;
/** A throw's overarm swing: how long it takes (s) and how far up the arm goes first (radians). */
const THROW_SWING = 0.5;
const THROW_REACH = 0.9;
/** The name tag clears the head top by this much, and never sits lower than NAME_MIN. */
const NAME_CLEARANCE = 0.15;
const NAME_MIN = 2.1;
/** Seconds a fallen body stays before it disappears. */
const BODY_STAYS = 2.6;

type Gait = "walk" | "run" | "runBack" | "strafeL" | "strafeR";
const GAITS: Gait[] = ["walk", "run", "runBack", "strafeL", "strafeR"];

interface GaitClip {
  clip: THREE.AnimationClip;
  /** Metres the planted foot travels in one cycle. */
  length: number;
  /** Where in its cycle this clip plants the right foot, against the run's (0 to 1). */
  offset: number;
}

/** One look's soldier (a team's, or the free-for-all's), prepared once and cloned for every player wearing it. */
interface Template {
  /** The soldier file it was made from (art manifest character.files): which look it is. */
  file: string;
  scene: THREE.Object3D;
  art: CharacterArt;
  idle: THREE.AnimationClip;
  gaits: Map<Gait, GaitClip>;
  /** Upper-body clips by name, one per clip the holds use. */
  holds: Map<string, THREE.AnimationClip>;
  death: THREE.AnimationClip;
  /** The skinned mesh's bounds over every pose it takes, set by the first instance. */
  sphere: THREE.Sphere | null;
  /** Vertices that mostly follow the head, and each foot: what measure() reads. */
  headVertices: number[] | null;
  footVertices: [number[], number[]] | null;
}

const templates = new Map<string, Template | null>();

/** Where the right foot lands in a stride clip, from its own track: its speed while planted and when. */
function stride(clip: THREE.AnimationClip): { speed: number; phase: number } | null {
  const track = clip.tracks.find((item) => item.name === "footR.position");
  if (!track) return null;
  // Every track gets this from its interpolation mode at runtime; the typings leave it out.
  const sample = (track as unknown as { createInterpolant(): THREE.Interpolant }).createInterpolant();
  const steps = 96;
  const points = Array.from({ length: steps }, (_, i) => Array.from(sample.evaluate((clip.duration * i) / steps) as ArrayLike<number>));
  const floor = Math.min(...points.map((point) => point[1]));
  const planted = (point: number[]) => point[1] < floor + 0.02;
  let travel = 0;
  let time = 0;
  let x = 0;
  let y = 0;
  points.forEach((point, i) => {
    if (!planted(point)) return;
    x += Math.cos((2 * Math.PI * i) / steps);
    y += Math.sin((2 * Math.PI * i) / steps);
    const next = points[(i + 1) % steps];
    if (planted(next)) {
      travel += Math.hypot(next[0] - point[0], next[2] - point[2]);
      time += clip.duration / steps;
    }
  });
  if (!time || !travel) return null;
  return { speed: travel / time, phase: (Math.atan2(y, x) / (2 * Math.PI) + 1) % 1 };
}

function prepare(art: CharacterArt, file: string, scene: THREE.Object3D, animations: THREE.AnimationClip[]): Template {
  // Throws, so the team gets the placeholder, if a bone the rig needs is missing.
  new Rig(scene);
  const find = (name: string) => {
    const clip = animations.find((item) => item.name === name);
    if (!clip) throw new Error(`no clip ${name}`);
    return clip;
  };
  // The upper body (chest and up) plays the hold; the rest plays the stride.
  const chest = scene.getObjectByName("chest");
  if (!chest) throw new Error("no chest bone");
  const upper = new Set<string>();
  chest.traverse((object) => upper.add(object.name));
  const part = (clip: THREE.AnimationClip, wantUpper: boolean) =>
    new THREE.AnimationClip(clip.name, clip.duration, clip.tracks.filter((track) => upper.has(THREE.PropertyBinding.parseTrackName(track.name).nodeName) === wantUpper));

  const gaits = new Map<Gait, GaitClip>();
  const run = stride(find(art.clips.run));
  if (!run) throw new Error(`${art.clips.run} has no planted right foot`);
  for (const gait of GAITS) {
    const name = art.clips[gait];
    if (!name) continue;
    const clip = find(name);
    const measured = stride(clip);
    if (!measured) continue;
    gaits.set(gait, { clip: part(clip, false), length: measured.speed * clip.duration, offset: (measured.phase - run.phase + 1) % 1 });
  }
  const holds = new Map<string, THREE.AnimationClip>();
  for (const hold of Object.values(art.holds ?? {})) {
    const name = hold.clip ?? art.clips.idle;
    if (!holds.has(name)) holds.set(name, part(find(name), true));
  }
  if (!holds.has(art.clips.idle)) holds.set(art.clips.idle, part(find(art.clips.idle), true));
  return { file, scene, art, idle: part(find(art.clips.idle), false), gaits, holds, death: find(art.clips.death), sphere: null, headVertices: null, footVertices: null };
}

function template(team: Team): Template | null {
  const art = characterArt();
  const set = loadedArt();
  if (!art || !set) return null;
  const file = teamFile(art.files, team);
  if (templates.has(file)) return templates.get(file)!;
  const gltf = set.models.get(file);
  // A file that failed to load is not remembered: the loader's retry (loadArt) may bring it in.
  if (!gltf) return null;
  let made: Template | null = null;
  try {
    made = prepare(art, file, gltf.scene, gltf.animations);
  } catch (error) {
    console.warn(`[art] ${file}: ${error instanceof Error ? error.message : String(error)}`);
  }
  templates.set(file, made);
  return made;
}

let shared: {
  blob: THREE.BufferGeometry;
  shadow: THREE.Material;
  spark: THREE.BufferGeometry;
  sparkMaterial: THREE.Material;
  shield: THREE.BufferGeometry;
  shieldMaterial: THREE.Material;
} | null = null;

function paint(geometry: THREE.BufferGeometry, hex: number): THREE.BufferGeometry {
  const color = new THREE.Color(hex);
  const count = geometry.getAttribute("position").count;
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(Array.from({ length: count }, () => [color.r, color.g, color.b]).flat(), 3));
  return geometry;
}

/** Geometry and materials every character shares. */
function parts() {
  if (!shared) {
    const blob = new THREE.CircleGeometry(0.42, 16);
    blob.rotateX(-Math.PI / 2);
    blob.translate(0, 0.02, 0);
    // The server's shield box (art/hitbox.ts) as one plate, its vision strip a little proud of
    // both faces, in one draw call.
    const depth = SHIELD_FAR - SHIELD_NEAR;
    const centre = -(SHIELD_NEAR + SHIELD_FAR) / 2;
    const plate = new RoundedBoxGeometry(SHIELD_HALF_WIDTH * 2, SHIELD_TOP, depth, 2, 0.035);
    plate.translate(0, SHIELD_TOP / 2, centre);
    const strip = new THREE.BoxGeometry(0.42, 0.14, depth + 0.012);
    strip.translate(0, 1.42, centre);
    shared = {
      blob,
      shadow: new THREE.MeshBasicMaterial({ color: 0x000000, transparent: true, opacity: 0.32, depthWrite: false }),
      spark: new THREE.BoxGeometry(0.025, 0.42, 0.025),
      sparkMaterial: new THREE.MeshBasicMaterial({ color: 0xa8ecff, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending, depthWrite: false, fog: false }),
      // RoundedBoxGeometry has no index, so neither may the strip have one to merge with it.
      shield: mergeGeometries([paint(plate.index ? plate.toNonIndexed() : plate, 0x3c4348), paint(strip.toNonIndexed(), 0x9fc4d8)])!,
      shieldMaterial: new THREE.MeshLambertMaterial({ vertexColors: true }),
    };
  }
  return shared;
}

/** Frees the shared character parts on the GPU; they stay usable. The match calls it on its way out. */
export function releaseCharacterGpu(): void {
  if (!shared) return;
  for (const item of [shared.blob, shared.spark, shared.shield, shared.shadow, shared.sparkMaterial, shared.shieldMaterial]) item.dispose();
}

/**
 * A gun model, its grip at the origin and its barrel along −Z, and its muzzle node; the
 * placeholder's if the model failed to load. Its geometry and materials are shared: never dispose them.
 */
export function makeGun(kind: WeaponKind): { gun: THREE.Object3D; muzzle: THREE.Object3D } {
  const art = weaponArt(kind);
  const gltf = art && loadedArt()?.models.get(art.file);
  const gun = gltf ? gltf.scene.clone() : placeholderGun();
  if (gltf) toLambert(gun);
  return { gun, muzzle: gun.getObjectByName("muzzle") ?? gun };
}

/** What measure() reports, in world space; the e2e checks it against the hitboxes. */
export interface CharacterMeasure {
  /** The head bone: where the head box starts. */
  neck: number[];
  /** Horizontal distance from the neck to the character's position, the hitboxes' column. */
  neckOffset: number;
  /** The highest point of the head. */
  headTop: number;
  /** The lowest point of the left and of the right foot. */
  feet: number[];
  muzzle: number[];
  /** The grip: where the right fist holds the weapon. */
  grip: number[];
  /** How far the muzzle is ahead of the character's position, along its facing. */
  muzzleAhead: number;
  /** Radians between the barrel and the aim. */
  gunToAim: number;
  /** Metres from the middle of the left fist to the gun's support point; null without one. */
  support: number | null;
  /**
   * How squarely the left palm faces the way its hold needs, as the dot product of its facing with
   * that way: up into the handguard, or (supportGrip) across the grip toward the gun's right, from
   * its left side. Null without a support.
   */
  supportFacing: number | null;
  /**
   * How far each wrist bends from straight (Rig.wristBend), in radians: the right hand on the
   * grip, and the left one where it holds the gun too (a support or a second gun), else null. Both
   * null without the rig.
   */
  wrist: { right: number | null; left: number | null };
  /** Visible meshes and sprites: the draw calls the character costs. */
  parts: number;
}

const _target = new THREE.Vector3();
const _turn = new THREE.Quaternion();
const _bounds = new THREE.Sphere();

/** The view this frame is drawn in, from setCharacterView(); inactive until a camera is given. */
const view = { frustum: new THREE.Frustum(), projection: new THREE.Matrix4(), active: false };

/**
 * The camera this frame is drawn from, given before the characters are updated: update() then
 * leaves the pose of a character whose posed bounds are wholly out of its view as it was, and
 * catches up on the time it missed when the character is next in view. Null poses every character
 * on every update, as the ?art page's benches need.
 */
export function setCharacterView(camera: THREE.Camera | null): void {
  view.active = camera !== null;
  if (!camera) return;
  camera.updateMatrixWorld();
  view.projection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
  view.frustum.setFromProjectionMatrix(view.projection, camera.coordinateSystem, camera.reversedDepth);
}

export class Character {
  readonly root = new THREE.Group();
  /** Turns the model to face −Z; the model hangs below it. */
  private readonly holder = new THREE.Group();
  private readonly model: THREE.Object3D;
  private readonly name: THREE.Sprite;
  private readonly made: Template | null;
  private readonly rig: Rig | null = null;
  private readonly mixer: THREE.AnimationMixer | null = null;
  private readonly mesh: THREE.SkinnedMesh | null = null;
  private readonly idle: THREE.AnimationAction | null = null;
  private readonly gaits = new Map<Gait, { action: THREE.AnimationAction; data: GaitClip }>();
  private readonly holdActions = new Map<string, THREE.AnimationAction>();
  private readonly death: THREE.AnimationAction | null = null;
  /** Current blend weight of every action but death, by action. */
  private readonly weights = new Map<THREE.AnimationAction, number>();
  private deathWeight = 0;
  private phase = 0;
  /** Seconds the mixer is behind, while the model was hidden or out of view. */
  private behind = 0;
  /** The placeholder's gun turns with the aim about the shoulder. */
  private readonly shoulder = new THREE.Group();
  private readonly rightSocket = new THREE.Group();
  private readonly leftSocket = new THREE.Group();
  private guns: THREE.Object3D[] = [];
  private muzzle: THREE.Object3D = this.rightSocket;
  /** Where the left fist closes on the gun, in the gun's frame, and the left hand's turn relative to the gun there. */
  private support: THREE.Vector3 | null = null;
  private supportGrip = false;
  private readonly supportTurn = new THREE.Quaternion();
  /** The middle of each fist in its hand's frame: the weapon's tp.position, mirrored for the left. */
  private readonly fist: Record<Side, THREE.Vector3> = { R: new THREE.Vector3(), L: new THREE.Vector3() };
  private hold: HoldArt = {};
  private weapon = WeaponKind.UNSPECIFIED;
  private deadFor = -1;
  private crouchAmount = 0;
  private headScale = 1;
  private kickAmount = 0;
  /** Seconds of a throw's swing still to play. */
  private throwLeft = 0;
  /** The aim last drawn, for measure(). */
  private pitch = 0;
  private readonly shield: THREE.Mesh;
  private xray = false;
  private readonly sparks = new THREE.Group();
  private stunned = false;
  /** Smoothed ground velocity from the positions update() is given, and the last position. */
  private readonly velocity = new THREE.Vector2();
  private readonly last = new THREE.Vector2();
  private tracked = false;
  /** Each stride clip's target weight this frame. */
  private readonly targets: Record<Gait, number> = { walk: 0, run: 0, runBack: 0, strafeL: 0, strafeR: 0 };

  constructor(displayName: string, team: Team) {
    const s = parts();
    this.made = template(team);
    this.root.add(this.holder);
    if (this.made) {
      this.model = cloneSkinned(this.made.scene);
      toLambert(this.model);
      this.holder.rotation.y = this.made.art.forwardYaw;
      let mesh: THREE.SkinnedMesh | null = null;
      this.model.traverse((object) => {
        if (object instanceof THREE.SkinnedMesh) mesh ??= object;
      });
      this.mesh = mesh;
      this.rig = new Rig(this.model);
      this.rig.bones.handR.add(this.rightSocket);
      this.rig.bones.handL.add(this.leftSocket);
      const mixer = new THREE.AnimationMixer(this.model);
      this.mixer = mixer;
      // Every action plays from the start, mostly at weight 0, so the mixer saves the rest pose
      // as the value an unweighted bone falls back to.
      this.idle = mixer.clipAction(this.made.idle).play();
      this.weights.set(this.idle, 1);
      for (const [gait, data] of this.made.gaits) {
        const action = mixer.clipAction(data.clip).setEffectiveTimeScale(0).setEffectiveWeight(0).play();
        this.gaits.set(gait, { action, data });
        this.weights.set(action, 0);
      }
      for (const [name, clip] of this.made.holds) {
        const action = mixer.clipAction(clip).setEffectiveWeight(0).play();
        this.holdActions.set(name, action);
        this.weights.set(action, 0);
      }
      this.death = mixer.clipAction(this.made.death).setLoop(THREE.LoopOnce, 1);
      this.death.clampWhenFinished = true;
    } else {
      this.model = placeholderFigure(team);
      this.shoulder.position.set(0.2, 1.32, 0);
      this.shoulder.add(this.rightSocket);
      this.rightSocket.position.set(0, -0.05, -0.32);
      this.model.add(this.shoulder);
    }
    this.holder.add(this.model);
    this.setWeapon(WeaponKind.RIFLE);
    // The first hold is on from the start rather than fading in.
    const firstHold = this.made && this.holdActions.get(this.hold.clip ?? this.made.art.clips.idle);
    if (firstHold) this.weights.set(firstHold, 1);
    this.animate(0, 0, true, 0);
    if (this.mesh && this.made) {
      // Bounds that hold every pose (crouch, aim, death, a big head), so it is never culled
      // while some of it is in view.
      this.made.sphere ??= this.posedBounds(this.mesh);
      this.mesh.boundingSphere = this.made.sphere.clone();
    }

    this.shield = new THREE.Mesh(s.shield, s.shieldMaterial);
    this.shield.visible = false;
    this.root.add(this.shield);

    // Arcs flickering around a paralysed soldier.
    for (let i = 0; i < 10; i++) this.sparks.add(new THREE.Mesh(s.spark, s.sparkMaterial));
    this.sparks.visible = false;
    this.root.add(this.sparks);

    // Sun shadows are baked, so moving soldiers get a soft blob instead.
    this.root.add(new THREE.Mesh(s.blob, s.shadow));

    const texture = nameTexture(displayName, teamColor(team));
    this.name = new THREE.Sprite(new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true, sizeAttenuation: false }));
    this.name.scale.set(0.24, 0.045, 1);
    this.name.position.y = NAME_MIN;
    this.name.renderOrder = 10;
    this.root.add(this.name);
  }

  setWeapon(kind: WeaponKind): void {
    if (kind === this.weapon) return;
    this.weapon = kind;
    const art = weaponArt(kind);
    // Shared geometry and materials: removed, never disposed.
    for (const gun of this.guns) gun.removeFromParent();
    const right = makeGun(kind);
    this.rightSocket.add(right.gun);
    this.muzzle = right.muzzle;
    this.guns = [right.gun];
    this.hold = (art && this.made?.art.holds?.[art.hold]) ?? {};
    this.support = this.rig && art?.tp.support ? new THREE.Vector3().fromArray(art.tp.support) : null;
    if (this.rig && art) {
      // A rig's left bones mirror its right ones: x negated, and a turn (x, y, z, w) is (x, −y, −z, w).
      this.fist.R.fromArray(art.tp.position ?? [0, 0, 0]);
      this.fist.L.copy(this.fist.R).setX(-this.fist.R.x);
      this.supportGrip = !!art.tp.supportGrip;
      if (this.supportGrip) {
        // Beside the right fist on the grip: the right hand's hold mirrored. The gun's turn in that
        // hand, inverted, is the hand's turn relative to the gun.
        const grip = new THREE.Quaternion().setFromEuler(new THREE.Euler().fromArray([...(art.tp.rotation ?? [0, 0, 0]), "XYZ"] as [number, number, number, THREE.EulerOrder]));
        this.supportTurn.set(grip.x, -grip.y, -grip.z, grip.w).invert();
      } else {
        this.supportTurn.copy(this.rig.supportTurn);
      }
      place(this.rightSocket, art.tp);
      if (this.hold.mirror) {
        // The second gun: the first one's place in the right hand, mirrored into the left.
        place(this.leftSocket, art.tp);
        const q = this.leftSocket.quaternion;
        this.leftSocket.quaternion.set(q.x, -q.y, -q.z, q.w);
        this.leftSocket.position.x *= -1;
        const left = makeGun(kind);
        this.leftSocket.add(left.gun);
        this.guns.push(left.gun);
      }
    }
    if (this.xray) this.markXray();
  }

  /** Puts the model on the layer the Wallhack pass draws. */
  enableXray(): void {
    this.xray = true;
    this.markXray();
  }

  private markXray(): void {
    for (const group of [this.holder, this.shield]) group.traverse((child) => child.layers.enable(1));
  }

  setLook(headScale: number, shielded: boolean, stunned: boolean): void {
    this.stunned = stunned;
    this.headScale = headScale;
    this.shield.visible = shielded;
  }

  setNameVisible(visible: boolean): void {
    this.name.visible = visible && this.deadFor < 0;
  }

  /**
   * Where the gun's muzzle is now. Off screen the arms keep the last pose drawn (from the start,
   * the standing pose the constructor gives), carried along with the character's place and facing.
   */
  muzzleWorldPosition(target: THREE.Vector3): THREE.Vector3 {
    return this.muzzle.getWorldPosition(target);
  }

  /** The player fired: the upper body kicks back a little, harder for heavier guns. */
  kick(): void {
    const scale = weaponArt(this.weapon)?.fp.kickScale ?? 1;
    this.kickAmount = Math.min(KICK_MAX, this.kickAmount + KICK * scale);
  }

  /** The player threw a grenade: the arm swings up and over, and follows through below the aim. */
  throwGrenade(): void {
    this.throwLeft = THROW_SWING;
  }

  /** The throw's lift of the arm now, radians: up, then over and down past the aim, then back. */
  private throwSwing(): number {
    if (this.throwLeft <= 0) return 0;
    const p = 1 - this.throwLeft / THROW_SWING;
    return THROW_REACH * Math.sin(2 * Math.PI * p) * (1 - p);
  }

  /**
   * `shielded` is spawn protection, which makes the model blink. The stride follows the positions
   * given, not the server's moving flag: that flag runs ahead of the interpolated position, so it
   * would stop the legs while the body still slides to a halt.
   */
  update(dt: number, x: number, z: number, yaw: number, pitch: number, _moving: boolean, alive: boolean, shielded: boolean, time: number, crouching: boolean, y = 0): void {
    this.crouchAmount += (Number(crouching && alive) - this.crouchAmount) * Math.min(1, dt * 14);
    const drop = CROUCH_DROP * this.crouchAmount;
    this.name.position.y = Math.max(NAME_MIN, HEAD_BASE + HEAD_HEIGHT * this.headScale + NAME_CLEARANCE) - drop;
    this.shield.scale.y = 1 - (CROUCH_DROP / SHIELD_TOP) * this.crouchAmount;
    this.track(dt, x, z);
    // `y` lifts the feet off the floor: a box top, or in the air.
    this.root.position.set(x, y, z);
    this.root.rotation.y = yaw;

    this.sparks.visible = this.stunned && alive;
    if (this.sparks.visible) {
      for (const arc of this.sparks.children) {
        arc.visible = Math.random() > 0.3;
        arc.position.set((Math.random() - 0.5) * 0.9, 0.2 + Math.random() * (HEAD_BASE - drop), (Math.random() - 0.5) * 0.9);
        arc.rotation.set(Math.random() * 6.3, Math.random() * 6.3, Math.random() * 6.3);
      }
    }

    if (alive) {
      this.deadFor = -1;
      this.root.visible = !shielded || Math.sin(time * 26) > -0.3;
    } else {
      if (this.deadFor < 0) this.deadFor = 0;
      this.deadFor += dt;
      this.name.visible = false;
      this.root.visible = this.deadFor < BODY_STAYS;
    }
    this.animate(dt, pitch, alive, time, this.inView());
  }

  /**
   * Whether the posed model can be in this frame's view: its bounds over every pose against the
   * view's frustum, as the renderer culls it. Always true without a view or a rigged model.
   */
  private inView(): boolean {
    if (!view.active || !this.mesh?.boundingSphere) return true;
    // The root has just moved; the world matrices still hold the last frame's place.
    this.mesh.updateWorldMatrix(true, false);
    return view.frustum.intersectsSphere(_bounds.copy(this.mesh.boundingSphere).applyMatrix4(this.mesh.matrixWorld));
  }

  /** The ground velocity from successive positions; a jump of metres is a respawn, not a step. */
  private track(dt: number, x: number, z: number): void {
    const dx = x - this.last.x;
    const dz = z - this.last.y;
    const first = !this.tracked;
    this.last.set(x, z);
    this.tracked = true;
    if (first || dt <= 0) return;
    if (Math.hypot(dx, dz) > 2) {
      this.velocity.set(0, 0);
      return;
    }
    const k = Math.min(1, dt * 10);
    this.velocity.x += (dx / dt - this.velocity.x) * k;
    this.velocity.y += (dz / dt - this.velocity.y) * k;
  }

  /** Poses the model for this frame; while it is hidden or `inView` is false it only counts the time. */
  private animate(dt: number, pitch: number, alive: boolean, time: number, inView = true): void {
    this.kickAmount *= Math.exp(-dt * KICK_SETTLE);
    this.throwLeft = Math.max(0, this.throwLeft - dt);
    this.pitch = pitch;
    this.behind += dt;
    if (!this.root.visible || !inView) return;
    const step = this.behind;
    this.behind = 0;
    const shake = this.stunned && alive ? Math.sin(time * 60) * 0.05 : 0;
    if (!this.rig || !this.mixer) {
      this.posePlaceholder(pitch, alive, shake);
      return;
    }

    this.rig.restore();
    this.blend(step, alive);
    this.mixer.update(step);
    this.rig.capture();
    const weight = 1 - this.deathWeight;
    if (weight > 0) {
      this.rig.steady(weight);
      this.rig.addRotations(this.hold.bones, weight);
      if (this.hold.fist) this.rig.closeIndex("R", weight);
      if (this.hold.mirror) this.rig.mirrorArms(weight);
      else if (this.support) this.rig.mirrorFingers(weight);
      this.rig.pose(this.root, {
        pitch: pitch * weight,
        kick: (this.kickAmount + this.throwSwing()) * weight,
        shake,
        lean: CROUCH_LEAN * this.crouchAmount * weight,
        drop: CROUCH_DROP * this.crouchAmount * weight,
        lift: 1 - (1 - CROUCH_LIFT) * this.crouchAmount,
        bothArms: !!this.support || !!this.hold.mirror,
      });
      if (this.support && alive) {
        const turn = this.rightSocket.getWorldQuaternion(_turn).multiply(this.supportTurn);
        this.rig.support(this.rightSocket.localToWorld(_target.copy(this.support)), turn, this.fist.L, this.root);
      }
    }
    this.rig.setHeadScale(this.headScale);
  }

  /** The clips' weights and times: idle and the strides by speed and direction, the hold, death. */
  private blend(dt: number, alive: boolean): void {
    const death = this.death!;
    if (alive) {
      if (this.deathWeight > 0 || death.isRunning()) {
        // Respawned: standing again at once.
        death.stop();
        this.deathWeight = 0;
      }
    } else {
      if (!death.isRunning() && this.deathWeight === 0) death.reset().play();
      this.deathWeight = Math.min(1, this.deathWeight + dt / FADE);
    }
    death.setEffectiveWeight(this.deathWeight);

    // Speed and direction in the character's frame, where it faces −Z.
    const yaw = this.root.rotation.y;
    const speed = alive ? this.velocity.length() : 0;
    const forward = -this.velocity.x * Math.sin(yaw) - this.velocity.y * Math.cos(yaw);
    const right = this.velocity.x * Math.cos(yaw) - this.velocity.y * Math.sin(yaw);
    const stride = THREE.MathUtils.smoothstep(speed, STRIDE_SPEED[0], STRIDE_SPEED[1]);
    // Each direction's share of the stride, by how much of the motion goes that way.
    const across = Math.abs(forward) + Math.abs(right) || 1;
    const walk = (1 - this.crouchAmount) * (1 - THREE.MathUtils.smoothstep(speed, WALK_SPEED[0], WALK_SPEED[1]));
    const want = this.targets;
    want.walk = (stride * Math.max(0, forward) * walk) / across;
    want.run = (stride * Math.max(0, forward) * (1 - walk)) / across;
    want.runBack = (stride * Math.max(0, -forward)) / across;
    want.strafeR = (stride * Math.max(0, right)) / across;
    want.strafeL = (stride * Math.max(0, -right)) / across;
    // A clip the rig lacks hands its share to the run.
    for (const gait of GAITS) {
      if (gait !== "run" && !this.gaits.has(gait)) {
        want.run += want[gait];
        want[gait] = 0;
      }
    }

    // Every weight eases toward its target, the stride and idle ones kept summing to one.
    const idle = this.idle!;
    let total = this.ease(idle, 1 - stride, dt);
    for (const [gait, { action }] of this.gaits) total += this.ease(action, want[gait], dt);
    total ||= 1;
    const live = 1 - this.deathWeight;
    idle.setEffectiveWeight((this.weights.get(idle)! / total) * live);

    // The strides share one phase, each offset so its feet land with the run's, and its rate
    // follows the ground speed over the blended stride length.
    let length = 0;
    let duration = 0;
    let striding = 0;
    for (const { action, data } of this.gaits.values()) {
      const weight = this.weights.get(action)! / total;
      action.setEffectiveWeight(weight * live);
      length += weight * data.length;
      duration += weight * data.clip.duration;
      striding += weight;
    }
    if (striding > 1e-4) {
      length /= striding;
      duration /= striding;
      const rate = THREE.MathUtils.clamp(speed / length, TIME_SCALE[0] / duration, TIME_SCALE[1] / duration);
      this.phase = (this.phase + rate * dt) % 1;
    }
    for (const { action, data } of this.gaits.values()) action.time = ((this.phase + data.offset) % 1) * data.clip.duration;

    const holdClip = this.hold.clip ?? this.made!.art.clips.idle;
    for (const [name, action] of this.holdActions) action.setEffectiveWeight(this.ease(action, name === holdClip ? 1 : 0, dt) * live);
  }

  /** Moves an action's weight toward `target` at the fade rate; returns the new weight. */
  private ease(action: THREE.AnimationAction, target: number, dt: number): number {
    const now = this.weights.get(action)!;
    const next = now + THREE.MathUtils.clamp(target - now, -dt / FADE, dt / FADE);
    this.weights.set(action, next);
    return next;
  }

  /** The stand-in figure: the gun turns with the aim, a crouch shortens it and death lays it down. */
  private posePlaceholder(pitch: number, alive: boolean, shake: number): void {
    const drop = CROUCH_DROP * this.crouchAmount;
    this.shoulder.rotation.set(pitch, 0, shake);
    this.shoulder.position.y = 1.32 - drop;
    const body = this.model.getObjectByName("body");
    if (body) body.scale.y = 1 - drop / BODY_TOP;
    const head = this.model.getObjectByName("head");
    if (head) {
      head.position.y = HEAD_BASE - drop;
      head.scale.setScalar(this.headScale);
    }
    const fall = alive ? 0 : Math.min(this.deadFor / 0.35, 1);
    this.model.rotation.x = (fall * fall * Math.PI) / 2;
  }

  /**
   * The skinned mesh's bounds over the poses that reach furthest (aiming up standing, aiming down
   * crouched, lying dead, all with the biggest head Big Head grows), a tenth larger, in the mesh's own frame.
   */
  private posedBounds(mesh: THREE.SkinnedMesh): THREE.Sphere {
    const sphere = new THREE.Sphere();
    const take = () => {
      this.root.updateMatrixWorld(true);
      mesh.computeBoundingSphere();
      if (sphere.isEmpty()) sphere.copy(mesh.boundingSphere!);
      else sphere.union(mesh.boundingSphere!);
    };
    const look = this.headScale;
    this.headScale = HEAD_SCALE_MAX;
    for (const [crouch, pitch] of [[0, 1.45], [1, -1.45]]) {
      this.crouchAmount = crouch;
      this.animate(0, pitch, true, 0);
      take();
    }
    this.crouchAmount = 0;
    this.rig!.restore();
    this.blend(FADE, false);
    this.mixer!.update(this.made!.death.duration + 0.1);
    this.rig!.capture();
    this.rig!.setHeadScale(HEAD_SCALE_MAX);
    take();
    // Standing again: animate() restores the clip pose, stops the death clip and poses afresh.
    this.headScale = look;
    this.animate(0, 0, true, 0);
    sphere.radius *= 1.1;
    return sphere;
  }

  /** Vertices that mostly follow the head bone, and each foot bone; found once per team. */
  private vertexGroups(mesh: THREE.SkinnedMesh, made: Template, rig: Rig): { head: number[]; feet: [number[], number[]] } {
    if (!made.headVertices || !made.footVertices) {
      const bones = mesh.skeleton.bones;
      const head = bones.indexOf(rig.bones.head as THREE.Bone);
      const feet = [bones.indexOf(rig.bones.footL as THREE.Bone), bones.indexOf(rig.bones.footR as THREE.Bone)];
      const index = mesh.geometry.getAttribute("skinIndex");
      const weight = mesh.geometry.getAttribute("skinWeight");
      made.headVertices = [];
      made.footVertices = [[], []];
      for (let i = 0; i < index.count; i++) {
        let best = 0;
        for (let k = 1; k < 4; k++) if (weight.getComponent(i, k) > weight.getComponent(i, best)) best = k;
        const bone = index.getComponent(i, best);
        if (bone === head) made.headVertices.push(i);
        else if (bone === feet[0]) made.footVertices[0].push(i);
        else if (bone === feet[1]) made.footVertices[1].push(i);
      }
    }
    return { head: made.headVertices, feet: made.footVertices };
  }

  /** Where the parts that the hitboxes judge are, for the ?art bench and e2e/art.mjs. */
  measure(): CharacterMeasure {
    this.root.updateMatrixWorld(true);
    const at = new THREE.Vector3(this.root.position.x, 0, this.root.position.z);
    const facing = new THREE.Vector3(-Math.sin(this.root.rotation.y), 0, -Math.cos(this.root.rotation.y));
    const neck = this.rig ? this.rig.bones.head.getWorldPosition(new THREE.Vector3()) : new THREE.Vector3(at.x, HEAD_BASE - CROUCH_DROP * this.crouchAmount, at.z);
    const muzzle = this.muzzle.getWorldPosition(new THREE.Vector3());
    const grip = this.rightSocket.getWorldPosition(new THREE.Vector3());
    const barrel = this.rightSocket.localToWorld(new THREE.Vector3(0, 0, -1)).sub(grip).normalize();
    const aim = facing.clone().multiplyScalar(Math.cos(this.pitch)).setY(Math.sin(this.pitch));
    let headTop = HEAD_BASE + HEAD_HEIGHT * this.headScale - CROUCH_DROP * this.crouchAmount;
    let feet = [0, 0];
    if (this.mesh && this.made && this.rig) {
      const { head, feet: soles } = this.vertexGroups(this.mesh, this.made, this.rig);
      const heights = (list: number[]) => list.map((i) => this.mesh!.localToWorld(this.mesh!.getVertexPosition(i, new THREE.Vector3())).y);
      headTop = Math.max(...heights(head));
      feet = soles.map((list) => Math.min(...heights(list)));
    }
    let support: number | null = null;
    let supportFacing: number | null = null;
    if (this.support && this.rig) {
      const fist = this.rig.bones.handL.localToWorld(this.fist.L.clone());
      support = fist.distanceTo(this.rightSocket.localToWorld(this.support.clone()));
      const wanted = new THREE.Vector3(this.supportGrip ? 1 : 0, this.supportGrip ? 0 : 1, 0).applyQuaternion(this.rightSocket.getWorldQuaternion(new THREE.Quaternion()));
      supportFacing = this.rig.palmFacing("L").dot(wanted);
    }
    const wrist: CharacterMeasure["wrist"] = { right: null, left: null };
    if (this.rig) {
      wrist.right = this.rig.wristBend("R");
      if (this.support || this.hold.mirror) wrist.left = this.rig.wristBend("L");
    }
    let count = 0;
    this.root.traverseVisible((object) => {
      if (object instanceof THREE.Mesh || object instanceof THREE.Sprite || object instanceof THREE.Line || object instanceof THREE.Points) count++;
    });
    return {
      neck: neck.toArray(),
      neckOffset: Math.hypot(neck.x - at.x, neck.z - at.z),
      headTop,
      feet,
      muzzle: muzzle.toArray(),
      grip: grip.toArray(),
      muzzleAhead: muzzle.clone().sub(at).dot(facing),
      gunToAim: barrel.angleTo(aim),
      support,
      supportFacing,
      wrist,
      parts: count,
    };
  }

  dispose(): void {
    this.root.removeFromParent();
    if (this.mixer) {
      this.mixer.stopAllAction();
      this.mixer.uncacheRoot(this.model);
    }
    // The bone texture is this instance's own; the geometry and materials are the template's.
    this.mesh?.skeleton.dispose();
    (this.name.material.map as THREE.Texture).dispose();
    this.name.material.dispose();
  }
}
