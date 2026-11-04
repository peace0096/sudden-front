// Marks left on the map: bullet holes, blood splats and scorch marks. Each lies flat on a face of
// one of the server's collision boxes, which are all axis-aligned, so a face's normal is an axis;
// or on the ground. A mark is multiplied into what is drawn under it, so it darkens the surface in
// whatever light falls there. Up to 128 at once in one draw call, in two rings: bullet holes, and
// the bigger marks (blood, scorch), so a long firefight's holes don't wipe out the rest. A mark is
// shrunk or moved to stay on its face rather than hang in the air past an edge. The functions
// below find the face a shot struck or an explosion went off next to, from the same boxes.
import * as THREE from "three";
import { BoxKind, type MapBox } from "../gen/game_pb";
import { rayBox } from "./movement";
import type { Atlas, UvRect } from "./particles";

/** A face to put a mark on. Returned by the functions below and valid until their next call. */
export interface Surface {
  /** The box whose face it is; null for the ground. */
  box: MapBox | null;
  /** The face's outward normal: an axis (0 x, 1 y, 2 z) and its sign. */
  axis: number;
  sign: number;
  /** A point on the face. */
  point: THREE.Vector3;
}

/** Boxes that take no marks: a barrel is round inside its box, and an invisible box has no face to see. */
function marksSkip(box: MapBox | null): boolean {
  return !!box && (box.kind === BoxKind.BARREL || box.kind === BoxKind.INVISIBLE);
}

const found: Surface = { box: null, axis: 1, sign: 1, point: new THREE.Vector3() };
const origin: [number, number, number] = [0, 0, 0];
const direction: [number, number, number] = [0, 0, 0];
const low: [number, number, number] = [0, 0, 0];
const high: [number, number, number] = [0, 0, 0];

const lower = (b: MapBox, axis: number) => (axis === 0 ? b.minX : axis === 1 ? b.minY : b.minZ);
const upper = (b: MapBox, axis: number) => (axis === 0 ? b.maxX : axis === 1 ? b.maxY : b.maxZ);

function face(b: MapBox, axis: number, sign: number, point: THREE.Vector3): Surface {
  found.box = b;
  found.axis = axis;
  found.sign = sign;
  found.point.copy(point).setComponent(axis, sign < 0 ? lower(b, axis) : upper(b, axis));
  return found;
}

function ground(point: THREE.Vector3): Surface {
  found.box = null;
  found.axis = 1;
  found.sign = 1;
  found.point.set(point.x, 0, point.z);
  return found;
}

/**
 * The face a ray from `from` along the unit `dir` meets first within `reach` metres, among the
 * server's own boxes, so a shot's ray finds the face the server's met; null if none.
 */
export function firstFace(boxes: MapBox[], from: THREE.Vector3, dir: THREE.Vector3, reach: number): Surface | null {
  from.toArray(origin);
  dir.toArray(direction);
  let best: MapBox | null = null;
  let bestT = reach;
  for (const b of boxes) {
    low[0] = b.minX;
    low[1] = b.minY;
    low[2] = b.minZ;
    high[0] = b.maxX;
    high[1] = b.maxY;
    high[2] = b.maxZ;
    const t = rayBox(origin, direction, low, high);
    if (t !== null && t <= bestT) {
      bestT = t;
      best = b;
    }
  }
  if (!best) return null;
  // Of the faces the ray can enter by, the one the point is on.
  const at = found.point.copy(dir).multiplyScalar(bestT).add(from);
  let axis = 1;
  let sign = 1;
  let gap = Infinity;
  for (let a = 0; a < 3; a++) {
    const d = dir.getComponent(a);
    if (Math.abs(d) < 1e-6) continue;
    const s = d > 0 ? -1 : 1;
    const g = Math.abs(at.getComponent(a) - (s < 0 ? lower(best, a) : upper(best, a)));
    if (g < gap) {
      gap = g;
      axis = a;
      sign = s;
    }
  }
  return face(best, axis, sign, at);
}

/** How far before a shot's end its ray is cast again to find the face it struck. */
const BACKUP = 0.5;
const backedUp = new THREE.Vector3();

/**
 * The surface a shot moving along the unit `dir` struck at `point`: a box face, or the ground where
 * the shot ended under it (the server's ray leaves the ground out); null in the open.
 */
export function struckSurface(boxes: MapBox[], point: THREE.Vector3, dir: THREE.Vector3): Surface | null {
  const hit = firstFace(boxes, backedUp.copy(dir).multiplyScalar(-BACKUP).add(point), dir, BACKUP + 0.1);
  if (hit) return hit;
  return point.y < 0.05 && dir.y < 0 ? ground(point) : null;
}

