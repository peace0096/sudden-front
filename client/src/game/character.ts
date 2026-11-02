import * as THREE from "three";
import { Team, WeaponKind } from "../gen/game_pb";
import { camoTexture, nameTexture } from "./textures";

export function teamColor(team: Team): string {
  return team === Team.RED ? "#ff5a4a" : "#5aa8ff";
}

let shared: {
  red: THREE.Material;
  blue: THREE.Material;
  skin: THREE.Material;
  boot: THREE.Material;
  redHelmet: THREE.Material;
  blueHelmet: THREE.Material;
  gun: THREE.Material;
  shadow: THREE.Material;
  spark: THREE.Material;
} | null = null;

function materials() {
  shared ??= {
    // Woodland camouflage for red, plain navy fatigues for blue.
    red: new THREE.MeshLambertMaterial({
      map: camoTexture("#6a5a3c", ["#3f3a24", "#8a3e2c", "#2e2a1c", "#857650"]),
    }),
    blue: new THREE.MeshLambertMaterial({
      map: camoTexture("#2f4a68", ["#223650", "#3c5f86", "#1c2c40"]),
    }),
    skin: new THREE.MeshLambertMaterial({ color: 0xc69a78 }),
    boot: new THREE.MeshLambertMaterial({ color: 0x1d1a17 }),
    redHelmet: new THREE.MeshLambertMaterial({ color: 0x8f3a2c }),
    blueHelmet: new THREE.MeshLambertMaterial({ color: 0x2c5a8f }),
    gun: new THREE.MeshLambertMaterial({ color: 0x1b1c1d }),
    shadow: new THREE.MeshBasicMaterial({ color: 0x000000, transparent: true, opacity: 0.32, depthWrite: false }),
    spark: new THREE.MeshBasicMaterial({ color: 0xa8ecff, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending, depthWrite: false, fog: false }),
  };
  return shared;
}

function part(w: number, h: number, d: number, material: THREE.Material, x = 0, y = 0, z = 0): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), material);
  mesh.position.set(x, y, z);
  return mesh;
}

/** A limb that pivots at its top so it can swing. */
function limb(w: number, h: number, material: THREE.Material, x: number, y: number, end: THREE.Material): THREE.Group {
  const pivot = new THREE.Group();
  pivot.position.set(x, y, 0);
  pivot.add(part(w, h, w * 1.1, material, 0, -h / 2, 0));
  pivot.add(part(w * 1.05, 0.14, w * 1.5, end, 0, -h + 0.05, -0.03));
  return pivot;
}

const STAND_HIP = 0.85;
/** Matches CROUCH_DROP in the server, which lowers the hitboxes the same way. */
const CROUCH_DROP = 0.5;

/**
 * Standing soldier built to the server's hitboxes: legs to 0.85 m, torso to
 * 1.5 m, head to 1.76 m. Faces -Z at zero rotation, like the camera.
 */
export class Character {
  readonly root = new THREE.Group();
  private readonly body = new THREE.Group();
  private readonly legL: THREE.Group;
  private readonly legR: THREE.Group;
  private readonly upper = new THREE.Group();
  private readonly name: THREE.Sprite;
  private readonly muzzle = new THREE.Object3D();
  private readonly gun = new THREE.Group();
  private weapon = WeaponKind.UNSPECIFIED;
  private walkPhase = 0;
  private deadFor = -1;
  private crouchAmount = 0;
  private readonly headGroup = new THREE.Group();
  private readonly shield = new THREE.Group();
  private xray = false;
  private readonly sparks = new THREE.Group();
  private stunned = false;

