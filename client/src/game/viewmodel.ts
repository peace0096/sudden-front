// The first-person view: the local player's arms and gun, drawn in a scene of their own with a
// camera that never moves, over the world (game.ts clears the depth first, and skips it while
// scoped). The arms are the third-person soldier of the player's team (art manifest `arms`), of
// which only the arm triangles are drawn: those skinned to the clavicles and the bones below, so
// no head, torso or legs can come into view. The gun sits in the rig where its manifest `fp` puts
// it; the right hand holds the grip as `tp` places a gun in the palm and the left wrist goes to
// the support point, both by two-bone IK (art/rig.ts) from shoulders kept below the view. Akimbo
// puts a second gun, mirrored, in the left hand; a one-handed gun folds the left arm away.
// The scripted motion moves the whole rig, arms and gun together, so the hands stay on the gun:
// the kick, the flash at the muzzle node, the reload dip with the left hand dropping away to swap
// the magazine, the bolt or pump cycle (the left hand racking a pump), the knife slash, the
// walking bob and the lowering on death. The flash is a muzzle cell of the effect atlas, drawn
// over the gun as the game's other effects are drawn (particles.ts), its flame pointing in toward
// the middle of the view where the barrel's line runs. A gun whose model failed to load is
// placeholder.ts's box; without the arms' model the gun shows alone.
import * as THREE from "three";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import { Team, WeaponKind } from "../gen/game_pb";
import { loadedArt } from "./art/loader";
import { armsArt, characterArt, place, weaponArt, type WeaponArt } from "./art/manifest";
import { toLambert } from "./art/materials";
import { Rig, type Side } from "./art/rig";
import { makeGun } from "./character";
import { ParticleBatch, particleAtlas, particleSpec, resetSpec, type UvRect } from "./particles";

const RIG_SCALE = 0.82;
/**
 * Where each shoulder sits in the rig, from its wrist's resting place: down, back and out to its
 * own side, at this share of the arm's length, so the elbow keeps a bend and the arm comes into
 * view from below. Each elbow bends toward its POLE from the shoulder: down and out.
 */
const SHOULDER: Record<Side, THREE.Vector3> = { R: new THREE.Vector3(0.25, -0.6, 0.75).normalize(), L: new THREE.Vector3(-0.65, -0.6, 0.45).normalize() };
const SHOULDER_REACH = 0.85;
const POLE: Record<Side, THREE.Vector3> = { R: new THREE.Vector3(0.5, -0.5, 0), L: new THREE.Vector3(-0.6, -0.4, 0) };
/** Where the left hand goes during a reload, from the gun, in the rig: down and back for a magazine. */
const RELOAD_REACH = new THREE.Vector3(-0.04, -0.16, 0.12);
/**
 * The left hand under a handguard, in the gun's frame: the palm faces up into it, its length
 * running across to the gun's right where the fingers close around it, the index finger forward.
 */
const SUPPORT_FINGERS = new THREE.Vector3(0.85, -0.45, -0.25).normalize();
const SUPPORT_INDEX = new THREE.Vector3(0, 0, -1);
/** The flash's colour (linear, above 1 for its glare) and how long it shows, s. */
const FLASH_COLOUR = new THREE.Color(0xffa64d).multiplyScalar(1.6);
const FLASH_LIFE = 0.045;

/** The soldier's arms in one team's colour. */
interface Arms {
  team: Team;
  model: THREE.Object3D;
  mesh: THREE.SkinnedMesh;
  rig: Rig;
  /** The left hand's turn on a handguard, relative to the gun. */
  supportTurn: THREE.Quaternion;
}

