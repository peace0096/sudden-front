import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { BoxKind, MapId, PropKind, type MapBox, type MapData, type MapProp } from "../gen/game_pb";
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
    [BoxKind.CONTAINER_BLUE, { material: lambert(tex.containerTexture("#44627c", "rgba(16,30,44,.55)", "SFLU 2210")), tile: [6, 2.6] }],
    [BoxKind.CONTAINER_GRAY, { material: lambert(tex.containerTexture("#8d9598", "rgba(40,46,50,.5)", "YARD 0741")), tile: [6, 2.6] }],
    [BoxKind.CONTAINER_RUST, { material: lambert(tex.containerTexture("#8c4b30", "rgba(50,22,12,.55)", "CGO 5518")), tile: [6, 2.6] }],
    [BoxKind.CONTAINER_PALE, { material: lambert(tex.containerTexture("#9db0bc", "rgba(44,60,72,.42)", "WH 4410")), tile: [6, 2.6] }],
    [BoxKind.CRATE, { material: lambert(tex.crateTexture()), tile: [1.2, 1.2] }],
    [BoxKind.ROOF, { material: lambert(tex.roofTexture()), tile: [4, 4] }],
  ]);
}

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

function trussModel(span: number, height: number): THREE.Group {
  const truss = new THREE.Group();
  const wood = new THREE.MeshLambertMaterial({ color: 0x3b2a1c });
  truss.add(new THREE.Mesh(new THREE.BoxGeometry(0.28, 0.28, span), wood));
  for (const side of [-1, 1]) {
    const length = Math.hypot(span / 2, height);
    const rafter = new THREE.Mesh(new THREE.BoxGeometry(0.24, 0.24, length), wood);
    rafter.position.set(0, height / 2, (side * span) / 4);
    rafter.rotation.x = side * Math.atan2(height, span / 2);
    truss.add(rafter);
    for (const fraction of [0.25, 0.6]) {
      const post = new THREE.Mesh(new THREE.BoxGeometry(0.14, height * (1 - fraction), 0.14), wood);
      post.position.set(0, (height * (1 - fraction)) / 2, (side * span * fraction) / 2);
      truss.add(post);
    }
  }
  return truss;
}

function addProp(
  scene: THREE.Scene,
  prop: MapProp,
  textures: Map<PropKind, THREE.Texture>,
  table: Map<BoxKind, Surface>,
  lightPower: number,
): void {
  switch (prop.kind) {
    case PropKind.TRUSS: {
      const truss = trussModel(prop.width, prop.height);
      truss.position.set(prop.x, prop.y, prop.z);
      scene.add(truss);
      return;
    }
    case PropKind.TILTED_CONTAINER: {
      // Leans over at an angle, with its collision footprint authored on the server.
      const mesh = new THREE.Mesh(
        new THREE.BoxGeometry(prop.width, 2.6, prop.height),
        table.get(BoxKind.CONTAINER_PALE)!.material,
      );
      mesh.position.set(prop.x, prop.y, prop.z);
      mesh.rotation.set(0, prop.rotation, 0.34);
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      scene.add(mesh);
      return;
    }
    case PropKind.FLOOR_LINE: {
      const line = new THREE.Mesh(
        new THREE.PlaneGeometry(prop.width, prop.height),
        new THREE.MeshLambertMaterial({ color: 0xc7a23a }),
      );
      line.rotation.set(-Math.PI / 2, 0, prop.rotation);
      line.position.set(prop.x, prop.y, prop.z);
      line.receiveShadow = true;
      scene.add(line);
      return;
    }
    case PropKind.LAMP: {
      const lamp = lampModel(prop.rotation);
      lamp.position.set(prop.x, prop.y, prop.z);
      scene.add(lamp);
      return;
    }
    case PropKind.CEILING_LIGHT: {
      const fixture = new THREE.Mesh(
        new THREE.BoxGeometry(1.4, 0.08, 0.3),
        new THREE.MeshBasicMaterial({ color: 0xfff0cf }),
      );
      fixture.position.set(prop.x, prop.y, prop.z);
      const light = new THREE.PointLight(0xffe2b4, lightPower, lightPower > 40 ? 34 : 16, 1.8);
      light.position.set(prop.x, prop.y - 0.3, prop.z);
      scene.add(fixture, light);
      return;
    }
    default: {
      const map = textures.get(prop.kind);
      if (!map) return;
      const plane = new THREE.Mesh(
        new THREE.PlaneGeometry(prop.width, prop.height),
        new THREE.MeshLambertMaterial({ map }),
      );
      plane.position.set(prop.x, prop.y, prop.z);
      plane.rotation.y = prop.rotation;
      plane.receiveShadow = true;
      scene.add(plane);
    }
  }
}

