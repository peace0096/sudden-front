import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { BoxKind, MapId, PropKind, type MapBox, type MapData, type MapProp } from "../gen/game_pb";
import { loadedArt } from "./art/loader";
import { kitArt, propArt, type KitArt } from "./art/manifest";
import { toLambert } from "./art/materials";
import * as tex from "./textures";

interface Surface {
  material: THREE.Material;
  /** World metres covered by one texture repeat. */
  tile: [number, number];
}

function surfaces(): Map<BoxKind, Surface> {
  const lambert = (map: THREE.Texture) => new THREE.MeshLambertMaterial({ map });
  return new Map<BoxKind, Surface>([
    [BoxKind.BRICK, { material: lambert(tex.brickTexture()), tile: [3, 3] }],
    [BoxKind.CONCRETE, { material: lambert(tex.concreteTexture()), tile: [3, 3] }],
    [BoxKind.ROOF, { material: lambert(tex.roofTexture()), tile: [4, 4] }],
  ]);
}

/**
 * The boxes drawn as a kit model fitted to the box (manifest.json map.kits, by BoxKind name), and
 * the flat colour of the plain box drawn instead if the model is missing: the base colours of the
 * painted boxes from before the kit models.
 */
const KIT_BOXES = new Map<BoxKind, string>([
  [BoxKind.CONTAINER_BLUE, "#44627c"],
  [BoxKind.CONTAINER_GRAY, "#8d9598"],
  [BoxKind.CONTAINER_RUST, "#8c4b30"],
  [BoxKind.CONTAINER_PALE, "#9db0bc"],
  [BoxKind.CRATE, "#8d6f49"],
  [BoxKind.BARREL, "#4c5a3c"],
]);

/** The leaning container's height; the prop's width and height are its length and depth. */
const TILTED_HEIGHT = 2.6;
/** How far it tips one end up: radians about its own z axis, across its length. */
const TILT = 0.34;

/** Props drawn as a textured plane, width × height, facing +Z turned by the prop's rotation. */
const PLANES = new Map<PropKind, () => THREE.Texture>([
  [PropKind.SHUTTER, tex.shutterTexture],
  [PropKind.BILLBOARD, tex.billboardTexture],
  [PropKind.WINDOW, tex.windowTexture],
  [PropKind.LOUVER, tex.louverTexture],
]);

/** ?debug&collision edges sit this far out from (and up off) the box, so they draw over its faces. */
const EDGE_OUT = 0.01;

const UP = new THREE.Vector3(0, 1, 0);
const ONE = new THREE.Vector3(1, 1, 1);
const WHITE = new THREE.Color(1, 1, 1);

/** A box whose UVs are in world metres so textures keep their scale at any size. */
function boxGeometry(b: MapBox, tile: [number, number]): THREE.BufferGeometry {
  const sx = b.maxX - b.minX;
  const sy = b.maxY - b.minY;
  const sz = b.maxZ - b.minZ;
  const geometry = new THREE.BoxGeometry(sx, sy, sz);
  const uv = geometry.getAttribute("uv") as THREE.BufferAttribute;
  // BoxGeometry face order: +x -x +y -y +z -z, four vertices each.
  const faceSize: [number, number][] = [[sz, sy], [sz, sy], [sx, sz], [sx, sz], [sx, sy], [sx, sy]];
  for (let face = 0; face < 6; face++) {
    for (let v = 0; v < 4; v++) {
      const i = face * 4 + v;
      uv.setXY(i, (uv.getX(i) * faceSize[face][0]) / tile[0], (uv.getY(i) * faceSize[face][1]) / tile[1]);
    }
  }
  geometry.translate((b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2, (b.minZ + b.maxZ) / 2);
  return geometry;
}

/** The plain stand-in for a barrel whose model is missing. */
function barrelGeometry(b: MapBox): THREE.BufferGeometry {
  const radius = (b.maxX - b.minX) / 2;
  const geometry = new THREE.CylinderGeometry(radius, radius, b.maxY - b.minY, 14);
  geometry.translate((b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2, (b.minZ + b.maxZ) / 2);
  return geometry;
}

/** The stand-in for a lamp whose model is missing. */
function lampModel(rotation: number): THREE.Group {
  const lamp = new THREE.Group();
  const metal = new THREE.MeshLambertMaterial({ color: 0x8f9394 });
  const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.07, 0.1, 5.6, 8), metal);
  pole.position.y = 2.8;
  const arm = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.08, 1.7), metal);
  arm.position.set(0, 5.55, 0.8);
  const head = new THREE.Mesh(
    new THREE.BoxGeometry(0.34, 0.1, 0.7),
    new THREE.MeshLambertMaterial({ color: 0xdedbc8, emissive: 0x77735f }),
  );
  head.position.set(0, 5.5, 1.6);
  const base = new THREE.Mesh(new THREE.BoxGeometry(0.4, 0.05, 0.4), metal);
  base.position.y = 0.03;
  lamp.add(pole, arm, head, base);
  for (const part of lamp.children) part.castShadow = true;
  lamp.rotation.y = rotation;
  return lamp;
}