/** The gun in hand, measured in the rig when it was set. */
interface Held {
  art: WeaponArt | undefined;
  /** The right hand's gun, then the left hand's for akimbo. */
  guns: THREE.Object3D[];
  /** Each gun's muzzle, where its flash shows. */
  muzzles: THREE.Vector3[];
  /** Where the left wrist rests on the gun, for a two-handed gun. */
  support: THREE.Vector3 | null;
  /** The barrel's backward direction, along which a pump racks. */
  back: THREE.Vector3;
  /** The grip's place and turn in the right palm (tp), each hand mirroring them for its own side. */
  palm: Record<Side, THREE.Vector3>;
  turn: Record<Side, THREE.Quaternion>;
  /** Each shoulder's place for this gun; the left one is null while that arm is folded away. */
  shoulders: Record<Side, THREE.Vector3 | null>;
}

/** What measure() reports, for the ?art bench and e2e/art.mjs. Distances are in the view's metres. */
export interface ViewModelMeasure {
  /** From the right palm to the grip of the gun it holds; null without arms. */
  right: number | null;
  /** From the left hand to where it should be: the support point (moved by a reload or pump) or the second gun's grip; null when it is off the gun. */
  left: number | null;
  /** Each muzzle in the camera's normalised device coordinates: on screen within ±1. */
  muzzles: number[][];
  /** Triangles the arms draw. */
  armTriangles: number;
}

/** Teams whose arms could not be built from their model: told once a page, then shown without arms. */
const brokenArms = new Set<Team>();

/** A team's soldier model, from the art manifest's arms files; null if it did not load. */
function armsTemplate(team: Team): THREE.Object3D | null {
  const art = armsArt();
  const file = art && (team === Team.RED ? art.files.red : art.files.blue);
  return (file && loadedArt()?.models.get(file)?.scene) || null;
}

/**
 * The arms alone: a copy of the soldier's geometry with only the triangles whose corners all follow
 * mostly a bone from a clavicle down. It is the view model's own, so disposing it leaves the
 * shared character's buffers alone.
 */
function armsGeometry(mesh: THREE.SkinnedMesh, rig: Rig): THREE.BufferGeometry {
  const arms = new Set<THREE.Object3D>();
  for (const side of ["L", "R"] as const) rig.bones[`clavicle${side}`].traverse((bone) => arms.add(bone));
  const source = mesh.geometry;
  const skinIndex = source.getAttribute("skinIndex");
  const skinWeight = source.getAttribute("skinWeight");
  const bones = mesh.skeleton.bones;
  const inArm = (vertex: number) => {
    let best = 0;
    for (let k = 1; k < 4; k++) if (skinWeight.getComponent(vertex, k) > skinWeight.getComponent(vertex, best)) best = k;
    return arms.has(bones[skinIndex.getComponent(vertex, best)]);
  };
  const corners = source.index ? (i: number) => source.index!.getX(i) : (i: number) => i;
  const count = source.index ? source.index.count : source.getAttribute("position").count;
  const kept: number[] = [];
  const renumbered = new Map<number, number>();
  const triangles: number[] = [];
  for (let i = 0; i < count; i += 3) {
    const triangle = [corners(i), corners(i + 1), corners(i + 2)];
    if (!triangle.every(inArm)) continue;
    for (const vertex of triangle) {
      if (!renumbered.has(vertex)) renumbered.set(vertex, kept.push(vertex) - 1);
      triangles.push(renumbered.get(vertex)!);
    }
  }
  const geometry = new THREE.BufferGeometry();
  for (const [name, attribute] of Object.entries(source.attributes)) {
    const size = attribute.itemSize;
    const values = new Float32Array(kept.length * size);
    kept.forEach((vertex, i) => {
      for (let k = 0; k < size; k++) values[i * size + k] = attribute.getComponent(vertex, k);
    });
    geometry.setAttribute(name, new THREE.BufferAttribute(values, size));
  }
  geometry.setIndex(triangles);
  return geometry;
}

/**
 * A hand's frame from its own bones: along the palm to the middle finger, then across it toward
 * the index finger, then square to both.
 */
function handBasis(rig: Rig, side: Side, basis: THREE.Matrix4): THREE.Matrix4 {
  const bone = (name: string) => rig.bones[`hand${side}`].getObjectByName(`${name}${side}1`)!.position;
  return basis3(bone("middle").clone(), bone("index").clone().sub(bone("pinky")), basis);
}