/** The face `point` lies on (within 3 cm), when the way the shot came is unknown; null in the open. */
export function touchingSurface(boxes: MapBox[], point: THREE.Vector3): Surface | null {
  const margin = 0.03;
  let best: MapBox | null = null;
  let axis = 1;
  let sign = 1;
  let gap = margin;
  for (const b of boxes) {
    if (point.x < b.minX - margin || point.x > b.maxX + margin) continue;
    if (point.y < b.minY - margin || point.y > b.maxY + margin) continue;
    if (point.z < b.minZ - margin || point.z > b.maxZ + margin) continue;
    for (let a = 0; a < 3; a++) {
      const p = point.getComponent(a);
      for (let s = -1; s <= 1; s += 2) {
        const g = Math.abs(p - (s < 0 ? lower(b, a) : upper(b, a)));
        if (g <= gap) {
          gap = g;
          best = b;
          axis = a;
          sign = s;
        }
      }
    }
  }
  if (best) return face(best, axis, sign, point);
  return point.y < margin ? ground(point) : null;
}

/**
 * The surface nearest `point` within `reach` metres, the ground included, for the scorch of an
 * explosion (the server sets one off just short of what the missile hit); boxes that take no
 * marks are passed over.
 */
export function nearestSurface(boxes: MapBox[], point: THREE.Vector3, reach: number): Surface | null {
  let best: MapBox | null = null;
  let bestDistance = point.y >= 0 && point.y <= reach ? point.y : Infinity;
  let axis = 1;
  let sign = 1;
  for (const b of boxes) {
    if (marksSkip(b)) continue;
    // How far outside the box the point is along each axis.
    let distance = 0;
    let outAxis = -1;
    let outSign = 1;
    let most = 0;
    for (let a = 0; a < 3; a++) {
      const p = point.getComponent(a);
      const below = lower(b, a) - p;
      const above = p - upper(b, a);
      const out = Math.max(below, above, 0);
      distance += out * out;
      if (out > most) {
        most = out;
        outAxis = a;
        outSign = above > 0 ? 1 : -1;
      }
    }
    distance = Math.sqrt(distance);
    if (distance > reach || distance >= bestDistance) continue;
    if (outAxis < 0) {
      // Inside: the nearest of its faces.
      let gap = Infinity;
      for (let a = 0; a < 3; a++) {
        const p = point.getComponent(a);
        for (let s = -1; s <= 1; s += 2) {
          const g = Math.abs(p - (s < 0 ? lower(b, a) : upper(b, a)));
          if (g < gap) {
            gap = g;
            outAxis = a;
            outSign = s;
          }
        }
      }
    }
    best = b;
    bestDistance = distance;
    axis = outAxis;
    sign = outSign;
  }
  if (!best) return bestDistance < Infinity ? ground(point) : null;
  const on = found.point.copy(point);
  on.set(
    THREE.MathUtils.clamp(on.x, best.minX, best.maxX),
    THREE.MathUtils.clamp(on.y, best.minY, best.maxY),
    THREE.MathUtils.clamp(on.z, best.minZ, best.maxZ),
  );
  return face(best, axis, sign, on);
}

const DECAL_VERTEX = /* glsl */ `
#include <common>
#include <fog_pars_vertex>
// Centre and which atlas (0 the decals', 1 the particles'); the quad's x and y edges in the world;
// its cell (u, v, width, height); what it multiplies the surface by (rgb) and how solid it is.
attribute vec4 aCentre;
attribute vec3 aRight;
attribute vec3 aUp;
attribute vec4 aCell;
attribute vec4 aColour;
varying vec2 vUv;
varying vec4 vColour;
varying float vAtlas;

void main() {
  vUv = aCell.xy + uv * aCell.zw;
  vColour = aColour;
  vAtlas = aCentre.w;
  vec4 mvPosition = modelViewMatrix * vec4(aCentre.xyz + aRight * position.x + aUp * position.y, 1.0);
  gl_Position = projectionMatrix * mvPosition;
  #include <fog_vertex>
}
`;

