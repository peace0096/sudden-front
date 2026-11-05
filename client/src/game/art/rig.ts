// The third-person soldier's procedural layers, applied each frame after the AnimationMixer has
// posed the clips. They work in world space on the canonical bones (tools/art/lib/character.mjs),
// so any rig with those names fits, and they hold the model to the server's hitboxes (hitbox.ts):
// - steady: the chest kept facing where the character faces, however a stride turns the hips;
// - hold: rotations added to named bones (manifest character.holds), the left arm mirroring the
//   right for two guns, and the left fist closed round the gun's support point: the hand turned
//   to hold it and its wrist brought where that puts the fist by two-bone IK, its fingers closed
//   like the right hand's on the grip;
// - aim: the pitch spread over the chest (40%, up to ±0.5), the neck and head (the rest, up to
//   ±0.6) and the arms (whatever remains, so the gun points along the aim), each a rotation about
//   the character's right axis turned into the bone's parent frame;
// - stance: the body moved so the neck stays where the clip put it, over the hitbox column, and
//   lowered by the crouch; each leg is then solved by two-bone IK onto its foot, which the clip
//   plants (the feet hang from the root, not the shins), bending toward the clip's knee pole;
// - head size: the head bone scaled about its joint, which sits on the head box's base.
// The first-person arms (viewmodel.ts) use the arm tools alone: a shoulder moved to a point, a
// wrist brought to a point by the same two-bone IK, and a hand turned to hold a gun.
// A hand holds a gun by its fist: the middle of the ring its middle, ring and little fingers close
// (the weapon's tp.position, measured by tools/art/lib/character.mjs), never by its wrist, so the
// wrist always goes where the turned hand puts that fist on the gun.
// Nothing here allocates per frame: each function has its own scratch objects, and the bone lists
// the loops walk are made once (a hold's, the first time it is added).
import * as THREE from "three";

// The rig also needs each hand's index and middle finger joints 2 to 4 (closeIndex) and the left
// hand's index, middle and little finger knuckles (joint 2, for supportTurn); the constructor looks
// them up by name and throws without them, as it does for these.
const BONES = [
  "root", "body", "spine", "chest", "neck", "head",
  "clavicleL", "clavicleR", "upperArmL", "upperArmR", "foreArmL", "foreArmR", "handL", "handR",
  "thighL", "thighR", "shinL", "shinR", "footL", "footR", "kneePoleL", "kneePoleR",
] as const;
export type BoneName = (typeof BONES)[number];
/** The character's own left or right, as the bone names end. */
export type Side = "L" | "R";
const SIDES: readonly Side[] = ["L", "R"];

/** One side's arm, from the clavicle to the hand. */
interface Arm {
  clavicle: THREE.Object3D;
  upperArm: THREE.Object3D;
  foreArm: THREE.Object3D;
  hand: THREE.Object3D;
}

/** Share of the aim pitch the chest takes, and its limit in radians. */
const CHEST_SHARE = 0.4;
const CHEST_LIMIT = 0.5;
/** Limit of what the neck and head add on top of the chest, split evenly between them. */
const HEAD_LIMIT = 0.6;
/** Share of a shot's kick the chest takes; the arms take the rest. */
const KICK_CHEST = 0.35;
/** How much of a crouch's forward lean the chest takes back, so the shoulders stay over the knees. */
const LEAN_CHEST = 0.5;
/** A leg is never stretched past this share of its length, so the knee keeps a slight bend. */
const LEG_REACH = 0.995;
/** Where the left elbow points while the hand holds a gun, from the shoulder: down, out and back. */
const ELBOW_POLE = { down: 0.5, out: 0.3, back: 0.1 };
const UP = new THREE.Vector3(0, 1, 0);
/**
 * The left hand under a handguard, in the gun's frame (grip at the origin, barrel along −Z): the
 * palm faces up into it, its length running across to the gun's right, a little up and forward,
 * where the fingers close round it, the index finger forward; the forearm comes from below on the
 * left.
 */