/** An orthonormal basis whose first axis is `along` and whose second lies toward `across`. */
function basis3(along: THREE.Vector3, across: THREE.Vector3, basis: THREE.Matrix4): THREE.Matrix4 {
  along.normalize();
  across.addScaledVector(along, -across.dot(along)).normalize();
  return basis.makeBasis(along, across, new THREE.Vector3().crossVectors(along, across));
}

/**
 * A team's arms, or null without their model. A model they can't be built from is told once and
 * remembered (brokenArms), and leaves nothing of its own behind: the arms' geometry is made last.
 */
function buildArms(team: Team): Arms | null {
  const template = armsTemplate(team);
  if (!template) return null;
  const model = cloneSkinned(template);
  toLambert(model);
  try {
    const rig = new Rig(model);
    let mesh: THREE.SkinnedMesh | null = null;
    model.traverse((object) => {
      if (object instanceof THREE.SkinnedMesh) mesh ??= object;
    });
    if (!mesh) throw new Error("no skinned mesh");
    const skinned: THREE.SkinnedMesh = mesh;
    // The hand frame that lines its palm up with SUPPORT_*: wanted · own⁻¹ (orthonormal, so transposed).
    const own = handBasis(rig, "L", new THREE.Matrix4()).transpose();
    const wanted = basis3(SUPPORT_FINGERS.clone(), SUPPORT_INDEX.clone(), new THREE.Matrix4());
    const supportTurn = new THREE.Quaternion().setFromRotationMatrix(wanted.multiply(own));
    skinned.geometry = armsGeometry(skinned, rig);
    // The pose decides what is in view, not the bounds of the rest pose.
    skinned.frustumCulled = false;
    return { team, model, mesh: skinned, rig, supportTurn };
  } catch (error) {
    brokenArms.add(team);
    console.warn(`[art] first-person arms: ${error instanceof Error ? error.message : String(error)}`);
    return null;
  }
}

/** Sets the bones a clip moves to their values at its first frame. */
function poseAtStart(model: THREE.Object3D, clip: THREE.AnimationClip): void {
  for (const track of clip.tracks) {
    const { nodeName, propertyName } = THREE.PropertyBinding.parseTrackName(track.name);
    const node = model.getObjectByName(nodeName);
    if (node && (propertyName === "position" || propertyName === "quaternion" || propertyName === "scale")) node[propertyName].fromArray(track.values);
  }
}

const scratch = {
  a: new THREE.Vector3(),
  b: new THREE.Vector3(),
  c: new THREE.Vector3(),
  wrist: new THREE.Vector3(),
  pole: new THREE.Vector3(),
  turn: new THREE.Quaternion(),
  hand: new THREE.Quaternion(),
};

export class ViewModel {
  readonly scene = new THREE.Scene();
  readonly camera = new THREE.PerspectiveCamera(50, 1, 0.01, 10);
  private readonly rig = new THREE.Group();
  /** Holds the arms' model, turned to face −Z and moved so the right shoulder lands where the gun needs it. */
  private readonly armsRoot = new THREE.Group();
  private arms: Arms | null = null;
  private held: Held = {
    art: undefined,
    guns: [],
    muzzles: [],
    support: null,
    back: new THREE.Vector3(0, 0, 1),
    palm: { R: new THREE.Vector3(), L: new THREE.Vector3() },
    turn: { R: new THREE.Quaternion(), L: new THREE.Quaternion() },
    shoulders: { R: null, L: null },
  };
  private readonly flash: ParticleBatch;
  private readonly flashCells: UvRect[];
  private readonly flashSpec = particleSpec();
  private readonly shield: THREE.Group;
  private kind = WeaponKind.UNSPECIFIED;
  private altMuzzle = false;
  private kick = 0;
  private swing = 0;
  private bob = 0;
  private lowered = 0;
  /** The left hand's offsets this frame, as fractions of the reload reach and the pump stroke. */
  private reloadOut = 0;
  private cycle = 0;
  /** Where the left wrist was sent last, in the world, for measure(), while it holds the support. */
  private readonly leftTarget = new THREE.Vector3();
  private leftOnSupport = false;