  constructor(displayName: string, team: Team) {
    const m = materials();
    const uniform = team === Team.RED ? m.red : m.blue;
    this.legL = limb(0.19, 0.85, uniform, -0.12, 0.85, m.boot);
    this.legR = limb(0.19, 0.85, uniform, 0.12, 0.85, m.boot);

    this.upper.position.y = 0.85;
    const torso = part(0.5, 0.63, 0.3, uniform, 0, 0.315, 0);
    const vest = part(0.53, 0.42, 0.34, new THREE.MeshLambertMaterial({ color: 0x2c2c26 }), 0, 0.33, 0);
    // Pivots at the neck, where the head hitbox starts, for augments that resize it.
    this.headGroup.position.y = 0.65;
    this.headGroup.add(
      part(0.2, 0.22, 0.22, m.skin, 0, 0.11, 0),
      part(0.25, 0.11, 0.27, team === Team.RED ? m.redHelmet : m.blueHelmet, 0, 0.205, 0),
    );
    // Arms reach forward to hold the rifle.
    const armL = part(0.14, 0.14, 0.5, uniform, -0.2, 0.45, -0.28);
    armL.rotation.y = -0.45;
    const armR = part(0.14, 0.14, 0.42, uniform, 0.28, 0.42, -0.18);
    this.gun.position.set(0.1, 0.46, -0.32);
    this.setWeapon(WeaponKind.RIFLE);
    this.upper.add(torso, vest, this.headGroup, armL, armR, this.gun);

    this.body.add(this.legL, this.legR, this.upper);
    this.body.traverse((child) => {
      if (child instanceof THREE.Mesh) child.castShadow = false;
    });
    this.root.add(this.body);

    // Matches SHIELD_* in the server's systems.rs.
    this.shield.add(
      part(0.9, 1.75, 0.08, new THREE.MeshLambertMaterial({ color: 0x3c4348 }), 0, 0.875, -0.6),
      part(0.42, 0.14, 0.09, new THREE.MeshLambertMaterial({ color: 0x9fc4d8 }), 0, 1.42, -0.6),
    );
    this.shield.visible = false;
    this.root.add(this.shield);

    // Arcs flickering around a paralysed soldier.
    for (let i = 0; i < 10; i++) this.sparks.add(part(0.025, 0.42, 0.025, m.spark));
    this.sparks.visible = false;
    this.root.add(this.sparks);

    // Sun shadows are baked, so moving soldiers get a soft blob instead.
    const blob = new THREE.Mesh(new THREE.CircleGeometry(0.42, 16), m.shadow);
    blob.rotation.x = -Math.PI / 2;
    blob.position.y = 0.02;
    this.root.add(blob);

    const texture = nameTexture(displayName, teamColor(team));
    this.name = new THREE.Sprite(new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true, sizeAttenuation: false }));
    this.name.scale.set(0.24, 0.045, 1);
    this.name.position.y = 2.1;
    this.name.renderOrder = 10;
    this.root.add(this.name);
  }

  setWeapon(kind: WeaponKind): void {
    if (kind === this.weapon) return;
    this.weapon = kind;
    const m = materials();
    for (const child of [...this.gun.children]) {
      this.gun.remove(child);
      if (child instanceof THREE.Mesh) child.geometry.dispose();
    }
    let muzzleZ: number;
    switch (kind) {
      case WeaponKind.SNIPER:
        this.gun.add(part(0.06, 0.1, 0.9, m.gun, 0, 0, -0.08));
        this.gun.add(part(0.03, 0.03, 0.62, m.gun, 0, 0.02, -0.82));
        this.gun.add(part(0.06, 0.07, 0.3, m.gun, 0, 0.1, -0.1));
        muzzleZ = -1.14;
        break;
      case WeaponKind.MACHINE_GUN:
        this.gun.add(part(0.1, 0.14, 0.9, m.gun, 0, 0, -0.08));
        this.gun.add(part(0.13, 0.17, 0.2, m.gun, -0.02, -0.15, -0.1));
        this.gun.add(part(0.05, 0.05, 0.4, m.gun, 0, 0.02, -0.72));
        muzzleZ = -0.93;
        break;
      case WeaponKind.MAGNUM:
        this.gun.add(part(0.05, 0.07, 0.34, m.gun, 0, 0, -0.2));
        this.gun.add(part(0.08, 0.09, 0.1, m.gun, 0, -0.005, -0.08));
        this.gun.add(part(0.045, 0.14, 0.06, new THREE.MeshLambertMaterial({ color: 0x5a3e28 }), 0, -0.1, -0.03));
        muzzleZ = -0.38;
        break;
      case WeaponKind.AKIMBO_SMG:
        // The second gun, in the left hand; the first is the plain SMG below.
        this.gun.add(part(0.07, 0.1, 0.5, m.gun, -0.42, 0, -0.02));
        this.gun.add(part(0.05, 0.3, 0.07, m.gun, -0.42, -0.18, -0.14));
        this.gun.add(part(0.07, 0.1, 0.5, m.gun, 0, 0, -0.02));
        this.gun.add(part(0.05, 0.3, 0.07, m.gun, 0, -0.18, -0.14));
        this.gun.add(part(0.03, 0.03, 0.14, m.gun, 0, 0.01, -0.34));
        muzzleZ = -0.42;
        break;
      case WeaponKind.PISTOL:
      case WeaponKind.PISTOL_PLUS:
        this.gun.add(part(0.05, 0.08, 0.26, m.gun, 0, 0, -0.16));
        this.gun.add(part(0.045, 0.14, 0.06, m.gun, 0, -0.1, -0.06));
        muzzleZ = -0.3;
        break;
      case WeaponKind.SHOTGUN:
        this.gun.add(part(0.07, 0.1, 0.6, m.gun, 0, 0, -0.02));
        this.gun.add(part(0.04, 0.04, 0.5, m.gun, 0, 0.02, -0.5));
        this.gun.add(part(0.06, 0.06, 0.2, new THREE.MeshLambertMaterial({ color: 0x5a3e28 }), 0, -0.03, -0.42));
        muzzleZ = -0.76;
        break;
      case WeaponKind.TASER:
        this.gun.add(part(0.05, 0.08, 0.24, new THREE.MeshLambertMaterial({ color: 0xd8b52c }), 0, 0, -0.15));
        this.gun.add(part(0.045, 0.14, 0.06, m.gun, 0, -0.1, -0.06));
        muzzleZ = -0.28;
        break;
      case WeaponKind.MISSILE:
        this.gun.add(part(0.2, 0.2, 1.05, new THREE.MeshLambertMaterial({ color: 0x3f4a3a }), 0, 0.07, -0.1));
        this.gun.add(part(0.24, 0.24, 0.12, new THREE.MeshLambertMaterial({ color: 0x26291f }), 0, 0.07, -0.66));
        muzzleZ = -0.74;
        break;
      case WeaponKind.KNIFE:
        this.gun.add(part(0.03, 0.06, 0.12, m.gun, 0, 0, -0.1));
        this.gun.add(part(0.012, 0.05, 0.26, new THREE.MeshLambertMaterial({ color: 0xb8bcbe }), 0, 0.005, -0.28));
        muzzleZ = -0.4;
        break;
      case WeaponKind.SMG:
        this.gun.add(part(0.07, 0.1, 0.5, m.gun, 0, 0, -0.02));
        this.gun.add(part(0.05, 0.3, 0.07, m.gun, 0, -0.18, -0.14));
        this.gun.add(part(0.03, 0.03, 0.14, m.gun, 0, 0.01, -0.34));
        muzzleZ = -0.42;
        break;
      default:
        this.gun.add(part(0.07, 0.11, 0.92, m.gun, 0, 0, -0.1));
        this.gun.add(part(0.05, 0.22, 0.09, m.gun, 0, -0.14, -0.14));
        this.gun.add(part(0.03, 0.03, 0.3, m.gun, 0, 0.01, -0.68));
        muzzleZ = -0.86;
    }
    this.muzzle.position.set(0, 0.01, muzzleZ);
    this.gun.add(this.muzzle);
    if (this.xray) this.markXray();
  }

  /** Puts the model on the layer the Wallhack pass draws. */
  enableXray(): void {
    this.xray = true;
    this.markXray();
  }

  private markXray(): void {
    for (const group of [this.body, this.shield]) group.traverse((child) => child.layers.enable(1));
  }

  setLook(headScale: number, shielded: boolean, stunned: boolean): void {
    this.stunned = stunned;
    this.headGroup.scale.setScalar(headScale);
    this.shield.visible = shielded;
  }

  setNameVisible(visible: boolean): void {
    this.name.visible = visible && this.deadFor < 0;
  }

  muzzleWorldPosition(target: THREE.Vector3): THREE.Vector3 {
    return this.muzzle.getWorldPosition(target);
  }

  update(dt: number, x: number, z: number, yaw: number, pitch: number, moving: boolean, alive: boolean, shielded: boolean, time: number, crouching: boolean): void {
    this.crouchAmount += (Number(crouching && alive) - this.crouchAmount) * Math.min(1, dt * 14);
    const hip = STAND_HIP - CROUCH_DROP * this.crouchAmount;
    this.upper.position.y = hip;
    this.legL.position.y = this.legR.position.y = hip;
    this.legL.scale.y = this.legR.scale.y = hip / STAND_HIP;
    this.name.position.y = 2.1 - CROUCH_DROP * this.crouchAmount;
    this.shield.scale.y = 1 - (CROUCH_DROP / 1.75) * this.crouchAmount;

    this.root.position.set(x, 0, z);
    this.root.rotation.y = yaw;
    this.upper.rotation.x = pitch * 0.55;

    this.sparks.visible = this.stunned && alive;
    if (this.sparks.visible) {
      this.upper.rotation.z = Math.sin(time * 60) * 0.05;
      for (const arc of this.sparks.children) {
        arc.visible = Math.random() > 0.3;
        arc.position.set((Math.random() - 0.5) * 0.9, 0.2 + Math.random() * (1.5 - CROUCH_DROP * this.crouchAmount), (Math.random() - 0.5) * 0.9);
        arc.rotation.set(Math.random() * 6.3, Math.random() * 6.3, Math.random() * 6.3);
      }
    } else {
      this.upper.rotation.z = 0;
    }

    if (alive) {
      this.deadFor = -1;
      this.body.rotation.x = 0;
      this.body.position.y = 0;
      this.root.visible = !shielded || Math.sin(time * 26) > -0.3;
      if (moving) this.walkPhase += dt * 10;
      const swing = moving ? Math.sin(this.walkPhase) * 0.6 : 0;
      this.legL.rotation.x = swing;
      this.legR.rotation.x = -swing;
      return;
    }

    if (this.deadFor < 0) this.deadFor = 0;
    this.deadFor += dt;
    const fall = Math.min(this.deadFor / 0.35, 1);
    this.body.rotation.x = (fall * fall * Math.PI) / 2;
    this.body.position.y = fall * 0.14;
    this.name.visible = false;
    this.root.visible = this.deadFor < 2.6;
  }

  dispose(): void {
    this.root.removeFromParent();
    (this.name.material.map as THREE.Texture).dispose();
    this.name.material.dispose();
  }
}
