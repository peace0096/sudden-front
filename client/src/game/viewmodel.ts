// The first-person weapon and hands, drawn in their own scene over the world.
import * as THREE from "three";
import { WeaponKind } from "../gen/game_pb";
import { camoTexture, glowTexture } from "./textures";

const RIG_SCALE = 0.82;

function box(w: number, h: number, d: number, material: THREE.Material, x: number, y: number, z: number): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), material);
  mesh.position.set(x, y, z);
  return mesh;
}

function cylinderZ(radius: number, length: number, material: THREE.Material, x: number, y: number, z: number): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.CylinderGeometry(radius, radius, length, 12), material);
  mesh.rotation.x = Math.PI / 2;
  mesh.position.set(x, y, z);
  return mesh;
}

interface Materials {
  steel: THREE.Material;
  polymer: THREE.Material;
  light: THREE.Material;
  wood: THREE.Material;
}

interface GunModel {
  /** Moves the whole rig, hands included, for weapons held away from the rifle pose. */
  rigOffset?: [number, number, number];
  /** Where the support hand sits sideways; 0 unless the left hand holds a gun. */
  handX?: number;
  /** Sideways position of a second muzzle that fires in turn with the first. */
  altMuzzleX?: number;
  group: THREE.Group;
  magazine: THREE.Object3D;
  magazineY: number;
  muzzleZ: number;
  handZ: number;
  bolt: THREE.Object3D | null;
}

function buildRifle(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.05, 0.068, 0.32, m.light, 0, 0, 0), // Upper receiver.
    box(0.046, 0.05, 0.2, m.steel, 0, -0.055, 0.02), // Lower receiver.
    box(0.052, 0.058, 0.26, m.polymer, 0, 0.002, -0.27), // Handguard.
    box(0.016, 0.016, 0.28, m.steel, 0, 0.008, -0.53), // Barrel.
    box(0.024, 0.024, 0.06, m.steel, 0, 0.008, -0.68), // Flash hider.
    box(0.014, 0.05, 0.014, m.steel, 0, 0.05, -0.42), // Front sight.
    box(0.026, 0.024, 0.2, m.steel, 0, 0.066, -0.02), // Carry handle.
    box(0.012, 0.03, 0.02, m.steel, 0, 0.048, -0.1),
    box(0.012, 0.03, 0.02, m.steel, 0, 0.048, 0.07),
    box(0.04, 0.09, 0.05, m.polymer, 0, -0.105, 0.09), // Pistol grip.
    box(0.042, 0.07, 0.2, m.polymer, 0, -0.01, 0.26), // Stock.
  );
  const magazine = box(0.032, 0.14, 0.05, m.polymer, 0, -0.14, -0.05);
  magazine.rotation.x = 0.16;
  group.add(magazine);
  return { group, magazine, magazineY: -0.14, muzzleZ: -0.76, handZ: -0.28, bolt: null };
}

function buildSniper(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.052, 0.07, 0.62, m.wood, 0, -0.018, -0.08), // Stock body.
    box(0.05, 0.1, 0.24, m.wood, 0, -0.03, 0.32), // Butt.
    box(0.044, 0.044, 0.26, m.steel, 0, 0.03, -0.02), // Receiver.
    cylinderZ(0.011, 0.62, m.steel, 0, 0.03, -0.54), // Long barrel.
    cylinderZ(0.018, 0.07, m.steel, 0, 0.03, -0.86), // Muzzle brake.
    cylinderZ(0.026, 0.26, m.polymer, 0, 0.098, -0.02), // Scope tube.
    cylinderZ(0.036, 0.05, m.polymer, 0, 0.098, -0.17), // Objective bell.
    cylinderZ(0.032, 0.04, m.polymer, 0, 0.098, 0.13),
    box(0.016, 0.03, 0.02, m.steel, 0, 0.06, -0.08),
    box(0.016, 0.03, 0.02, m.steel, 0, 0.06, 0.05),
  );
  const bolt = new THREE.Group();
  bolt.add(box(0.05, 0.012, 0.012, m.steel, 0.045, 0.03, 0.06), box(0.018, 0.018, 0.018, m.steel, 0.074, 0.03, 0.06));
  group.add(bolt);
  const magazine = box(0.04, 0.05, 0.1, m.polymer, 0, -0.075, -0.02);
  group.add(magazine);
  return { group, magazine, magazineY: -0.075, muzzleZ: -0.92, handZ: -0.3, bolt };
}