const SUPPORT_FINGERS = new THREE.Vector3(0.9, 0.15, -0.2).normalize();
const SUPPORT_INDEX = new THREE.Vector3(0, 0, -1);
/** The finger joints that bend, knuckle out: the index finger takes the middle finger's bends for a fist. */
const BENDS = ["2", "3", "4"] as const;

export interface AimPose {
  /** Radians, up positive, as the server sends it. */
  pitch: number;
  /** Extra pitch from a shot, radians. */
  kick: number;
  /** Radians about the facing axis, for the taser's shaking. */
  shake: number;
  /** Radians the torso bends forward at the spine. */
  lean: number;
  /** Metres the neck goes below where the clip put it. */
  drop: number;
  /** 0 to 1: how much of the stride's foot lift the feet keep (a crouch keeps them nearer the floor). */
  lift: number;
  /** Whether the left arm turns with the right, for a hold where both hands are on the guns. */
  bothArms: boolean;
}

const rotating = { axis: new THREE.Vector3(), parent: new THREE.Quaternion(), turn: new THREE.Quaternion() };

/** A rotation about a world axis through the bone's own joint, written into its local rotation. */
function rotateAbout(bone: THREE.Object3D, axis: THREE.Vector3, angle: number): void {
  if (!angle) return;
  // Rotations about one world axis leave it where it is in every frame turned only about it, so
  // the parent's current frame turns it correctly however the chain above has moved.
  const parent = bone.parent!.getWorldQuaternion(rotating.parent).invert();
  bone.quaternion.premultiply(rotating.turn.setFromAxisAngle(rotating.axis.copy(axis).applyQuaternion(parent), angle));
}

const turning = { from: new THREE.Vector3(), to: new THREE.Vector3(), world: new THREE.Quaternion(), local: new THREE.Quaternion() };

/**
 * Turns `bone` so the world direction `from` (from its joint) becomes `to`, given its parent's
 * world rotation. Returns the turn in world space, valid until the next call.
 */
function turn(bone: THREE.Object3D, from: THREE.Vector3, to: THREE.Vector3, parentWorld: THREE.Quaternion): THREE.Quaternion {
  turning.world.setFromUnitVectors(turning.from.copy(from).normalize(), turning.to.copy(to).normalize());
  // The world turn in the parent's frame: P⁻¹ · Δ · P.
  turning.local.copy(parentWorld).invert().multiply(turning.world).multiply(parentWorld);
  bone.quaternion.premultiply(turning.local);
  return turning.world;
}

const ik = {
  a: new THREE.Vector3(),
  b: new THREE.Vector3(),
  tip: new THREE.Vector3(),
  u: new THREE.Vector3(),
  bend: new THREE.Vector3(),
  knee: new THREE.Vector3(),
  goal: new THREE.Vector3(),
  segment: new THREE.Vector3(),
  from: new THREE.Vector3(),
  to: new THREE.Vector3(),
  upperWorld: new THREE.Quaternion(),
  parentWorld: new THREE.Quaternion(),
};

/**
 * Two-bone IK: turns `upper` and its child `lower` so the point `end` (in lower's frame) reaches
 * `target`, the joint between them bending toward `pole`. Out of reach, the chain points at the
 * target stretched to `reach` of its length. Everything in world space.
 */