/** A roof truss spanning along Z, in world space, as the separate beams to merge. */
function trussParts(prop: MapProp): THREE.BufferGeometry[] {
  const span = prop.width;
  const height = prop.height;
  const parts: THREE.BufferGeometry[] = [];
  const add = (geometry: THREE.BufferGeometry, y: number, z: number, tilt = 0) => {
    geometry.rotateX(tilt);
    geometry.translate(prop.x, prop.y + y, prop.z + z);
    parts.push(geometry);
  };
  add(new THREE.BoxGeometry(0.28, 0.28, span), 0, 0);
  for (const side of [-1, 1]) {
    const length = Math.hypot(span / 2, height);
    add(new THREE.BoxGeometry(0.24, 0.24, length), height / 2, (side * span) / 4, side * Math.atan2(height, span / 2));
    for (const fraction of [0.25, 0.6]) {
      add(new THREE.BoxGeometry(0.14, height * (1 - fraction), 0.14), (height * (1 - fraction)) / 2, (side * span * fraction) / 2);
    }
  }
  return parts;
}

/** A painted line on the floor, width × height, turned by the prop's rotation. */
function floorLine(prop: MapProp): THREE.BufferGeometry {
  const rotation = new THREE.Quaternion().setFromEuler(new THREE.Euler(-Math.PI / 2, 0, prop.rotation));
  return new THREE.PlaneGeometry(prop.width, prop.height).applyMatrix4(
    new THREE.Matrix4().compose(new THREE.Vector3(prop.x, prop.y, prop.z), rotation, ONE),
  );
}

/** A coin flip that comes out the same for a box every match. */
function flipped(box: MapBox): boolean {
  // Its centre in decimetres, so float noise in the map data can't change the outcome.
  const x = Math.round((box.minX + box.maxX) * 5);
  const z = Math.round((box.minZ + box.maxZ) * 5);
  let h = Math.imul(x, 0x9e3779b1) ^ Math.imul(z, 0x85ebca77) ^ Math.round(box.minY * 10);
  h = Math.imul(h ^ (h >>> 16), 0x7feb352d);
  return ((h ^ (h >>> 15)) & 1) === 1;
}

/**
 * Where a kit model built to `size` ([x, y, z] with x its long side, standing on y = 0 around
 * x = z = 0) goes to fill a box: scaled to the box, which is 1 for models baked to the server's
 * sizes; its long side along the box's; turned end for end by a coin flip so containers don't all
 * show their doors the same way.
 */
function fitToBox(box: MapBox, size: number[] | undefined): THREE.Matrix4 {
  const sx = box.maxX - box.minX;
  const sy = box.maxY - box.minY;
  const sz = box.maxZ - box.minZ;
  // Clearly longer, so float noise never turns a square box.
  const alongZ = sz > sx + 0.01;
  const long = alongZ ? sz : sx;
  const short = alongZ ? sx : sz;
  const [kx, ky, kz] = size ?? [long, sy, short];
  const yaw = (alongZ ? Math.PI / 2 : 0) + (flipped(box) ? Math.PI : 0);
  return new THREE.Matrix4().compose(
    new THREE.Vector3((box.minX + box.maxX) / 2, box.minY, (box.minZ + box.maxZ) / 2),
    new THREE.Quaternion().setFromAxisAngle(UP, yaw),
    new THREE.Vector3(long / kx, sy / ky, short / kz),
  );
}