function buildMachineGun(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.074, 0.09, 0.4, m.light, 0, 0, 0), // Receiver.
    box(0.074, 0.03, 0.22, m.steel, 0, 0.058, -0.02), // Feed cover.
    box(0.06, 0.06, 0.24, m.steel, 0, 0.01, -0.34), // Barrel shroud.
    cylinderZ(0.016, 0.36, m.steel, 0, 0.012, -0.6), // Thick barrel.
    cylinderZ(0.024, 0.07, m.steel, 0, 0.012, -0.8),
    box(0.014, 0.06, 0.014, m.steel, 0, 0.06, -0.44), // Front sight.
    box(0.05, 0.1, 0.05, m.polymer, 0, -0.1, 0.1), // Grip.
    box(0.052, 0.09, 0.22, m.polymer, 0, -0.012, 0.31), // Stock.
    // Folded bipod.
    box(0.01, 0.01, 0.2, m.steel, -0.03, -0.03, -0.46),
    box(0.01, 0.01, 0.2, m.steel, 0.03, -0.03, -0.46),
  );
  const magazine = box(0.09, 0.13, 0.15, m.steel, -0.012, -0.11, -0.06); // Ammo box.
  group.add(magazine);
  return { group, magazine, magazineY: -0.11, muzzleZ: -0.88, handZ: -0.32, bolt: null };
}

function buildSmg(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.044, 0.07, 0.3, m.light, 0, 0, -0.02), // Receiver.
    box(0.046, 0.05, 0.1, m.polymer, 0, -0.003, -0.21), // Front grip area.
    cylinderZ(0.012, 0.1, m.steel, 0, 0.012, -0.31), // Short barrel.
    box(0.014, 0.034, 0.014, m.steel, 0, 0.05, -0.14), // Sights.
    box(0.014, 0.034, 0.014, m.steel, 0, 0.05, 0.08),
    box(0.04, 0.09, 0.05, m.polymer, 0, -0.08, 0.09), // Grip.
    // Wire stock.
    box(0.008, 0.008, 0.22, m.steel, -0.018, 0.012, 0.23),
    box(0.008, 0.008, 0.22, m.steel, 0.018, 0.012, 0.23),
    box(0.044, 0.07, 0.012, m.polymer, 0, -0.01, 0.34),
  );
  const magazine = box(0.028, 0.2, 0.04, m.polymer, 0, -0.14, -0.1); // Long stick mag.
  group.add(magazine);
  return { group, magazine, magazineY: -0.14, muzzleZ: -0.37, handZ: -0.2, bolt: null };
}

function buildPistol(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.034, 0.036, 0.2, m.light, 0, 0.012, -0.08), // Slide.
    box(0.03, 0.026, 0.14, m.steel, 0, -0.018, -0.1), // Frame.
    box(0.012, 0.012, 0.018, m.steel, 0, 0.036, -0.17), // Sights.
    box(0.012, 0.012, 0.018, m.steel, 0, 0.036, 0.005),
    box(0.012, 0.012, 0.03, m.steel, 0, 0.012, -0.195), // Muzzle.
  );
  const grip = box(0.032, 0.11, 0.05, m.polymer, 0, -0.08, 0.0);
  grip.rotation.x = 0.2;
  group.add(grip);
  // The magazine is the grip's lower half, which is what drops out on a reload.
  const magazine = box(0.026, 0.05, 0.04, m.steel, 0, -0.13, 0.012);
  group.add(magazine);
  // Seats the grip in the right hand.
  group.position.set(0, 0, 0.085);
  return { group, magazine, magazineY: -0.13, muzzleZ: -0.125, handZ: 0.05, bolt: null, rigOffset: [-0.03, 0.06, -0.2] };
}

function buildKnife(m: Materials): GunModel {
  const group = new THREE.Group();
  const blade = new THREE.MeshLambertMaterial({ color: 0xb8bcbe });
  group.add(
    box(0.03, 0.04, 0.11, m.polymer, 0, 0, 0), // Handle.
    box(0.05, 0.05, 0.012, m.steel, 0, 0, -0.058), // Guard.
    box(0.006, 0.042, 0.22, blade, 0, 0.002, -0.17), // Blade.
    box(0.004, 0.016, 0.12, m.steel, 0.004, 0.014, -0.12), // Fuller.
  );
  // Pivots at the handle in the right hand, tip raised so the blade reads on screen.
  group.position.set(0, -0.09, 0.07);
  group.rotation.x = 0.6;
  return { group, magazine: new THREE.Object3D(), magazineY: 0, muzzleZ: -0.2, handZ: 0, bolt: null, rigOffset: [-0.02, 0.08, -0.1] };
}