function solveTwoBone(upper: THREE.Object3D, lower: THREE.Object3D, end: THREE.Vector3, target: THREE.Vector3, pole: THREE.Vector3, reach = 1): void {
  const a = upper.getWorldPosition(ik.a);
  const b = lower.getWorldPosition(ik.b);
  const tip = ik.tip.copy(end).applyMatrix4(lower.matrixWorld);
  const l1 = a.distanceTo(b);
  const l2 = b.distanceTo(tip);
  const u = ik.u.subVectors(target, a);
  const distance = u.length();
  if (distance < 1e-6 || l1 < 1e-6 || l2 < 1e-6) return;
  u.divideScalar(distance);
  const d = THREE.MathUtils.clamp(distance, Math.abs(l1 - l2) + 1e-4, (l1 + l2) * reach);
  // The bend direction: toward the pole, square to the line from the joint to the target. A pole
  // on that line keeps the bend the joint already has.
  const bend = ik.bend.subVectors(pole, a);
  bend.addScaledVector(u, -bend.dot(u));
  if (bend.lengthSq() < 1e-10) bend.subVectors(b, a).addScaledVector(u, -ik.from.subVectors(b, a).dot(u));
  if (bend.lengthSq() < 1e-10) return;
  bend.normalize();
  const cos = THREE.MathUtils.clamp((l1 * l1 + d * d - l2 * l2) / (2 * l1 * d), -1, 1);
  const sin = Math.sqrt(1 - cos * cos);
  // The middle joint's new place, and where the end goes.
  const knee = ik.knee.copy(a).addScaledVector(u, l1 * cos).addScaledVector(bend, l1 * sin);
  const goal = ik.goal.copy(a).addScaledVector(u, d);

  const upperWorld = upper.getWorldQuaternion(ik.upperWorld);
  const parentWorld = upper.parent!.getWorldQuaternion(ik.parentWorld);
  const segment = ik.segment.subVectors(tip, b);
  const turned = turn(upper, ik.from.subVectors(b, a), ik.to.subVectors(knee, a), parentWorld);
  // The lower bone now hangs from the moved joint, turned with the upper one.
  segment.applyQuaternion(turned);
  upperWorld.premultiply(turned);
  turn(lower, segment, ik.to.subVectors(goal, knee), upperWorld);
}

const posing = {
  right: new THREE.Vector3(),
  forward: new THREE.Vector3(),
  back: new THREE.Vector3(),
  neck: new THREE.Vector3(),
  moved: new THREE.Vector3(),
  shift: new THREE.Vector3(),
  across: new THREE.Vector3(),
  pivot: new THREE.Vector3(),
  joint: new THREE.Vector3(),
  hip: new THREE.Vector3(),
  foot: new THREE.Vector3(),
  pole: new THREE.Vector3(),
  turn: new THREE.Quaternion(),
  frame: new THREE.Quaternion(),
  euler: new THREE.Euler(),
  wanted: new THREE.Quaternion(),
};

const holding = { local: new THREE.Quaternion(), frame: new THREE.Quaternion(), twist: new THREE.Quaternion(), axis: new THREE.Vector3() };
const fisting = { wrist: new THREE.Vector3(), size: new THREE.Vector3(), bend: new THREE.Quaternion() };

function isBelow(bone: THREE.Object3D, ancestor: THREE.Object3D): boolean {
  for (let node = bone.parent; node; node = node.parent) if (node === ancestor) return true;
  return false;
}

/** An orthonormal basis whose first axis is `along` and whose second lies toward `across`. */
function basis3(along: THREE.Vector3, across: THREE.Vector3, basis: THREE.Matrix4): THREE.Matrix4 {
  along.normalize();
  across.addScaledVector(along, -across.dot(along)).normalize();
  return basis.makeBasis(along, across, new THREE.Vector3().crossVectors(along, across));
}

/** One bending joint of the index finger, the middle finger's joint beside it, and both their loaded turns. */
interface Bend {
  index: THREE.Object3D;
  middle: THREE.Object3D;
  indexRest: THREE.Quaternion;
  /** The middle joint's loaded turn, inverted. */
  middleRestInverse: THREE.Quaternion;
}

