// Stand-ins for art that failed to load: a capsule soldier and a box gun, sized to the hitboxes
// so the game still reads right. Every copy shares one set of geometry and materials.
import * as THREE from "three";
import { Team } from "../../gen/game_pb";
import { BODY_TOP, HEAD_BASE, HEAD_HALF_WIDTH, HEAD_HEIGHT, LEGS_HALF_WIDTH } from "./hitbox";

const GUN_LENGTH = 0.6;

let shared: {
  body: THREE.BufferGeometry;
  head: THREE.BufferGeometry;
  gun: THREE.BufferGeometry;
  red: THREE.Material;
  blue: THREE.Material;
  steel: THREE.Material;
} | null = null;

function parts() {
  if (!shared) {
    // From the feet to the top of the body box, as wide as the legs.
    const body = new THREE.CapsuleGeometry(LEGS_HALF_WIDTH, BODY_TOP - LEGS_HALF_WIDTH * 2, 4, 12);
    body.translate(0, BODY_TOP / 2, 0);
    // Around the neck, so it grows from there like the head box.
    const head = new THREE.SphereGeometry(HEAD_HALF_WIDTH, 12, 8);
    head.translate(0, HEAD_HEIGHT / 2, 0);
    // The grip end at the origin, the barrel along -Z.
    const gun = new THREE.BoxGeometry(0.07, 0.1, GUN_LENGTH);
    gun.translate(0, 0, -GUN_LENGTH / 2 + 0.05);
    shared = {
      body,
      head,
      gun,
      red: new THREE.MeshLambertMaterial({ color: 0xb4483a }),
      blue: new THREE.MeshLambertMaterial({ color: 0x3f6fa8 }),
      steel: new THREE.MeshLambertMaterial({ color: 0x2a2c2e }),
    };
  }
  return shared;
}

/**
 * A soldier in the team's colour, feet at the origin, facing -Z. Its parts are named like the
 * rig's bones they stand for: "body", and "head", which pivots at the neck (HEAD_BASE).
 */
export function placeholderFigure(team: Team): THREE.Group {
  const s = parts();
  const material = team === Team.RED ? s.red : s.blue;
  const figure = new THREE.Group();
  const body = new THREE.Mesh(s.body, material);
  body.name = "body";
  const head = new THREE.Group();
  head.name = "head";
  head.position.y = HEAD_BASE;
  head.add(new THREE.Mesh(s.head, material));
  figure.add(body, head);
  return figure;
}

/** A gun held at the origin and pointing along -Z, with a `muzzle` node like the real models. */
export function placeholderGun(): THREE.Group {
  const s = parts();
  const gun = new THREE.Group();
  const muzzle = new THREE.Object3D();
  muzzle.name = "muzzle";
  muzzle.position.z = -GUN_LENGTH + 0.05;
  gun.add(new THREE.Mesh(s.gun, s.steel), muzzle);
  return gun;
}

/** Frees the shared geometry and materials on the GPU; they stay usable. */
export function disposePlaceholders(): void {
  if (!shared) return;
  for (const geometry of [shared.body, shared.head, shared.gun]) geometry.dispose();
  for (const material of [shared.red, shared.blue, shared.steel]) material.dispose();
}