function buildShotgun(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.05, 0.07, 0.3, m.steel, 0, 0, -0.02), // Receiver.
    cylinderZ(0.014, 0.56, m.steel, 0, 0.018, -0.44), // Barrel.
    cylinderZ(0.012, 0.42, m.polymer, 0, -0.012, -0.36), // Magazine tube.
    box(0.012, 0.018, 0.014, m.light, 0, 0.042, -0.7), // Bead sight.
    box(0.04, 0.09, 0.05, m.wood, 0, -0.085, 0.09), // Grip.
    box(0.046, 0.075, 0.26, m.wood, 0, -0.03, 0.26), // Stock.
  );
  const pump = new THREE.Group();
  pump.add(box(0.05, 0.05, 0.16, m.wood, 0, -0.012, -0.32));
  group.add(pump);
  return { group, magazine: new THREE.Object3D(), magazineY: 0, muzzleZ: -0.76, handZ: -0.32, bolt: pump };
}

function buildMagnum(m: Materials): GunModel {
  const group = new THREE.Group();
  group.add(
    box(0.03, 0.036, 0.26, m.steel, 0, 0.016, -0.14), // Barrel with its full-length rib.
    box(0.012, 0.016, 0.018, m.light, 0, 0.042, -0.25), // Front sight.
    cylinderZ(0.03, 0.07, m.light, 0, 0.002, -0.015), // Cylinder.
    box(0.03, 0.03, 0.06, m.steel, 0, -0.012, 0.03), // Frame.
    box(0.012, 0.022, 0.018, m.steel, 0, 0.034, 0.06), // Hammer.
  );
  const grip = box(0.036, 0.11, 0.05, m.wood, 0, -0.075, 0.05);
  grip.rotation.x = 0.28;
  group.add(grip);
  group.position.set(0, 0, 0.05);
  return { group, magazine: new THREE.Object3D(), magazineY: 0, muzzleZ: -0.22, handZ: 0.05, bolt: null, rigOffset: [-0.03, 0.06, -0.2] };
}

function buildAkimbo(m: Materials): GunModel {
  const right = buildSmg(m);
  const left = buildSmg(m);
  const spread = -0.42;
  left.group.position.x = spread;
  const group = new THREE.Group();
  group.add(right.group, left.group);
  return {
    group,
    magazine: right.magazine,
    magazineY: right.magazineY,
    muzzleZ: right.muzzleZ,
    handZ: 0.09,
    handX: spread,
    altMuzzleX: spread,
    bolt: null,
    rigOffset: [0.02, 0, -0.04],
  };
}

function buildTaser(m: Materials): GunModel {
  const group = new THREE.Group();
  const yellow = new THREE.MeshLambertMaterial({ color: 0xe0bb2e });
  group.add(
    box(0.036, 0.044, 0.2, yellow, 0, 0.012, -0.08), // Body.
    box(0.03, 0.03, 0.03, m.polymer, 0, 0.014, -0.195), // Cartridge face.
    box(0.012, 0.012, 0.018, m.steel, 0, 0.042, -0.02), // Sight.
  );
  const grip = box(0.032, 0.11, 0.05, m.polymer, 0, -0.08, 0.0);
  grip.rotation.x = 0.2;
  group.add(grip);
  group.position.set(0, 0, 0.085);
  return { group, magazine: new THREE.Object3D(), magazineY: 0, muzzleZ: -0.125, handZ: 0.05, bolt: null, rigOffset: [-0.03, 0.06, -0.2] };
}

function buildMissileLauncher(m: Materials): GunModel {
  const group = new THREE.Group();
  const body = new THREE.MeshLambertMaterial({ color: 0x4b5742 });
  const dark = new THREE.MeshLambertMaterial({ color: 0x23261c });
  group.add(
    box(0.13, 0.13, 0.78, body, 0, 0.05, -0.1), // Launcher body.
    box(0.15, 0.15, 0.05, dark, 0, 0.05, -0.5), // Front collar.
    cylinderZ(0.024, 0.04, dark, -0.032, 0.082, -0.53), // Tube mouths.
    cylinderZ(0.024, 0.04, dark, 0.032, 0.082, -0.53),
    cylinderZ(0.024, 0.04, dark, -0.032, 0.018, -0.53),
    cylinderZ(0.024, 0.04, dark, 0.032, 0.018, -0.53),
    box(0.02, 0.06, 0.02, m.steel, 0, 0.145, -0.12), // Sight.
    box(0.04, 0.1, 0.05, m.polymer, 0, -0.06, 0.09), // Grip.
  );
  return { group, magazine: new THREE.Object3D(), magazineY: 0, muzzleZ: -0.72, handZ: -0.2, bolt: null, rigOffset: [-0.02, -0.02, -0.04] };
}