export class Rig {
  readonly bones: Record<BoneName, THREE.Object3D>;
  /**
   * Every bone's local transform as the clips last set it, before this rig changed it. The mixer
   * only writes a value that changed since its last frame, so the bones must hold what it last
   * wrote when it runs again: restore() puts this back and capture() takes it after the mixer.
   */
  private readonly clipPose: { bone: THREE.Bone; position: THREE.Vector3; quaternion: THREE.Quaternion; scale: THREE.Vector3 }[] = [];
  /** Right-arm bones and their left mirror images, for two guns; the fingers among them. */
  private readonly mirrored: [THREE.Object3D, THREE.Object3D][] = [];
  private readonly fingers: [THREE.Object3D, THREE.Object3D][] = [];
  /** The point on each shin that meets its foot, in the shin's frame. */
  private readonly shinEnd: { L: THREE.Vector3; R: THREE.Vector3 };
  /** Each foot bone's resting height in its parent's frame: the floor it is planted on. */
  private readonly footRest: { L: number; R: number };
  /** Each side's arm bones, and the clavicles the aim turns for one hand on the gun or both. */
  private readonly arms: Record<Side, Arm>;
  private readonly oneClavicle: THREE.Object3D[];
  private readonly bothClavicles: THREE.Object3D[];
  /** Each hold's rotations with their bones looked up, made the first time it is added. */
  private readonly holdBones = new WeakMap<Record<string, number[]>, { bone: THREE.Object3D; rotation: number[] }[]>();
  /** The chest's parent's orientation in the armature's frame as loaded: where steady() keeps it. */
  private readonly steadyFrame: THREE.Quaternion;
  private readonly model: THREE.Object3D;
  /**
   * The left hand's turn relative to a gun whose handguard it holds from below (SUPPORT_*): its
   * world turn is the gun's times this. Made from the hand's own bones, so any rig with these
   * fingers fits.
   */
  readonly supportTurn: THREE.Quaternion;
  /** Each side's index finger joints with the middle finger's beside them, for a full fist. */
  private readonly bends: Record<Side, Bend[]>;
  /** Each hand's loaded turn in its forearm's frame, inverted: the straight wrist wristBend() measures from. */
  private readonly straightWrist: Record<Side, THREE.Quaternion>;