function addGround(scene: THREE.Scene, map: MapData): void {
  const indoor = map.id === MapId.WAREHOUSE;
  const surface = indoor ? tex.concreteTexture() : tex.sandTexture();
  surface.repeat.set(map.halfX / (indoor ? 2 : 6), map.halfZ / (indoor ? 2 : 6));
  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(map.halfX * 2 + 4, map.halfZ * 2 + 4),
    new THREE.MeshLambertMaterial({ map: surface }),
  );
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);

  if (indoor) return;

  // Warehouses get a concrete floor under their roof.
  const concrete = tex.concreteTexture();
  for (const roof of map.boxes.filter((b) => b.kind === BoxKind.ROOF)) {
    const width = roof.maxX - roof.minX - 1;
    const depth = roof.maxZ - roof.minZ - 1;
    const floorTexture = concrete.clone();
    floorTexture.repeat.set(width / 3, depth / 3);
    floorTexture.needsUpdate = true;
    const floor = new THREE.Mesh(
      new THREE.PlaneGeometry(width, depth),
      new THREE.MeshLambertMaterial({ map: floorTexture }),
    );
    floor.rotation.x = -Math.PI / 2;
    floor.position.set((roof.minX + roof.maxX) / 2, 0.012, (roof.minZ + roof.maxZ) / 2);
    floor.receiveShadow = true;
    scene.add(floor);
  }
}

export function buildWorld(scene: THREE.Scene, map: MapData): void {
  const sky = new THREE.Mesh(
    new THREE.SphereGeometry(400, 24, 16),
    new THREE.MeshBasicMaterial({ map: tex.skyTexture(), side: THREE.BackSide, fog: false }),
  );
  scene.add(sky);
  const indoor = map.id === MapId.WAREHOUSE;
  scene.fog = indoor ? new THREE.Fog(0x2a2622, 40, 160) : new THREE.Fog(0xcdd6d4, 70, 260);

  scene.add(
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
  scene.add(sun, sun.target);

  addGround(scene, map);

  const table = surfaces();
  const grouped = new Map<BoxKind, THREE.BufferGeometry[]>();
  const barrels: THREE.BufferGeometry[] = [];
  for (const b of map.boxes) {
    if (b.kind === BoxKind.BARREL) {
      const geometry = new THREE.CylinderGeometry(
        (b.maxX - b.minX) / 2,
        (b.maxX - b.minX) / 2,
        b.maxY - b.minY,
        14,
      );
      geometry.translate((b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2, (b.minZ + b.maxZ) / 2);
      barrels.push(geometry);
      continue;
    }
    const surface = table.get(b.kind);
    if (!surface) continue;
    const list = grouped.get(b.kind) ?? [];
    list.push(boxGeometry(b, surface.tile));
    grouped.set(b.kind, list);
  }
  for (const [kind, geometries] of grouped) {
    const mesh = new THREE.Mesh(mergeGeometries(geometries), table.get(kind)!.material);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    scene.add(mesh);
  }
  if (barrels.length) {
    const mesh = new THREE.Mesh(
      mergeGeometries(barrels),
      new THREE.MeshLambertMaterial({ map: tex.barrelTexture() }),
    );
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    scene.add(mesh);
  }

  const propTextures = new Map<PropKind, THREE.Texture>([
    [PropKind.SHUTTER, tex.shutterTexture()],
    [PropKind.BILLBOARD, tex.billboardTexture()],
    [PropKind.WINDOW, tex.windowTexture()],
    [PropKind.LOUVER, tex.louverTexture()],
  ]);
  const lightPower = indoor ? 62 : 32;
  for (const prop of map.props) addProp(scene, prop, propTextures, table, lightPower);
}