  constructor() {
    // The world's outdoor sky and sun (world.ts), on the same Lambert models as the soldiers in it.
    // The sun comes from above and behind the left shoulder, onto the sides of the gun in view.
    this.scene.add(new THREE.HemisphereLight(0xfff1dc, 0xb09c7c, 2.1));
    const sun = new THREE.DirectionalLight(0xfff0d8, 2.2);
    sun.position.set(-1, 2, 1);
    this.scene.add(sun);

    const atlas = particleAtlas();
    this.flash = new ParticleBatch(atlas, { capacity: 2, additive: true, overlay: true, renderOrder: 5 });
    this.flashCells = ["muzzle1", "muzzle2", "muzzle3"].map((name) => atlas.cell(name));

    // The shield bearer's view past the edge of the shield.
    const plate = new THREE.Mesh(new THREE.BoxGeometry(0.62, 0.72, 0.03), new THREE.MeshLambertMaterial({ color: 0x3c4348, transparent: true, opacity: 0.82 }));
    const strip = new THREE.Mesh(new THREE.BoxGeometry(0.26, 0.07, 0.032), new THREE.MeshLambertMaterial({ color: 0x9fc4d8 }));
    strip.position.y = 0.2;
    this.shield = new THREE.Group();
    this.shield.add(plate, strip);
    this.shield.position.set(-0.3, -0.34, -0.62);
    this.shield.rotation.y = 0.25;
    this.shield.visible = false;
    this.scene.add(this.shield);

    this.armsRoot.rotation.y = characterArt()?.forwardYaw ?? 0;
    this.armsRoot.scale.setScalar(armsArt()?.scale ?? 1);
    this.rig.add(this.armsRoot, this.flash.mesh);
    this.rig.scale.setScalar(RIG_SCALE);
    this.scene.add(this.rig);
    // The arms wait for setTeam, which the game calls before its first frame; the gun shows alone until then.
    this.setWeapon(WeaponKind.RIFLE);
  }

  /** The arms in the local player's team colour. */
  setTeam(team: Team): void {
    if (this.arms?.team === team || (!this.arms && brokenArms.has(team))) return;
    this.dropArms();
    this.arms = brokenArms.has(team) ? null : buildArms(team);
    if (this.arms) this.armsRoot.add(this.arms.model);
    this.fit();
  }

  setWeapon(kind: WeaponKind): void {
    if (kind === this.kind) return;
    this.kind = kind;
    const art = weaponArt(kind);
    // Shared geometry and materials: removed, never disposed.
    for (const gun of this.held.guns) gun.removeFromParent();
    const right = makeGun(kind);
    const guns = [right.gun];
    const muzzles = [right.muzzle];
    if (art) place(right.gun, art.fp);
    if (art?.fp.handX !== undefined) {
      // The second gun: the first one's place mirrored across the middle between the two grips.
      const left = makeGun(kind);
      place(left.gun, art.fp);
      const q = left.gun.quaternion;
      q.set(q.x, -q.y, -q.z, q.w);
      left.gun.position.x = art.fp.handX - left.gun.position.x;
      guns.push(left.gun);
      muzzles.push(left.muzzle);
    }
    this.rig.add(...guns);
    this.rig.updateMatrixWorld(true);
    const inRig = (point: THREE.Vector3) => this.rig.worldToLocal(point);
    const support = art?.fp.support ?? art?.tp.support;
    const palm = new THREE.Vector3().fromArray(art?.tp.position ?? [0, 0, 0]);
    const turn = new THREE.Quaternion().setFromEuler(new THREE.Euler().fromArray([...(art?.tp.rotation ?? [0, 0, 0]), "XYZ"] as [number, number, number, THREE.EulerOrder]));
    this.held = {
      art,
      guns,
      muzzles: muzzles.map((muzzle) => inRig(muzzle.getWorldPosition(new THREE.Vector3()))),
      support: art && !art.fp.oneHanded && guns.length === 1 && support ? new THREE.Vector3().fromArray(support).applyMatrix4(right.gun.matrix) : null,
      back: new THREE.Vector3(0, 0, 1).applyQuaternion(right.gun.quaternion),
      // A rig's left bones mirror its right ones: x negated, and a turn (x, y, z, w) is (x, −y, −z, w).
      palm: { R: palm, L: palm.clone().setX(-palm.x) },
      turn: { R: turn, L: new THREE.Quaternion(turn.x, -turn.y, -turn.z, turn.w) },
      shoulders: { R: null, L: null },
    };
    this.fit();
  }