  constructor(model: THREE.Object3D) {
    this.model = model;
    const found: Partial<Record<BoneName, THREE.Object3D>> = {};
    for (const name of BONES) {
      const bone = model.getObjectByName(name);
      if (!bone) throw new Error(`the rig has no ${name} bone`);
      found[name] = bone;
    }
    this.bones = found as Record<BoneName, THREE.Object3D>;
    const b = this.bones;
    for (const [upper, lower] of [["thighL", "shinL"], ["thighR", "shinR"], ["upperArmL", "foreArmL"], ["upperArmR", "foreArmR"], ["foreArmL", "handL"]] as const) {
      if (b[lower].parent !== b[upper]) throw new Error(`${lower} does not hang from ${upper}`);
    }
    this.arms = {
      L: { clavicle: b.clavicleL, upperArm: b.upperArmL, foreArm: b.foreArmL, hand: b.handL },
      R: { clavicle: b.clavicleR, upperArm: b.upperArmR, foreArm: b.foreArmR, hand: b.handR },
    };
    this.oneClavicle = [b.clavicleR];
    this.bothClavicles = [b.clavicleL, b.clavicleR];
    // As loaded, before any clip: the rest pose, whose wrists are straight.
    this.straightWrist = { L: b.handL.quaternion.clone().invert(), R: b.handR.quaternion.clone().invert() };
    model.traverse((object) => {
      if (object instanceof THREE.Bone) {
        this.clipPose.push({ bone: object, position: object.position.clone(), quaternion: object.quaternion.clone(), scale: object.scale.clone() });
      }
    });
    b.clavicleR.traverse((right) => {
      if (!(right instanceof THREE.Bone)) return;
      const left = model.getObjectByName(right.name.replace(/R(\d*)$/, "L$1"));
      if (!left || left === right) return;
      this.mirrored.push([right, left]);
      if (isBelow(right, b.handR)) this.fingers.push([right, left]);
    });
    const finger = (name: string): THREE.Object3D => {
      const bone = model.getObjectByName(name);
      if (!bone) throw new Error(`the rig has no ${name} bone`);
      return bone;
    };
    const bendsOf = (side: Side): Bend[] =>
      BENDS.map((joint) => {
        const index = finger(`index${side}${joint}`);
        const middle = finger(`middle${side}${joint}`);
        return { index, middle, indexRest: index.quaternion.clone(), middleRestInverse: middle.quaternion.clone().invert() };
      });
    this.bends = { L: bendsOf("L"), R: bendsOf("R") };
    model.updateMatrixWorld(true);
    // The hand's own frame from its knuckles as loaded (along the palm from the wrist to the middle
    // knuckle, then across it toward the index knuckle; the fingers' first bones all start at the
    // wrist, so only the knuckles span the palm), and the turn that lines it up with SUPPORT_*:
    // wanted · own⁻¹, both orthonormal.
    const knuckle = (name: string) => b.handL.worldToLocal(finger(`${name}L2`).getWorldPosition(new THREE.Vector3()));
    const own = basis3(knuckle("middle"), knuckle("index").sub(knuckle("pinky")), new THREE.Matrix4()).transpose();
    const wanted = basis3(SUPPORT_FINGERS.clone(), SUPPORT_INDEX.clone(), new THREE.Matrix4());
    this.supportTurn = new THREE.Quaternion().setFromRotationMatrix(wanted.multiply(own)).normalize();
    this.steadyFrame = b.root.parent!.getWorldQuaternion(new THREE.Quaternion()).invert().multiply(b.chest.parent!.getWorldQuaternion(new THREE.Quaternion()));
    const end = (shin: THREE.Object3D, foot: THREE.Object3D) => shin.worldToLocal(foot.getWorldPosition(new THREE.Vector3()));
    this.shinEnd = { L: end(b.shinL, b.footL), R: end(b.shinR, b.footR) };
    this.footRest = { L: b.footL.position.y, R: b.footR.position.y };
  }

  /** Undoes this rig's changes: every bone as the clips last left it (as loaded, at first). */
  restore(): void {
    for (let i = 0; i < this.clipPose.length; i++) {
      const { bone, position, quaternion, scale } = this.clipPose[i];
      bone.position.copy(position);
      bone.quaternion.copy(quaternion);
      bone.scale.copy(scale);
    }
  }

  /** Remembers the pose the mixer just set, before the rig adds to it. */
  capture(): void {
    for (let i = 0; i < this.clipPose.length; i++) {
      const { bone, position, quaternion, scale } = this.clipPose[i];
      position.copy(bone.position);
      quaternion.copy(bone.quaternion);
      scale.copy(bone.scale);
    }
  }

  /**
   * Turns the chest so its parent's frame is as it was loaded, relative to the whole model: the
   * strides twist and lean the hips and spine, and the hold, the aim and the gun stay facing
   * forward on top of them. `weight` fades it.
   */
  steady(weight = 1): void {
    const chest = this.bones.chest;
    const current = chest.parent!.getWorldQuaternion(posing.frame);
    const wanted = this.bones.root.parent!.getWorldQuaternion(posing.wanted).multiply(this.steadyFrame);
    // From the parent's current frame to the wanted one, in the parent's frame.
    const turn = posing.turn.copy(current).invert().multiply(wanted);
    if (weight < 1) turn.slerp(posing.frame.identity(), 1 - weight);
    chest.quaternion.premultiply(turn);
  }