/** Kit models already said to be missing: a page warns once for each, not once a match. */
const missing = new Set<string>();

/**
 * A kit's loaded model, or null when there is none and the caller draws a plain stand-in. A file
 * that failed to load was already reported by the loader; anything else is reported here.
 */
function kitModel(name: string, art: KitArt | undefined): THREE.Object3D | null {
  const set = loadedArt();
  const model = art && set?.models.get(art.file)?.scene;
  if (model) return model;
  const key = art?.file ?? name;
  const reported = !!art && !!set?.failures.some((failure) => failure.file === art.file);
  if (!reported && !missing.has(key)) {
    missing.add(key);
    console.warn(`[art] ${art ? `${art.file} is not loaded` : `manifest.json has no map kit ${name}`}: drawn as a plain shape`);
  }
  return null;
}

/** Every copy of one kit model in the map: its matrix, and its colour if the kit is tinted. */
interface KitBatch {
  model: THREE.Object3D;
  tintNode?: string;
  matrices: THREE.Matrix4[];
  tints: (THREE.Color | null)[];
}

/** Kit copies, whose geometry and materials are the art's own, shared from match to match. */
const sharedParts = new WeakSet<THREE.Object3D>();

function under(object: THREE.Object3D | null, name: string): boolean {
  for (; object; object = object.parent) if (object.name === name) return true;
  return false;
}

/**
 * One InstancedMesh per mesh in a kit model, holding every copy: geometry shared with the model,
 * its material as Lambert, each copy's matrix times the mesh's place in the model (where a glTF
 * scales its quantised positions), and each copy's tint on the meshes under tintNode only.
 * Named "<file>#<mesh>".
 */
function instanceKit(file: string, batch: KitBatch): THREE.InstancedMesh[] {
  const template = batch.model.clone();
  toLambert(template);
  template.updateMatrixWorld(true);
  const toRoot = template.matrixWorld.clone().invert();
  const inModel = new THREE.Matrix4();
  const matrix = new THREE.Matrix4();
  const meshes: THREE.InstancedMesh[] = [];
  template.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    inModel.multiplyMatrices(toRoot, object.matrixWorld);
    const tinted = !!batch.tintNode && under(object, batch.tintNode);
    const mesh = new THREE.InstancedMesh(object.geometry, object.material, batch.matrices.length);
    sharedParts.add(mesh);
    mesh.name = `${file}#${object.name}`;
    batch.matrices.forEach((placed, i) => {
      mesh.setMatrixAt(i, matrix.multiplyMatrices(placed, inModel));
      if (tinted) mesh.setColorAt(i, batch.tints[i] ?? WHITE);
    });
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    // Culling needs the bounds of every copy, not of one model at the origin.
    mesh.computeBoundingSphere();
    meshes.push(mesh);
  });
  return meshes;
}

/** One InstancedMesh of a geometry at each matrix. */
function instances(name: string, geometry: THREE.BufferGeometry, material: THREE.Material, matrices: THREE.Matrix4[]): THREE.InstancedMesh {
  const mesh = new THREE.InstancedMesh(geometry, material, matrices.length);
  mesh.name = name;
  matrices.forEach((matrix, i) => mesh.setMatrixAt(i, matrix));
  mesh.computeBoundingSphere();
  return mesh;
}