  setShield(on: boolean): void {
    this.shield.visible = on;
  }

  setAspect(aspect: number): void {
    this.camera.aspect = aspect;
    this.camera.updateProjectionMatrix();
  }

  fire(): void {
    if (this.kind === WeaponKind.KNIFE) {
      this.swing = 1;
      return;
    }
    this.kick = 1;
    this.altMuzzle = !this.altMuzzle;
    const muzzles = this.held.muzzles;
    if (!muzzles.length) return;
    const muzzle = muzzles[this.altMuzzle ? muzzles.length - 1 : 0];
    // The camera sits at the origin looking down −Z, so the view's middle is straight in from the
    // muzzle's x and y; that way is brought into the rig, which holds the flash.
    this.rig.updateMatrixWorld();
    const seen = this.rig.localToWorld(scratch.a.copy(muzzle));
    const inward = scratch.b.set(-seen.x, -seen.y, 0);
    if (inward.lengthSq() < 1e-8) inward.set(0, 1, 0);
    inward.applyQuaternion(this.rig.getWorldQuaternion(scratch.turn).invert()).normalize();
    const size = (0.2 + Math.random() * 0.12) * (this.held.art?.fp.flashScale ?? 1);
    const cell = this.flashCells[Math.floor(Math.random() * this.flashCells.length) % this.flashCells.length];
    const p = resetSpec(this.flashSpec, cell, muzzle.x, muzzle.y, muzzle.z, FLASH_LIFE);
    // Its root at the muzzle, its flame reaching in.
    p.size = p.endSize = size * RIG_SCALE * 1.1;
    p.length = size * 1.2;
    p.ax = inward.x;
    p.ay = inward.y;
    p.az = inward.z;
    p.r = p.endR = FLASH_COLOUR.r;
    p.g = p.endG = FLASH_COLOUR.g;
    p.b = p.endB = FLASH_COLOUR.b;
    this.flash.spawn(p);
  }

  /** Where the muzzle that fired last is in the view, in normalised device coordinates. */
  muzzleNdc(target: THREE.Vector3): THREE.Vector3 {
    const muzzles = this.held.muzzles;
    if (!muzzles.length) return target.set(0, 0, 0);
    this.rig.updateMatrixWorld();
    return this.rig.localToWorld(target.copy(muzzles[this.altMuzzle ? muzzles.length - 1 : 0])).project(this.camera);
  }