  /**
   * Adds a hold's rotations (XYZ radians in each bone's own frame) after the clip, scaled by
   * `weight` as the hold fades out.
   */
  addRotations(rotations: Record<string, number[]> | undefined, weight = 1): void {
    if (!rotations) return;
    let list = this.holdBones.get(rotations);
    if (!list) {
      list = [];
      for (const [name, rotation] of Object.entries(rotations)) {
        const bone = this.model.getObjectByName(name);
        if (bone) list.push({ bone, rotation });
      }
      this.holdBones.set(rotations, list);
    }
    for (let i = 0; i < list.length; i++) {
      const { bone, rotation } = list[i];
      bone.quaternion.multiply(posing.turn.setFromEuler(posing.euler.set(rotation[0] * weight, rotation[1] * weight, rotation[2] * weight)));
    }
  }

  /** The left arm takes the mirror image of the right arm's pose, fingers included, by `weight`. */
  mirrorArms(weight = 1): void {
    Rig.mirror(this.mirrored, weight);
  }

  /** The left fingers close like the right ones on the grip, for a hand that holds the gun too. */
  mirrorFingers(weight = 1): void {
    Rig.mirror(this.fingers, weight);
  }

  /**
   * The index finger on `side` bends as the middle finger beside it does, from where each was
   * loaded, by `weight`: a full fist round a knife's handle instead of a finger along a trigger.
   */
  closeIndex(side: Side, weight = 1): void {
    const bends = this.bends[side];
    for (let i = 0; i < bends.length; i++) {
      const { index, middle, indexRest, middleRestInverse } = bends[i];
      // The middle joint's bend from its loaded turn, put on the index joint's loaded turn.
      const bent = fisting.bend.copy(indexRest).multiply(posing.turn.copy(middleRestInverse).multiply(middle.quaternion));
      index.quaternion.slerp(bent, weight);
    }
  }

  // A rig's left bones are the mirror images of its right ones, so a local rotation (x, y, z, w)
  // on the right is (x, −y, −z, w) on the left.
  private static mirror(pairs: [THREE.Object3D, THREE.Object3D][], weight: number): void {
    for (let i = 0; i < pairs.length; i++) {
      const q = pairs[i][0].quaternion;
      pairs[i][1].quaternion.slerp(posing.turn.set(q.x, -q.y, -q.z, q.w), weight);
    }
  }