export class ViewModel {
  readonly scene = new THREE.Scene();
  readonly camera = new THREE.PerspectiveCamera(50, 1, 0.01, 10);
  private readonly rig = new THREE.Group();
  private readonly materials: Materials;
  private readonly handL = new THREE.Group();
  private readonly handR = new THREE.Group();
  private readonly flash: THREE.Sprite;
  private gun: GunModel | null = null;
  private readonly shield: THREE.Group;
  private kind = WeaponKind.UNSPECIFIED;
  private kickScale = 1;
  private altMuzzle = false;
  private kick = 0;
  private swing = 0;
  private flashLeft = 0;
  private bob = 0;
  private lowered = 0;

  constructor() {
    this.scene.add(new THREE.HemisphereLight(0xf2f2ea, 0x7a6f58, 2.9));
    const key = new THREE.DirectionalLight(0xfff0dc, 1.8);
    key.position.set(1, 2, 1);
    this.scene.add(key);

    this.materials = {
      steel: new THREE.MeshLambertMaterial({ color: 0x5a5e60 }),
      polymer: new THREE.MeshLambertMaterial({ color: 0x3d3f40 }),
      light: new THREE.MeshLambertMaterial({ color: 0x74797a }),
      wood: new THREE.MeshLambertMaterial({ color: 0x6e4a2c }),
    };
    const sleeve = new THREE.MeshLambertMaterial({ map: camoTexture("#5f6a68", ["#3a4244", "#7a8583", "#2c3234"]) });
    const skin = new THREE.MeshLambertMaterial({ color: 0xb98d6c });
    const glove = new THREE.MeshLambertMaterial({ color: 0x3a3530 });

    // Support hand, positioned under each gun's handguard; arm runs back to camera.
    const forearm = box(0.058, 0.058, 0.42, skin, -0.06, -0.1, 0.2);
    forearm.rotation.y = -0.34;
    forearm.rotation.x = 0.36;
    const cuff = box(0.075, 0.075, 0.26, sleeve, -0.15, -0.17, 0.48);
    cuff.rotation.y = -0.34;
    cuff.rotation.x = 0.36;
    this.handL.add(
      box(0.074, 0.05, 0.12, glove, -0.006, -0.036, 0),
      box(0.012, 0.03, 0.08, glove, 0.032, -0.005, 0),
      forearm,
      cuff,
    );

    // Trigger hand on the grip.
    const armR = box(0.07, 0.07, 0.3, sleeve, 0.05, -0.17, 0.22);
    armR.rotation.x = 0.55;
    armR.rotation.y = 0.2;
    this.handR.add(box(0.056, 0.06, 0.07, glove, 0.006, -0.1, 0.085), armR);

    this.flash = new THREE.Sprite(
      new THREE.SpriteMaterial({
        map: glowTexture("rgba(255,250,215,1)", "rgba(255,170,50,0.85)"),
        blending: THREE.AdditiveBlending,
        depthTest: false,
        transparent: true,
      }),
    );
    this.flash.visible = false;

    // The shield bearer's view past the edge of the shield.
    this.shield = new THREE.Group();
    this.shield.add(
      box(0.62, 0.72, 0.03, new THREE.MeshLambertMaterial({ color: 0x3c4348, transparent: true, opacity: 0.82 }), 0, 0, 0),
      box(0.26, 0.07, 0.032, new THREE.MeshLambertMaterial({ color: 0x9fc4d8 }), 0, 0.2, 0),
    );
    this.shield.position.set(-0.3, -0.34, -0.62);
    this.shield.rotation.y = 0.25;
    this.shield.visible = false;
    this.scene.add(this.shield);

    this.rig.add(this.handL, this.handR, this.flash);
    this.rig.scale.setScalar(RIG_SCALE);
    this.scene.add(this.rig);
    this.setWeapon(WeaponKind.RIFLE);
  }