const DECAL_FRAGMENT = /* glsl */ `
#include <common>
#include <fog_pars_fragment>
uniform sampler2D uDecals;
uniform sampler2D uParticles;
varying vec2 vUv;
varying vec4 vColour;
varying float vAtlas;

void main() {
  float alpha = mix(texture2D(uDecals, vUv).a, texture2D(uParticles, vUv).a, vAtlas) * vColour.a;
  vec3 tint = vColour.rgb;
  #ifdef USE_FOG
    #ifdef FOG_EXP2
      float fogFactor = 1.0 - exp(-fogDensity * fogDensity * vFogDepth * vFogDepth);
    #else
      float fogFactor = smoothstep(fogNear, fogFar, vFogDepth);
    #endif
    tint = mix(tint, vec3(1.0), fogFactor);
  #endif
  // Premultiplied for multiply blending: the surface becomes surface × mix(1, tint, alpha).
  gl_FragColor = vec4(tint * alpha, alpha);
}
`;

export interface DecalSpec {
  cell: UvRect;
  /** Which atlas the cell is in: the decals' (0) or the particles' (1). */
  atlas: number;
  /** Width and height in metres, and the turn about the face's normal. */
  size: number;
  rotation: number;
  /** What the surface is multiplied by where the mark is solid (display RGB, 0 to 1), and how solid. */
  r: number;
  g: number;
  b: number;
  alpha: number;
  /** HOLES (shrunk to keep off the face's edges) or MARKS (moved instead). */
  lane: number;
}

export const HOLES = 0;
export const MARKS = 1;

/** What the bench measures of a mark. */
export interface DecalInfo {
  centre: number[];
  normal: number[];
  size: number;
}

/** Models fill their boxes only to within a centimetre, so their marks sit this far out. */
const MODEL_LIFT = 0.012;
const MODEL_KINDS = new Set([BoxKind.CONTAINER_BLUE, BoxKind.CONTAINER_GRAY, BoxKind.CONTAINER_RUST, BoxKind.CONTAINER_PALE, BoxKind.CRATE]);
/** Over the floors world.ts lays a little above the ground. */
const GROUND_LIFT = 0.02;
/** A hole that would have to be smaller than this to stay on its face is left out. */
const SMALLEST_HOLE = 0.04;

const right = new THREE.Vector3();
const up = new THREE.Vector3();
const turnedRight = new THREE.Vector3();
const turnedUp = new THREE.Vector3();
const centre = new THREE.Vector3();

export class DecalPool {
  readonly capacity: number;
  readonly mesh: THREE.Mesh<THREE.InstancedBufferGeometry, THREE.ShaderMaterial>;
  private readonly lanes: { start: number; size: number; next: number; filled: number }[];
  private readonly centres: Float32Array;
  private readonly rights: Float32Array;
  private readonly ups: Float32Array;
  private readonly cells: Float32Array;
  private readonly colours: Float32Array;
  private readonly attributes: THREE.InstancedBufferAttribute[] = [];

  constructor(
    private readonly decals: Atlas,
    private readonly particles: Atlas,
    holes = 96,
    marks = 32,
  ) {
    this.capacity = holes + marks;
    this.lanes = [
      { start: 0, size: holes, next: 0, filled: 0 },
      { start: holes, size: marks, next: 0, filled: 0 },
    ];
    const plane = new THREE.PlaneGeometry(1, 1);
    const geometry = new THREE.InstancedBufferGeometry();
    geometry.setIndex(plane.getIndex());
    geometry.setAttribute("position", plane.getAttribute("position"));
    geometry.setAttribute("uv", plane.getAttribute("uv"));
    const instanced = (name: string, size: number) => {
      // Zero edges: a slot not yet written draws nothing.
      const array = new Float32Array(this.capacity * size);
      const attribute = new THREE.InstancedBufferAttribute(array, size).setUsage(THREE.DynamicDrawUsage);
      geometry.setAttribute(name, attribute);
      this.attributes.push(attribute);
      return array;
    };
    this.centres = instanced("aCentre", 4);
    this.rights = instanced("aRight", 3);
    this.ups = instanced("aUp", 3);
    this.cells = instanced("aCell", 4);
    this.colours = instanced("aColour", 4);
    geometry.instanceCount = 0;
    const material = new THREE.ShaderMaterial({
      uniforms: {
        ...THREE.UniformsUtils.clone(THREE.UniformsLib.fog),
        uDecals: { value: decals.texture },
        uParticles: { value: particles.texture },
      },
      vertexShader: DECAL_VERTEX,
      fragmentShader: DECAL_FRAGMENT,
      transparent: true,
      depthWrite: false,
      blending: THREE.MultiplyBlending,
      premultipliedAlpha: true,
      polygonOffset: true,
      polygonOffsetFactor: -1,
      polygonOffsetUnits: -4,
      fog: true,
    });
    this.mesh = new THREE.Mesh(geometry, material);
    this.mesh.frustumCulled = false;
    this.mesh.matrixAutoUpdate = false;
    // On the surfaces, under everything else see-through.
    this.mesh.renderOrder = -1;
  }