  /**
   * Aim, crouch and legs. `facing` is the object whose +X is the character's right and −Z its
   * facing, like the camera's.
   */
  pose(facing: THREE.Object3D, aim: AimPose): void {
    const b = this.bones;
    facing.updateWorldMatrix(true, false);
    const right = posing.right.setFromMatrixColumn(facing.matrixWorld, 0).normalize();
    const forward = posing.forward.setFromMatrixColumn(facing.matrixWorld, 2).normalize().negate();
    const neck = b.head.getWorldPosition(posing.neck);

    // Pitch up turns the facing axis (−Z) up: a positive rotation about the right axis.
    const chest = THREE.MathUtils.clamp(CHEST_SHARE * aim.pitch, -CHEST_LIMIT, CHEST_LIMIT);
    const head = THREE.MathUtils.clamp(aim.pitch - chest, -HEAD_LIMIT, HEAD_LIMIT) + aim.lean * (1 - LEAN_CHEST);
    const kickChest = aim.kick * KICK_CHEST;
    rotateAbout(b.spine, right, -aim.lean);
    rotateAbout(b.chest, right, chest + aim.lean * LEAN_CHEST + kickChest);
    rotateAbout(b.chest, forward, aim.shake);
    rotateAbout(b.neck, right, head / 2);
    rotateAbout(b.head, right, head / 2);
    // The arms take the rest of the pitch. Two hands on the guns turn together about the
    // clavicles' midpoint, so they keep their hold between them; one hand turns alone.
    const arms = aim.pitch - chest + aim.lean * (1 - LEAN_CHEST) + aim.kick - kickChest;
    if (arms) {
      const pivot = b.clavicleR.getWorldPosition(posing.pivot);
      if (aim.bothArms) pivot.add(b.clavicleL.getWorldPosition(posing.joint)).multiplyScalar(0.5);
      const swing = posing.turn.setFromAxisAngle(right, arms);
      const clavicles = aim.bothArms ? this.bothClavicles : this.oneClavicle;
      for (let i = 0; i < clavicles.length; i++) {
        const clavicle = clavicles[i];
        const at = clavicle.getWorldPosition(posing.joint).sub(pivot).applyQuaternion(swing).add(pivot);
        rotateAbout(clavicle, right, arms);
        clavicle.position.copy(clavicle.parent!.worldToLocal(at));
      }
    }

    // A crouch keeps the feet nearer the floor through the stride.
    b.footL.position.y = this.footRest.L + (b.footL.position.y - this.footRest.L) * aim.lift;
    b.footR.position.y = this.footRest.R + (b.footR.position.y - this.footRest.R) * aim.lift;

    // The neck goes back over the column where the clip had it, and down by the crouch, so it
    // stays on the server's head box whatever the aim and lean did. Upward only as far as both
    // legs still reach their feet.
    const shift = posing.shift.subVectors(neck, b.head.getWorldPosition(posing.moved));
    shift.y -= aim.drop;
    if (shift.y > 0) {
      for (let i = 0; i < SIDES.length; i++) {
        const side = SIDES[i];
        const hip = (side === "L" ? b.thighL : b.thighR).getWorldPosition(posing.hip).add(posing.across.set(shift.x, 0, shift.z));
        const foot = (side === "L" ? b.footL : b.footR).getWorldPosition(posing.foot);
        const length = this.legLength(side) * LEG_REACH;
        const across = Math.hypot(hip.x - foot.x, hip.z - foot.z);
        if (across < length) shift.y = Math.min(shift.y, Math.max(0, foot.y + Math.sqrt(length * length - across * across) - hip.y));
      }
    }
    b.body.position.add(shift.applyQuaternion(b.root.getWorldQuaternion(posing.frame).invert()));

    // The legs reach for the feet the clip planted.
    solveTwoBone(b.thighL, b.shinL, this.shinEnd.L, b.footL.getWorldPosition(posing.foot), b.kneePoleL.getWorldPosition(posing.pole), LEG_REACH);
    solveTwoBone(b.thighR, b.shinR, this.shinEnd.R, b.footR.getWorldPosition(posing.foot), b.kneePoleR.getWorldPosition(posing.pole), LEG_REACH);
  }

  /**
   * Closes the left fist round a world point, the hand turned to the world rotation `turn` and the
   * elbow down and out; `fist` is the fist's middle in the left hand's frame. `facing` is the object
   * whose +X is the character's right and −Z its facing.
   */
  support(hole: THREE.Vector3, turn: THREE.Quaternion, fist: THREE.Vector3, facing: THREE.Object3D): void {
    facing.updateWorldMatrix(true, false);
    const right = posing.right.setFromMatrixColumn(facing.matrixWorld, 0).normalize();
    const back = posing.back.setFromMatrixColumn(facing.matrixWorld, 2).normalize();
    const pole = this.bones.upperArmL.getWorldPosition(posing.pole);
    pole.addScaledVector(UP, -ELBOW_POLE.down).addScaledVector(right, -ELBOW_POLE.out).addScaledVector(back, ELBOW_POLE.back);
    this.closeFist("L", hole, turn, fist, pole);
  }

  /**
   * Closes one fist round a world point: the hand takes the world rotation `turn`, and its wrist is
   * brought by two-bone IK (the elbow bending toward the world point `pole`) to where that turned
   * hand has `fist`, the fist's middle in the hand's frame, on the point.
   */
  closeFist(side: Side, hole: THREE.Vector3, turn: THREE.Quaternion, fist: THREE.Vector3, pole: THREE.Vector3): void {
    const hand = this.arms[side].hand;
    const reach = fisting.wrist.copy(fist).multiplyScalar(hand.getWorldScale(fisting.size).x).applyQuaternion(turn);
    this.reachWrist(side, reach.subVectors(hole, reach), pole);
    this.orientHand(side, turn);
  }