  /**
   * @param reloadFraction progress of the reload, 0 when not reloading
   * @param boltFraction 1 just after a bolt-action or pump shot, falling to 0 when ready
   */
  update(dt: number, moving: boolean, reloadFraction: number, boltFraction: number, alive: boolean): void {
    this.kick = Math.max(0, this.kick - dt * 11);
    this.swing = Math.max(0, this.swing - dt * 3.4);
    if (moving) this.bob += dt * 9.5;
    this.flash.update(dt);
    const targetLow = alive ? 0 : 1;
    this.lowered += (targetLow - this.lowered) * Math.min(1, dt * 8);

    const bobX = moving ? Math.sin(this.bob) * 0.006 : 0;
    const bobY = moving ? Math.abs(Math.cos(this.bob)) * 0.007 : 0;

    // Reload: dip the gun and tilt it while the left hand drops away for a fresh magazine.
    const r = reloadFraction;
    const dip = r > 0 ? Math.sin(Math.min(r, 1) * Math.PI) : 0;
    this.reloadOut = r > 0 ? Math.max(0, Math.sin(Math.min(r / 0.8, 1) * Math.PI)) : 0;

    // Bolt or pump cycle: cant the gun after the recoil settles; a pump racks under the left hand.
    this.cycle = boltFraction > 0 ? Math.sin((1 - boltFraction) * Math.PI) : 0;
    const cycle = this.cycle;

    // Knife: a slash across from the right, recovering to the guard.
    const slash = this.swing > 0 ? Math.sin((1 - this.swing) * Math.PI) : 0;

    const fp = this.held.art?.fp;
    const kick = this.kick * (fp?.kickScale ?? 1);
    const [offsetX, offsetY, offsetZ] = fp?.rigOffset ?? [0, 0, 0];
    this.rig.position.set(
      0.2 + offsetX + bobX - slash * 0.2,
      -0.19 + offsetY - bobY - dip * 0.08 - cycle * 0.03 - this.lowered * 0.5 + slash * 0.04,
      -0.42 + offsetZ + kick * 0.028 - slash * 0.1,
    );
    this.rig.rotation.set(kick * 0.03 + dip * 0.5 + cycle * 0.12 + slash * 0.1, slash * 0.8, dip * 0.28 + cycle * 0.2 + slash * 0.3);
    this.pose();
  }

  /** Where everything the bench checks is, for the ?art page and e2e/art.mjs. */
  measure(): ViewModelMeasure {
    this.scene.updateMatrixWorld(true);
    this.camera.updateMatrixWorld();
    const { guns, art, palm } = this.held;
    let right: number | null = null;
    let left: number | null = null;
    if (this.arms?.model.visible && art) {
      const b = this.arms.rig.bones;
      const grip = (side: Side, gun: THREE.Object3D) => b[`hand${side}`].localToWorld(scratch.a.copy(palm[side])).distanceTo(gun.getWorldPosition(scratch.b));
      right = grip("R", guns[0]);
      if (guns.length > 1) left = grip("L", guns[1]);
      else if (this.leftOnSupport) left = b.handL.getWorldPosition(scratch.a).distanceTo(this.leftTarget);
    }
    return {
      right,
      left,
      muzzles: this.held.muzzles.map((muzzle) => this.rig.localToWorld(scratch.a.copy(muzzle)).project(this.camera).toArray()),
      armTriangles: this.arms ? this.arms.mesh.geometry.index!.count / 3 : 0,
    };
  }

  /** Frees what this view model made for itself; the art's shared geometry and materials stay. */
  dispose(): void {
    this.dropArms();
    for (const gun of this.held.guns) gun.removeFromParent();
    this.shield.traverse((object) => {
      if (object instanceof THREE.Mesh) {
        object.geometry.dispose();
        (object.material as THREE.Material).dispose();
      }
    });
    this.flash.dispose();
  }

  private dropArms(): void {
    if (!this.arms) return;
    this.arms.model.removeFromParent();
    // The bone texture and the arms' geometry are this view model's own; the material is shared.
    this.arms.mesh.skeleton.dispose();
    this.arms.mesh.geometry.dispose();
    this.arms = null;
  }