  setWeapon(kind: WeaponKind): void {
    if (kind === this.kind) return;
    this.kind = kind;
    if (this.gun) {
      this.rig.remove(this.gun.group);
      this.gun.group.traverse((o) => {
        if (o instanceof THREE.Mesh) o.geometry.dispose();
      });
    }
    this.gun =
      kind === WeaponKind.SHOTGUN
        ? buildShotgun(this.materials)
        : kind === WeaponKind.MISSILE
          ? buildMissileLauncher(this.materials)
          : kind === WeaponKind.TASER
            ? buildTaser(this.materials)
            : kind === WeaponKind.MAGNUM
            ? buildMagnum(this.materials)
            : kind === WeaponKind.AKIMBO_SMG
              ? buildAkimbo(this.materials)
              : kind === WeaponKind.PISTOL || kind === WeaponKind.PISTOL_PLUS
        ? buildPistol(this.materials)
        : kind === WeaponKind.KNIFE
          ? buildKnife(this.materials)
          : kind === WeaponKind.SNIPER
        ? buildSniper(this.materials)
        : kind === WeaponKind.MACHINE_GUN
          ? buildMachineGun(this.materials)
          : kind === WeaponKind.SMG
            ? buildSmg(this.materials)
            : buildRifle(this.materials);
    this.rig.add(this.gun.group);
    this.handL.position.z = this.gun.handZ;
    this.handL.position.x = this.gun.handX ?? 0;
    // Held one-handed.
    this.handL.visible = ![WeaponKind.KNIFE, WeaponKind.PISTOL, WeaponKind.PISTOL_PLUS, WeaponKind.MAGNUM, WeaponKind.TASER].includes(kind);
    this.flash.position.set(0, 0.01, this.gun.muzzleZ);
    this.kickScale =
      kind === WeaponKind.SNIPER ? 3 : kind === WeaponKind.MACHINE_GUN ? 1.2 : kind === WeaponKind.SMG || kind === WeaponKind.AKIMBO_SMG ? 0.7 : kind === WeaponKind.PISTOL || kind === WeaponKind.PISTOL_PLUS ? 1.6 : kind === WeaponKind.MAGNUM ? 3.4 : kind === WeaponKind.SHOTGUN ? 2.4 : kind === WeaponKind.MISSILE ? 1.8 : 1;
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
    this.flashLeft = 0.045;
    this.flash.visible = true;
    this.altMuzzle = !this.altMuzzle;
    this.flash.position.x = this.altMuzzle ? (this.gun?.altMuzzleX ?? 0) : 0;
    const small = this.kind === WeaponKind.SMG || this.kind === WeaponKind.AKIMBO_SMG;
    this.flash.scale.setScalar((0.2 + Math.random() * 0.12) * (small ? 0.7 : 1));
    this.flash.material.rotation = Math.random() * 6.3;
  }

  /**
   * @param reloadFraction progress of the reload, 0 when not reloading
   * @param boltFraction 1 just after a bolt-action shot, falling to 0 when ready
   */
  update(dt: number, moving: boolean, reloadFraction: number, boltFraction: number, alive: boolean): void {
    this.kick = Math.max(0, this.kick - dt * 11);
    this.swing = Math.max(0, this.swing - dt * 3.4);
    if (moving) this.bob += dt * 9.5;
    this.flashLeft -= dt;
    if (this.flashLeft <= 0) this.flash.visible = false;
    const targetLow = alive ? 0 : 1;
    this.lowered += (targetLow - this.lowered) * Math.min(1, dt * 8);

    const bobX = moving ? Math.sin(this.bob) * 0.006 : 0;
    const bobY = moving ? Math.abs(Math.cos(this.bob)) * 0.007 : 0;

    // Reload: dip the gun, tilt it, drop the magazine out and bring a new one.
    const r = reloadFraction;
    const dip = r > 0 ? Math.sin(Math.min(r, 1) * Math.PI) : 0;
    const out = r > 0 ? Math.max(0, Math.sin(Math.min(r / 0.8, 1) * Math.PI)) : 0;
    const gun = this.gun!;
    gun.magazine.position.y = gun.magazineY - out * 0.3;
    gun.magazine.visible = out < 0.9;
    this.handL.position.y = -out * 0.1;

    // Bolt cycle: pull the handle back and cant the rifle after the recoil settles.
    const cycle = boltFraction > 0 ? Math.sin((1 - boltFraction) * Math.PI) : 0;
    if (gun.bolt) gun.bolt.position.z = cycle * 0.09;

    // Knife: a slash across from the right, recovering to the guard.
    const slash = this.swing > 0 ? Math.sin((1 - this.swing) * Math.PI) : 0;

    const kick = this.kick * this.kickScale;
    const [offsetX, offsetY, offsetZ] = this.gun?.rigOffset ?? [0, 0, 0];
    this.rig.position.set(
      0.2 + offsetX + bobX - slash * 0.2,
      -0.19 + offsetY - bobY - dip * 0.08 - cycle * 0.03 - this.lowered * 0.5 + slash * 0.04,
      -0.42 + offsetZ + kick * 0.028 - slash * 0.1,
    );
    this.rig.rotation.set(kick * 0.03 + dip * 0.5 + cycle * 0.12 + slash * 0.1, slash * 0.8, dip * 0.28 + cycle * 0.2 + slash * 0.3);
  }
}