  /** Marks present. */
  get count(): number {
    return this.lanes.reduce((sum, lane) => sum + lane.filled, 0);
  }

  /** Puts a mark on a surface, in place of its lane's oldest when the lane is full; false if it can't fit. */
  add(surface: Surface, spec: Readonly<DecalSpec>): boolean {
    if (marksSkip(surface.box) || !(spec.size > 0)) return false;
    const { box, axis, sign } = surface;
    // The quad's edges, turned about the normal, with right × up along the normal.
    if (axis === 0) {
      right.set(0, 0, -sign);
      up.set(0, 1, 0);
    } else if (axis === 1) {
      right.set(1, 0, 0);
      up.set(0, 0, -sign);
    } else {
      right.set(sign, 0, 0);
      up.set(0, 1, 0);
    }
    const c = Math.cos(spec.rotation);
    const s = Math.sin(spec.rotation);
    turnedRight.copy(right).multiplyScalar(c).addScaledVector(up, s);
    turnedUp.copy(up).multiplyScalar(c).addScaledVector(right, -s);
    // Half its reach along the face's own axes, turned as it is.
    const spread = (Math.abs(c) + Math.abs(s)) / 2;
    let size = spec.size;
    centre.copy(surface.point);
    if (box) {
      for (let a = 0; a < 3; a++) {
        if (a === axis) continue;
        const min = lower(box, a);
        const max = upper(box, a);
        const p = centre.getComponent(a);
        if (spec.lane === HOLES) {
          // A hole stays where the shot struck, smaller near an edge.
          size = Math.min(size, Math.max(0, Math.min(p - min, max - p)) / spread);
        } else {
          size = Math.min(size, (max - min) / (2 * spread));
          const reach = size * spread;
          centre.setComponent(a, THREE.MathUtils.clamp(p, min + reach, max - reach));
        }
      }
      if (size < (spec.lane === HOLES ? SMALLEST_HOLE : spec.size / 4)) return false;
    }
    const lift = box ? (MODEL_KINDS.has(box.kind) ? MODEL_LIFT : 0) : GROUND_LIFT;
    centre.setComponent(axis, centre.getComponent(axis) + sign * lift);

    const lane = this.lanes[spec.lane];
    const slot = lane.start + lane.next;
    lane.next = (lane.next + 1) % lane.size;
    lane.filled = Math.min(lane.size, lane.filled + 1);
    centre.toArray(this.centres, slot * 4);
    this.centres[slot * 4 + 3] = spec.atlas;
    turnedRight.multiplyScalar(size).toArray(this.rights, slot * 3);
    turnedUp.multiplyScalar(size).toArray(this.ups, slot * 3);
    this.cells.set(spec.cell, slot * 4);
    this.colours[slot * 4] = spec.r;
    this.colours[slot * 4 + 1] = spec.g;
    this.colours[slot * 4 + 2] = spec.b;
    this.colours[slot * 4 + 3] = spec.alpha;
    const geometry = this.mesh.geometry;
    geometry.instanceCount = Math.max(geometry.instanceCount, slot + 1);
    // Only the slot written goes up; three sends every range added since the last draw, then
    // drops them. They only pile up while the pool isn't drawn, and past one a slot a range over
    // the whole buffer takes their place.
    for (const attribute of this.attributes) {
      if (attribute.updateRanges.length < this.capacity) attribute.addUpdateRange(slot * attribute.itemSize, attribute.itemSize);
      else {
        attribute.clearUpdateRanges();
        attribute.addUpdateRange(0, attribute.array.length);
      }
      attribute.needsUpdate = true;
    }
    return true;
  }

  /** Every mark present: its centre, the normal of the face it lies on and its size. */
  list(): DecalInfo[] {
    const marks: DecalInfo[] = [];
    for (const lane of this.lanes) {
      for (let slot = lane.start; slot < lane.start + lane.filled; slot++) {
        const r = new THREE.Vector3().fromArray(this.rights, slot * 3);
        const u = new THREE.Vector3().fromArray(this.ups, slot * 3);
        marks.push({
          centre: Array.from(this.centres.subarray(slot * 4, slot * 4 + 3)),
          normal: r.clone().cross(u).normalize().toArray(),
          size: r.length(),
        });
      }
    }
    return marks;
  }

  dispose(): void {
    this.mesh.removeFromParent();
    this.mesh.geometry.dispose();
    this.mesh.material.dispose();
    for (const atlas of [this.decals, this.particles]) if (atlas.fallback) atlas.texture.dispose();
  }
}