/** ?debug&collision: the edges of every box the server collides with, invisible ones in magenta. */
function collisionEdges(boxes: MapBox[]): THREE.LineSegments {
  const positions: number[] = [];
  const colours: number[] = [];
  const solid = new THREE.Color(0xffd21f);
  const invisible = new THREE.Color(0xff3df2);
  for (const b of boxes) {
    const x = [b.minX - EDGE_OUT, b.maxX + EDGE_OUT];
    const y = [b.minY + EDGE_OUT, b.maxY + EDGE_OUT];
    const z = [b.minZ - EDGE_OUT, b.maxZ + EDGE_OUT];
    const colour = b.kind === BoxKind.INVISIBLE ? invisible : solid;
    for (const i of [0, 1]) {
      for (const j of [0, 1]) {
        positions.push(x[0], y[i], z[j], x[1], y[i], z[j]);
        positions.push(x[i], y[0], z[j], x[i], y[1], z[j]);
        positions.push(x[i], y[j], z[0], x[i], y[j], z[1]);
        for (let k = 0; k < 6; k++) colours.push(colour.r, colour.g, colour.b);
      }
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colours, 3));
  const edges = new THREE.LineSegments(geometry, new THREE.LineBasicMaterial({ vertexColors: true, fog: false, toneMapped: false }));
  edges.name = "collision";
  return edges;
}

function addGround(world: THREE.Group, map: MapData): void {
  const indoor = map.id === MapId.WAREHOUSE;
  const surface = indoor ? tex.concreteTexture() : tex.sandTexture();
  surface.repeat.set(map.halfX / (indoor ? 2 : 6), map.halfZ / (indoor ? 2 : 6));
  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(map.halfX * 2 + 4, map.halfZ * 2 + 4),
    new THREE.MeshLambertMaterial({ map: surface }),
  );
  ground.name = "ground";
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  world.add(ground);

  if (indoor) return;

  // Warehouses get a concrete floor under their roof, all of them one mesh with UVs in world
  // metres: one repeat of the texture covers 3 m.
  const floors = map.boxes
    .filter((b) => b.kind === BoxKind.ROOF)
    .map((roof) => {
      const width = roof.maxX - roof.minX - 1;
      const depth = roof.maxZ - roof.minZ - 1;
      const floor = new THREE.PlaneGeometry(width, depth);
      const uv = floor.getAttribute("uv") as THREE.BufferAttribute;
      for (let i = 0; i < uv.count; i++) uv.setXY(i, (uv.getX(i) * width) / 3, (uv.getY(i) * depth) / 3);
      floor.rotateX(-Math.PI / 2);
      floor.translate((roof.minX + roof.maxX) / 2, 0.012, (roof.minZ + roof.maxZ) / 2);
      return floor;
    });
  if (!floors.length) return;
  const floor = new THREE.Mesh(mergeGeometries(floors), new THREE.MeshLambertMaterial({ map: tex.concreteTexture() }));
  floor.name = "roofed floor";
  floor.receiveShadow = true;
  world.add(floor);
}

/**
 * Adds the map to the scene, the sky and lights included, as one group named "world", which it
 * returns. Containers, crates, barrels and lamps are kit models, drawn as instances fitted to the
 * server's boxes (a plain shape stands in for a model that failed to load); walls, concrete and
 * roofs are merged boxes, one mesh per surface; props of a kind share one mesh. All of it exists
 * before the first frame, which bakes the sun's shadows once.
 */
