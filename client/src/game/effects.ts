import * as THREE from "three";
import { glowTexture } from "./textures";

interface Timed {
  object: THREE.Object3D;
  material: THREE.Material & { opacity: number };
  left: number;
  life: number;
  grow: number;
}

export class Effects {
  private readonly active: Timed[] = [];
  private readonly dust = glowTexture("rgba(205,190,160,0.9)", "rgba(165,150,120,0.45)");
  private readonly blood = glowTexture("rgba(170,20,15,0.95)", "rgba(110,10,10,0.55)");
  private readonly flash = glowTexture("rgba(255,245,205,1)", "rgba(255,170,50,0.8)");
  private readonly tracerMaterial = new THREE.LineBasicMaterial({ color: 0xffe2a2, transparent: true, opacity: 0.75 });

  constructor(private readonly scene: THREE.Scene) {}

  private track(object: THREE.Object3D, material: Timed["material"], life: number, grow: number): void {
    this.scene.add(object);
    this.active.push({ object, material, left: life, life, grow });
  }

  private sprite(map: THREE.Texture, position: THREE.Vector3, size: number, life: number, grow: number, additive = false): void {
    const material = new THREE.SpriteMaterial({
      map,
      transparent: true,
      depthWrite: false,
      blending: additive ? THREE.AdditiveBlending : THREE.NormalBlending,
    });
    const sprite = new THREE.Sprite(material);
    sprite.position.copy(position);
    sprite.scale.setScalar(size);
    this.track(sprite, material, life, grow);
  }

  tracer(from: THREE.Vector3, to: THREE.Vector3): void {
    // Show a moving segment of the path rather than a full beam.
    const dir = to.clone().sub(from);
    const length = dir.length();
    if (length < 3) return;
    dir.divideScalar(length);
    const start = from.clone().addScaledVector(dir, Math.min(2, length * 0.2));
    const end = from.clone().addScaledVector(dir, Math.min(length, 9 + Math.random() * 14));
    const geometry = new THREE.BufferGeometry().setFromPoints([start, end]);
    const material = this.tracerMaterial.clone();
    const line = new THREE.Line(geometry, material);
    this.track(line, material, 0.05, 0);
  }

  impact(position: THREE.Vector3): void {
    this.sprite(this.dust, position, 0.3, 0.32, 2.4);
  }

  bloodPuff(position: THREE.Vector3): void {
    this.sprite(this.blood, position, 0.34, 0.26, 2.2);
  }

  muzzleFlash(position: THREE.Vector3): void {
    this.sprite(this.flash, position, 0.42, 0.05, 0, true);
  }

  explosion(position: THREE.Vector3): void {
    this.sprite(this.flash, position, 3.6, 0.22, 3, true);
    this.sprite(this.dust, position, 3.2, 0.9, 1.3);
  }

  missileTrail(position: THREE.Vector3): void {
    this.sprite(this.flash, position, 0.35, 0.07, 0, true);
    this.sprite(this.dust, position, 0.3, 0.45, 2.6);
  }

  update(dt: number): void {
    for (let i = this.active.length - 1; i >= 0; i--) {
      const fx = this.active[i];
      fx.left -= dt;
      if (fx.left <= 0) {
        fx.object.removeFromParent();
        fx.material.dispose();
        if (fx.object instanceof THREE.Line) fx.object.geometry.dispose();
        this.active.splice(i, 1);
        continue;
      }
      fx.material.opacity = (fx.left / fx.life) * 0.85;
      if (fx.grow) fx.object.scale.multiplyScalar(1 + fx.grow * dt);
    }
  }
}