  /** Brings one wrist to a world point by two-bone IK, the elbow bending toward the world point `pole`. */
  reachWrist(side: Side, target: THREE.Vector3, pole: THREE.Vector3): void {
    const arm = this.arms[side];
    solveTwoBone(arm.upperArm, arm.foreArm, arm.hand.position, target, pole);
  }

  /**
   * Moves one shoulder (the upper arm's joint) to a world point by moving its clavicle, the arm
   * coming along. The skin between the clavicle and the chest stretches, so this is for arms
   * whose shoulders are out of sight.
   */
  moveShoulder(side: Side, point: THREE.Vector3): void {
    const { clavicle, upperArm } = this.arms[side];
    const shift = posing.shift.subVectors(point, upperArm.getWorldPosition(posing.joint));
    const at = clavicle.getWorldPosition(posing.moved).add(shift);
    clavicle.position.copy(clavicle.parent!.worldToLocal(at));
  }

  /**
   * Turns one hand to a world orientation. Its roll about the forearm goes to the forearm, which
   * turns about its own length, so the wrist only bends and the skin around it never twists.
   */
  orientHand(side: Side, rotation: THREE.Quaternion): void {
    const { foreArm: forearm, hand } = this.arms[side];
    // The hand's wanted rotation in the forearm's frame, which is its local rotation.
    const local = holding.local.copy(forearm.getWorldQuaternion(holding.frame)).invert().multiply(rotation);
    // Its twist about the forearm's length (forearm joint to wrist): the swing–twist split.
    const axis = holding.axis.copy(hand.position).normalize();
    const along = axis.x * local.x + axis.y * local.y + axis.z * local.z;
    const twist = holding.twist.set(axis.x * along, axis.y * along, axis.z * along, local.w);
    if (twist.lengthSq() < 1e-12) twist.identity();
    else twist.normalize();
    forearm.quaternion.multiply(twist);
    hand.quaternion.copy(twist.invert().multiply(local));
  }

  /**
   * The way the palm on `side` faces, in the world: square to it from its knuckles (along the palm
   * from the wrist to the middle knuckle, across it from the little knuckle to the index one). For
   * the benches that check a hand is turned to hold its gun; it allocates.
   */
  palmFacing(side: Side): THREE.Vector3 {
    const knuckle = (name: string) => this.model.getObjectByName(`${name}${side}2`)!.getWorldPosition(new THREE.Vector3());
    const along = knuckle("middle").sub(this.arms[side].hand.getWorldPosition(new THREE.Vector3()));
    const facing = along.cross(knuckle("index").sub(knuckle("pinky"))).normalize();
    // The left hand is the right one's mirror image, so the same product points out of the left
    // palm and out of the back of the right hand.
    return side === "L" ? facing : facing.negate();
  }

  /**
   * How far the wrist on `side` bends from straight, in radians: the angle between the forearm's
   * length (its joint to the wrist) and the line through the hand that lay along it as loaded. The
   * hand's roll about the forearm is no bend and doesn't count. For the benches that check a hand
   * holds its weapon naturally; it allocates.
   */
  wristBend(side: Side): number {
    const hand = this.arms[side].hand;
    const forearm = hand.position.clone().normalize();
    const carried = forearm.clone().applyQuaternion(this.straightWrist[side]).applyQuaternion(hand.quaternion);
    return forearm.angleTo(carried);
  }

  /** The head grows or shrinks about its joint, the base of the server's head box. */
  setHeadScale(scale: number): void {
    this.bones.head.scale.setScalar(scale);
  }

  private legLength(side: "L" | "R"): number {
    const shin = side === "L" ? this.bones.shinL : this.bones.shinR;
    return shin.position.length() + this.shinEnd[side].length();
  }
}