export function buildWorld(scene: THREE.Scene, map: MapData): THREE.Group {
  const world = new THREE.Group();
  world.name = "world";
  scene.add(world);
  const sky = new THREE.Mesh(
    new THREE.SphereGeometry(400, 24, 16),
    new THREE.MeshBasicMaterial({ map: tex.skyTexture(), side: THREE.BackSide, fog: false }),
  );
  sky.name = "sky";
  world.add(sky);
  const indoor = map.id === MapId.WAREHOUSE;
  scene.fog = indoor ? new THREE.Fog(0x2a2622, 40, 160) : new THREE.Fog(0xcdd6d4, 70, 260);

  world.add(
    indoor
      ? new THREE.HemisphereLight(0xc4ccd2, 0x5a5044, 0.8)
      : new THREE.HemisphereLight(0xfff1dc, 0xb09c7c, 2.1),
  );
  // Indoors this is only soft fill to separate faces; the roof would shadow it all.
  const sun = new THREE.DirectionalLight(0xfff0d8, indoor ? 0.35 : 2.2);
  sun.position.set(38, 70, 26);
  sun.castShadow = !indoor;
  sun.shadow.mapSize.set(4096, 4096);
  const camera = sun.shadow.camera;
  camera.left = -66;
  camera.right = 66;
  camera.top = 50;
  camera.bottom = -50;
  camera.near = 10;
  camera.far = 180;
  sun.shadow.bias = -0.0006;
  sun.shadow.normalBias = 0.04;
  world.add(sun, sun.target);

  addGround(world, map);

  const batches = new Map<string, KitBatch>();
  const place = (art: KitArt, model: THREE.Object3D, matrix: THREE.Matrix4) => {
    let batch = batches.get(art.file);
    if (!batch) batches.set(art.file, (batch = { model, tintNode: art.tintNode, matrices: [], tints: [] }));
    batch.matrices.push(matrix);
    batch.tints.push(art.tint ? new THREE.Color(art.tint) : null);
  };

  const table = surfaces();
  const plain = new Map<BoxKind, THREE.Material>();
  const grouped = new Map<BoxKind, THREE.BufferGeometry[]>();
  for (const b of map.boxes) {
    let geometry: THREE.BufferGeometry;
    const colour = KIT_BOXES.get(b.kind);
    if (colour !== undefined) {
      const art = kitArt(BoxKind[b.kind]);
      const model = kitModel(BoxKind[b.kind], art);
      if (art && model) {
        place(art, model, fitToBox(b, art.size));
        continue;
      }
      if (!plain.has(b.kind)) plain.set(b.kind, new THREE.MeshLambertMaterial({ color: colour }));
      geometry = b.kind === BoxKind.BARREL ? barrelGeometry(b) : boxGeometry(b, [1, 1]);
    } else {
      const surface = table.get(b.kind);
      if (!surface) continue;
      geometry = boxGeometry(b, surface.tile);
    }
    const list = grouped.get(b.kind) ?? [];
    list.push(geometry);
    grouped.set(b.kind, list);
  }
  for (const [kind, geometries] of grouped) {
    const mesh = new THREE.Mesh(mergeGeometries(geometries), table.get(kind)?.material ?? plain.get(kind)!);
    mesh.name = BoxKind[kind];
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    world.add(mesh);
  }

  const planes = new Map<PropKind, THREE.Matrix4[]>();
  const trusses: THREE.BufferGeometry[] = [];
  const lines: THREE.BufferGeometry[] = [];
  const fixtures: THREE.Matrix4[] = [];
  const lightPower = indoor ? 62 : 32;
  for (const prop of map.props) {
    switch (prop.kind) {
      case PropKind.TRUSS:
        trusses.push(...trussParts(prop));
        break;
      case PropKind.TILTED_CONTAINER: {
        // Leans over at an angle, with its collision footprint authored on the server: a container
        // centred on the prop, turned by its rotation and tipped one end up.
        const at = new THREE.Vector3(prop.x, prop.y, prop.z);
        const rotation = new THREE.Quaternion().setFromEuler(new THREE.Euler(0, prop.rotation, TILT));
        const art = propArt(PropKind[prop.kind]);
        const model = kitModel(PropKind[prop.kind], art);
        if (art && model) {
          const [kx, ky, kz] = art.size ?? [prop.width, TILTED_HEIGHT, prop.height];
          const matrix = new THREE.Matrix4().compose(at, rotation, new THREE.Vector3(prop.width / kx, TILTED_HEIGHT / ky, prop.height / kz));
          place(art, model, matrix.multiply(new THREE.Matrix4().makeTranslation(0, -ky / 2, 0)));
          break;
        }
        const mesh = new THREE.Mesh(
          new THREE.BoxGeometry(prop.width, TILTED_HEIGHT, prop.height),
          new THREE.MeshLambertMaterial({ color: KIT_BOXES.get(BoxKind.CONTAINER_PALE) }),
        );
        mesh.position.copy(at);
        mesh.quaternion.copy(rotation);
        mesh.castShadow = true;
        mesh.receiveShadow = true;
        world.add(mesh);
        break;
      }
      case PropKind.FLOOR_LINE:
        lines.push(floorLine(prop));
        break;
      case PropKind.LAMP: {
        const art = propArt(PropKind[prop.kind]);
        const model = kitModel(PropKind[prop.kind], art);
        if (art && model) {
          const turn = new THREE.Quaternion().setFromAxisAngle(UP, prop.rotation);
          place(art, model, new THREE.Matrix4().compose(new THREE.Vector3(prop.x, prop.y, prop.z), turn, ONE));
          break;
        }
        const lamp = lampModel(prop.rotation);
        lamp.position.set(prop.x, prop.y, prop.z);
        world.add(lamp);
        break;
      }
      case PropKind.CEILING_LIGHT: {
        fixtures.push(new THREE.Matrix4().makeTranslation(prop.x, prop.y, prop.z));
        const light = new THREE.PointLight(0xffe2b4, lightPower, lightPower > 40 ? 34 : 16, 1.8);
        light.position.set(prop.x, prop.y - 0.3, prop.z);
        world.add(light);
        break;
      }
      default: {
        if (!PLANES.has(prop.kind)) break;
        const list = planes.get(prop.kind) ?? [];
        list.push(
          new THREE.Matrix4().compose(
            new THREE.Vector3(prop.x, prop.y, prop.z),
            new THREE.Quaternion().setFromAxisAngle(UP, prop.rotation),
            new THREE.Vector3(prop.width, prop.height, 1),
          ),
        );
        planes.set(prop.kind, list);
      }
    }
  }

  for (const [file, batch] of batches) world.add(...instanceKit(file, batch));
  const unitPlane = new THREE.PlaneGeometry(1, 1);
  for (const [kind, matrices] of planes) {
    const mesh = instances(PropKind[kind], unitPlane, new THREE.MeshLambertMaterial({ map: PLANES.get(kind)!() }), matrices);
    mesh.receiveShadow = true;
    world.add(mesh);
  }
  if (trusses.length) {
    const mesh = new THREE.Mesh(mergeGeometries(trusses), new THREE.MeshLambertMaterial({ color: 0x3b2a1c }));
    mesh.name = PropKind[PropKind.TRUSS];
    world.add(mesh);
  }
  if (lines.length) {
    const mesh = new THREE.Mesh(mergeGeometries(lines), new THREE.MeshLambertMaterial({ color: 0xc7a23a }));
    mesh.name = PropKind[PropKind.FLOOR_LINE];
    mesh.receiveShadow = true;
    world.add(mesh);
  }
  if (fixtures.length) {
    const fixture = new THREE.BoxGeometry(1.4, 0.08, 0.3);
    world.add(instances(PropKind[PropKind.CEILING_LIGHT], fixture, new THREE.MeshBasicMaterial({ color: 0xfff0cf }), fixtures));
  }

  const params = new URLSearchParams(location.search);
  if (params.has("debug") && params.has("collision")) world.add(collisionEdges(map.boxes));
  return world;
}

/**
 * Takes buildWorld's group out of the scene and frees what it made for the match on the GPU: its
 * geometry, materials and their textures, the instance buffers and the sun's shadow map. The kit
 * copies' geometry and materials are the art's, which only releaseGpu() (art/loader.ts) frees.
 */
export function disposeWorld(world: THREE.Group): void {
  world.removeFromParent();
  world.traverse((object) => {
    if (object instanceof THREE.DirectionalLight || object instanceof THREE.PointLight) object.shadow.dispose();
    if (object instanceof THREE.InstancedMesh) object.dispose();
    if (!(object instanceof THREE.Mesh || object instanceof THREE.Line) || sharedParts.has(object)) return;
    object.geometry.dispose();
    const materials: THREE.Material[] = Array.isArray(object.material) ? object.material : [object.material];
    for (const material of materials) {
      for (const value of Object.values(material)) if (value instanceof THREE.Texture) value.dispose();
      material.dispose();
    }
  });
}