  /**
   * Fits the arms to the gun in hand: their pose from the hold's clip (its first frame, the pose
   * `tp` was measured in), the shoulders below and behind where the wrists rest, and the model
   * moved so the right shoulder needs no stretching.
   */
  private fit(): void {
    const arms = this.arms;
    const { art, guns, support } = this.held;
    if (!arms || !art || !guns.length) {
      if (arms) arms.model.visible = false;
      return;
    }
    arms.model.visible = true;
    const rig = arms.rig;
    rig.restore();
    const character = characterArt();
    const clipName = character?.holds?.[art.hold]?.clip ?? character?.clips.idle;
    const clip = clipName && loadedArt()?.models.get(armsArt()!.files[arms.team === Team.RED ? "red" : "blue"])?.animations.find((item) => item.name === clipName);
    if (clip) poseAtStart(arms.model, clip);
    rig.capture();

    this.armsRoot.position.set(0, 0, 0);
    this.rig.updateMatrixWorld(true);
    const inRig = (point: THREE.Vector3) => this.rig.worldToLocal(point);
    const b = rig.bones;
    // The arm's straight length, in the rig.
    const length =
      (b.upperArmR.getWorldPosition(scratch.a).distanceTo(b.foreArmR.getWorldPosition(scratch.b)) + scratch.b.distanceTo(b.handR.getWorldPosition(scratch.c))) /
      this.rig.getWorldScale(scratch.c).x;
    const shoulder = (side: Side, wrist: THREE.Vector3) => wrist.addScaledVector(SHOULDER[side], SHOULDER_REACH * length);
    const restWrist = (side: Side, gun: THREE.Object3D) => {
      // The rig is where this frame put it, so the wrist is worked out in the world and brought back.
      this.handHold(side, gun, scratch.wrist, scratch.hand);
      return inRig(scratch.wrist.clone());
    };
    const shoulders: Record<Side, THREE.Vector3 | null> = { R: shoulder("R", restWrist("R", guns[0])), L: null };
    if (guns.length > 1) shoulders.L = shoulder("L", restWrist("L", guns[1]));
    else if (support) shoulders.L = shoulder("L", support.clone());
    this.held.shoulders = shoulders;
    // The right shoulder where its clavicle already puts it, so only the left one moves.
    this.armsRoot.position.add(shoulders.R!.clone().sub(inRig(b.upperArmR.getWorldPosition(scratch.a))));
    this.pose();
  }

  /** The world place (of the wrist) and turn the hand on `side` takes to hold `gun` as tp says. */
  private handHold(side: Side, gun: THREE.Object3D, wrist: THREE.Vector3, rotation: THREE.Quaternion): void {
    // The hand's turn is the gun's, less the gun's turn in the palm.
    gun.getWorldQuaternion(rotation).multiply(scratch.turn.copy(this.held.turn[side]).invert());
    const scale = this.arms!.rig.bones[`hand${side}`].getWorldScale(scratch.b).x;
    gun.getWorldPosition(wrist).sub(scratch.c.copy(this.held.palm[side]).multiplyScalar(scale).applyQuaternion(rotation));
  }

  /** Poses the arms on the gun for this frame. */
  private pose(): void {
    const arms = this.arms;
    const { art, guns, shoulders, support, back } = this.held;
    this.leftOnSupport = false;
    if (!arms || !art || !guns.length || !shoulders.R) return;
    const rig = arms.rig;
    rig.restore();
    this.rig.updateMatrixWorld(true);
    const world = (point: THREE.Vector3) => this.rig.localToWorld(point);
    const reach = (side: Side, wrist: THREE.Vector3) => {
      const at = world(scratch.a.copy(shoulders[side]!));
      rig.moveShoulder(side, at);
      const pole = world(scratch.pole.copy(shoulders[side]!).add(POLE[side]));
      rig.reachWrist(side, wrist, pole);
    };

    this.handHold("R", guns[0], scratch.wrist, scratch.hand);
    reach("R", scratch.wrist);
    rig.orientHand("R", scratch.hand);

    if (!shoulders.L) {
      // One-handed: the left arm folds away to nothing at its shoulder, below the view.
      rig.bones.clavicleL.scale.setScalar(0);
      return;
    }
    rig.mirrorFingers();
    if (guns.length > 1) {
      this.handHold("L", guns[1], scratch.wrist, scratch.hand);
      reach("L", scratch.wrist);
      rig.orientHand("L", scratch.hand);
      return;
    }
    world(this.leftTarget.copy(support!).addScaledVector(RELOAD_REACH, this.reloadOut).addScaledVector(back, this.cycle * (art.fp.pump ?? 0)));
    this.leftOnSupport = true;
    reach("L", this.leftTarget);
    rig.orientHand("L", guns[0].getWorldQuaternion(scratch.hand).multiply(arms.supportTurn));
  }
}
