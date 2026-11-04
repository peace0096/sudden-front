// Pooled, batched effect particles. A batch draws everything it holds as instances of one quad in a
// single draw call (none while empty), and simulates it here on the CPU (velocity, gravity, drag,
// growth, colour and fading) in arrays sized once, so effects allocate nothing as they come and
// go. A quad is a billboard, turned to face the camera with a spin of its own, or a streak: a
// ribbon between two points that turns about its line to face the camera, for flames, sparks,
// blood drops and tracers. Its shape is a cell of an effect atlas (white, the shape in its alpha)
// in the quad's colour, or for a streak a glowing line worked out in the shader, which stays
// smooth at any width. Widths are kept within a range of pixels, so a far tracer stays visible and
// a near one never fills the view. One batch blends normally (smoke, dust, blood) and another adds
// light (flashes, fire, sparks); fog fades the first into the fog colour and the second to nothing.
import * as THREE from "three";
import { loadedArt } from "./art/loader";
import { fxArt, type AtlasArt } from "./art/manifest";

/** [u, v, width, height] of an atlas cell in texture coordinates, v from the bottom. */
export type UvRect = readonly [number, number, number, number];

/** A streak's glowing line, worked out in the shader instead of read from a cell. */
export const GLOW: UvRect = [0, 0, -1, 1];
const WHOLE: UvRect = [0, 0, 1, 1];

export interface Atlas {
  texture: THREE.Texture;
  /** Whether the texture is the stand-in made here, which its users free; the art's is shared. */
  fallback: boolean;
  /** A named cell; the whole texture for a name the atlas lacks. */
  cell(name: string): UvRect;
}

let softDot: THREE.DataTexture | null = null;

/** A white dot fading out to its edge: what every cell becomes when an atlas is missing. */
function fallbackTexture(): THREE.DataTexture {
  if (softDot) return softDot;
  const size = 32;
  const data = new Uint8Array(size * size * 4).fill(255);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const d = Math.hypot(x + 0.5 - size / 2, y + 0.5 - size / 2) / (size / 2);
      data[(y * size + x) * 4 + 3] = Math.round(255 * Math.max(0, 1 - d) ** 1.5);
    }
  }
  softDot = new THREE.DataTexture(data, size, size);
  softDot.magFilter = THREE.LinearFilter;
  softDot.minFilter = THREE.LinearFilter;
  softDot.needsUpdate = true;
  return softDot;
}

/** Atlases already said to be missing: a page warns once for each, not once a match. */
const missing = new Set<string>();

function atlasFrom(art: AtlasArt | undefined, what: string): Atlas {
  const set = loadedArt();
  const texture = art && set?.textures.get(art.file);
  const image = texture?.image as { width?: number; height?: number } | undefined;
  if (!art || !texture || !image?.width || !image.height) {
    // A file that failed to load was reported by the loader; anything else is reported here.
    const reported = !!art && !!set?.failures.some((failure) => failure.file === art.file);
    const key = art?.file ?? what;
    if (!reported && !missing.has(key)) {
      missing.add(key);
      console.warn(`[art] ${art ? `${art.file} is not loaded` : `manifest.json has no ${what} atlas`}: effects drawn as soft dots`);
    }
    return { texture: fallbackTexture(), fallback: true, cell: () => WHOLE };
  }
  const { width, height } = image as { width: number; height: number };
  const cells = new Map<string, UvRect>();
  for (const [name, [x, y, w, h]] of Object.entries(art.cells)) cells.set(name, [x / width, 1 - (y + h) / height, w / width, h / height]);
  return { texture, fallback: false, cell: (name) => cells.get(name) ?? WHOLE };
}

/** The particle atlas (manifest fx). The art must have finished loading. */
export function particleAtlas(): Atlas {
  return atlasFrom(fxArt(), "particle");
}

/** The decal atlas (manifest fx.decals). The art must have finished loading. */
export function decalAtlas(): Atlas {
  return atlasFrom(fxArt()?.decals, "decal");
}

const QUAD_VERTEX = /* glsl */ `
#include <common>
#include <fog_pars_vertex>
// Pixels across one metre at one metre's depth, and the narrowest and widest a quad is drawn.
uniform float uPixelsPerUnit;
uniform vec2 uPixelClamp;
// Centre and width (m); unit axis and half length (m), 0 for a billboard; colour (linear rgb, a);
// cell (u, v, width, height), width below 0 for a streak's glow; a billboard's turn (rad).
attribute vec4 aCentre;
attribute vec4 aAxis;
attribute vec4 aColour;
attribute vec4 aCell;
attribute float aRotation;
varying vec2 vUv;
varying vec4 vColour;
varying float vGlow;

float clampedWidth(float width, float depth) {
  float pixels = width * uPixelsPerUnit / max(depth, 1e-3);
  return width * clamp(pixels, uPixelClamp.x, uPixelClamp.y) / max(pixels, 1e-6);
}

void main() {
  vGlow = aCell.z < 0.0 ? 1.0 : 0.0;
  vUv = vGlow > 0.5 ? uv : aCell.xy + uv * aCell.zw;
  vColour = aColour;
  vec3 centre = (modelViewMatrix * vec4(aCentre.xyz, 1.0)).xyz;
  vec3 point;
  if (aAxis.w > 0.0) {
    // A ribbon from tail (uv.y 0) to head (uv.y 1), its width across the line of sight; this way
    // round the quad keeps its front to the camera.
    vec3 halfAxis = (modelViewMatrix * vec4(aAxis.xyz, 0.0)).xyz * aAxis.w;
    point = centre + halfAxis * (2.0 * position.y);
    vec3 across = cross(point, halfAxis);
    float reach = length(across);
    across = reach > 1e-9 ? across / reach : vec3(1.0, 0.0, 0.0);
    point += across * position.x * clampedWidth(aCentre.w, -point.z);
  } else {
    float c = cos(aRotation);
    float s = sin(aRotation);
    vec2 corner = vec2(c * position.x - s * position.y, s * position.x + c * position.y);
    point = centre + vec3(corner * clampedWidth(aCentre.w, -centre.z), 0.0);
  }
  vec4 mvPosition = vec4(point, 1.0);
  gl_Position = projectionMatrix * mvPosition;
  #include <fog_vertex>
}
`;

const QUAD_FRAGMENT = /* glsl */ `
#include <common>
#include <fog_pars_fragment>
uniform sampler2D uMap;
varying vec2 vUv;
varying vec4 vColour;
varying float vGlow;

void main() {
  float shape = texture2D(uMap, vUv).a;
  // A streak's glow: brightest on its middle line, fading back along its tail from a sharp head.
  float across = smoothstep(0.0, 1.0, 1.0 - abs(vUv.x * 2.0 - 1.0));
  float glow = across * smoothstep(0.0, 1.0, vUv.y) * (1.0 - smoothstep(0.92, 1.0, vUv.y));
  float alpha = mix(shape, glow, vGlow) * vColour.a;
  vec3 colour = vColour.rgb;
  #ifdef USE_FOG
    #ifdef FOG_EXP2
      float fogFactor = 1.0 - exp(-fogDensity * fogDensity * vFogDepth * vFogDepth);
    #else
      float fogFactor = smoothstep(fogNear, fogFar, vFogDepth);
    #endif
    #ifdef ADDITIVE
      colour *= 1.0 - fogFactor;
    #else
      colour = mix(colour, fogColor, fogFactor);
    #endif
  #endif
  gl_FragColor = vec4(colour, alpha);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}
`;

export interface BatchOptions {
  capacity: number;
  /** Adds its light instead of blending over what is behind it. */
  additive: boolean;
  /** Drawn over everything without a depth test: the first-person flash. */
  overlay?: boolean;
  /** The narrowest and widest a quad is drawn, in CSS pixels. */
  pixels?: readonly [number, number];
  renderOrder?: number;
}

const viewport = new THREE.Vector4();

/** The GPU side of a batch: one quad, instanced up to `capacity` times, one draw call. */
class Quads {
  readonly mesh: THREE.Mesh<THREE.InstancedBufferGeometry, THREE.ShaderMaterial>;
  private readonly centre: Float32Array;
  private readonly axis: Float32Array;
  private readonly colour: Float32Array;
  private readonly cell: Float32Array;
  private readonly rotation: Float32Array;
  private readonly attributes: THREE.InstancedBufferAttribute[] = [];

  constructor(
    readonly capacity: number,
    atlas: Atlas,
    options: BatchOptions,
  ) {
    const plane = new THREE.PlaneGeometry(1, 1);
    const geometry = new THREE.InstancedBufferGeometry();
    geometry.setIndex(plane.getIndex());
    geometry.setAttribute("position", plane.getAttribute("position"));
    geometry.setAttribute("uv", plane.getAttribute("uv"));
    const instanced = (name: string, size: number) => {
      const array = new Float32Array(capacity * size);
      const attribute = new THREE.InstancedBufferAttribute(array, size).setUsage(THREE.DynamicDrawUsage);
      geometry.setAttribute(name, attribute);
      this.attributes.push(attribute);
      return array;
    };
    this.centre = instanced("aCentre", 4);
    this.axis = instanced("aAxis", 4);
    this.colour = instanced("aColour", 4);
    this.cell = instanced("aCell", 4);
    this.rotation = instanced("aRotation", 1);
    geometry.instanceCount = 0;

    const [least, most] = options.pixels ?? [0, 1e5];
    const material = new THREE.ShaderMaterial({
      uniforms: {
        ...THREE.UniformsUtils.clone(THREE.UniformsLib.fog),
        uMap: { value: atlas.texture },
        uPixelsPerUnit: { value: 500 },
        uPixelClamp: { value: new THREE.Vector2(least, most) },
      },
      vertexShader: QUAD_VERTEX,
      fragmentShader: QUAD_FRAGMENT,
      defines: options.additive ? { ADDITIVE: "" } : {},
      transparent: true,
      depthWrite: false,
      depthTest: !options.overlay,
      blending: options.additive ? THREE.AdditiveBlending : THREE.NormalBlending,
      fog: true,
    });
    this.mesh = new THREE.Mesh(geometry, material);
    // The quads are placed in the shader, so the mesh's own bounds mean nothing.
    this.mesh.frustumCulled = false;
    this.mesh.matrixAutoUpdate = false;
    this.mesh.renderOrder = options.renderOrder ?? 0;
    const pixelsPerUnit = material.uniforms.uPixelsPerUnit;
    this.mesh.onBeforeRender = (renderer, _scene, camera) => {
      renderer.getCurrentViewport(viewport);
      pixelsPerUnit.value = (camera.projectionMatrix.elements[5] * viewport.w) / (2 * renderer.getPixelRatio());
    };
  }

  /**
   * Quad i: a billboard turned by `rotation` when `half` is 0, else a streak from the centre `half`
   * metres each way along the unit axis. Its cell is the four numbers from `cells[cell]` on.
   */
  put(i: number, x: number, y: number, z: number, width: number, ax: number, ay: number, az: number, half: number, r: number, g: number, b: number, a: number, cells: ArrayLike<number>, cell: number, rotation: number): void {
    const o = i * 4;
    this.centre[o] = x;
    this.centre[o + 1] = y;
    this.centre[o + 2] = z;
    this.centre[o + 3] = width;
    this.axis[o] = ax;
    this.axis[o + 1] = ay;
    this.axis[o + 2] = az;
    this.axis[o + 3] = half;
    this.colour[o] = r;
    this.colour[o + 1] = g;
    this.colour[o + 2] = b;
    this.colour[o + 3] = a;
    this.cell[o] = cells[cell];
    this.cell[o + 1] = cells[cell + 1];
    this.cell[o + 2] = cells[cell + 2];
    this.cell[o + 3] = cells[cell + 3];
    this.rotation[i] = rotation;
  }

  /** Draws the first `count` quads from now on, and uploads only those. */
  commit(count: number): void {
    this.mesh.geometry.instanceCount = count;
    // Nothing to draw, nothing to send; and a range of length 0 would send the whole buffer.
    if (count === 0) return;
    for (const attribute of this.attributes) {
      attribute.clearUpdateRanges();
      attribute.addUpdateRange(0, count * attribute.itemSize);
      attribute.needsUpdate = true;
    }
  }

  dispose(atlas: Atlas): void {
    this.mesh.removeFromParent();
    this.mesh.geometry.dispose();
    this.mesh.material.dispose();
    if (atlas.fallback) atlas.texture.dispose();
  }
}

/** One particle to spawn: a still billboard unless the fields below say otherwise. */
export interface ParticleSpec {
  cell: UvRect;
  x: number;
  y: number;
  z: number;
  /** Velocity, m/s. */
  vx: number;
  vy: number;
  vz: number;
  /** Downward acceleration, m/s²; below 0 it rises. */
  gravity: number;
  /** How fast its velocity dies away, 1/s. */
  drag: number;
  /** Seconds. */
  life: number;
  /** Width in metres at birth and at death; it grows fast at first, then slows. */
  size: number;
  endSize: number;
  /** A billboard's turn and how fast it turns, rad and rad/s. */
  rotation: number;
  spin: number;
  /** Linear RGB at birth (above 1 for bright additive light) and at death; alpha at birth. */
  r: number;
  g: number;
  b: number;
  alpha: number;
  endR: number;
  endG: number;
  endB: number;
  /** Share of its life it takes to fade in. */
  fadeIn: number;
  /** Its alpha falls as 1 − age^fadeOut over its life: 1 is even, higher holds longer. */
  fadeOut: number;
  /** A streak trailing its motion, as long as it travels in this many seconds (0: none). */
  stretch: number;
  /**
   * Otherwise a streak this many metres long (0: none), from its position along the axis; as it
   * grows it lengthens as much as it widens.
   */
  length: number;
  ax: number;
  ay: number;
  az: number;
}

/** A spec with every field at its default, to fill in and spawn as often as needed. */
export function particleSpec(): ParticleSpec {
  return resetSpec(
    {
      cell: WHOLE,
      x: 0,
      y: 0,
      z: 0,
      vx: 0,
      vy: 0,
      vz: 0,
      gravity: 0,
      drag: 0,
      life: 1,
      size: 1,
      endSize: 1,
      rotation: 0,
      spin: 0,
      r: 1,
      g: 1,
      b: 1,
      alpha: 1,
      endR: 1,
      endG: 1,
      endB: 1,
      fadeIn: 0,
      fadeOut: 1,
      stretch: 0,
      length: 0,
      ax: 0,
      ay: 1,
      az: 0,
    },
    WHOLE,
    0,
    0,
    0,
    1,
  );
}

/** Puts a spec back to a still, white, unchanging billboard of `cell` at a point, living `life` s. */
export function resetSpec(spec: ParticleSpec, cell: UvRect, x: number, y: number, z: number, life: number): ParticleSpec {
  spec.cell = cell;
  spec.x = x;
  spec.y = y;
  spec.z = z;
  spec.vx = spec.vy = spec.vz = 0;
  spec.gravity = spec.drag = 0;
  spec.life = life;
  spec.size = spec.endSize = 1;
  spec.rotation = spec.spin = 0;
  spec.r = spec.g = spec.b = spec.alpha = 1;
  spec.endR = spec.endG = spec.endB = 1;
  spec.fadeIn = 0;
  spec.fadeOut = 1;
  spec.stretch = spec.length = 0;
  spec.ax = spec.az = 0;
  spec.ay = 1;
  return spec;
}

// A particle's state: one stride of floats each, kept dense (the last one fills a dead one's place).
const PX = 0;
const PY = 1;
const PZ = 2;
const VX = 3;
const VY = 4;
const VZ = 5;
const AX = 6;
const AY = 7;
const AZ = 8;
const LENGTH = 9;
const STRETCH = 10;
const AGE = 11;
const LIFE = 12;
const GRAVITY = 13;
const DRAG = 14;
const SIZE = 15;
const END_SIZE = 16;
const ROTATION = 17;
const SPIN = 18;
const FADE_IN = 19;
const FADE_OUT = 20;
const R = 21;
const G = 22;
const B = 23;
const A = 24;
const END_R = 25;
const END_G = 26;
const END_B = 27;
const CELL = 28;
const STRIDE = 32;

export class ParticleBatch {
  readonly capacity: number;
  private readonly quads: Quads;
  private readonly state: Float32Array;
  private live = 0;

  constructor(
    private readonly atlas: Atlas,
    options: BatchOptions,
  ) {
    this.capacity = options.capacity;
    this.quads = new Quads(options.capacity, atlas, options);
    this.state = new Float32Array(options.capacity * STRIDE);
  }

  get mesh(): THREE.Mesh {
    return this.quads.mesh;
  }

  /** Particles alive. */
  get count(): number {
    return this.live;
  }

  /** Adds a particle; when the batch is full it takes the place of the one nearest its end. */
  spawn(p: Readonly<ParticleSpec>): void {
    if (!(p.life > 0)) return;
    let i = this.live;
    if (i < this.capacity) {
      this.live++;
    } else {
      let spent = -1;
      for (let k = 0; k < this.capacity; k++) {
        const o = k * STRIDE;
        const share = this.state[o + AGE] / this.state[o + LIFE];
        if (share > spent) {
          spent = share;
          i = k;
        }
      }
    }
    const s = this.state;
    const o = i * STRIDE;
    s[o + PX] = p.x;
    s[o + PY] = p.y;
    s[o + PZ] = p.z;
    s[o + VX] = p.vx;
    s[o + VY] = p.vy;
    s[o + VZ] = p.vz;
    const axis = Math.hypot(p.ax, p.ay, p.az) || 1;
    s[o + AX] = p.ax / axis;
    s[o + AY] = p.ay / axis;
    s[o + AZ] = p.az / axis;
    s[o + LENGTH] = p.length;
    s[o + STRETCH] = p.stretch;
    s[o + AGE] = 0;
    s[o + LIFE] = p.life;
    s[o + GRAVITY] = p.gravity;
    s[o + DRAG] = p.drag;
    s[o + SIZE] = p.size;
    s[o + END_SIZE] = p.endSize;
    s[o + ROTATION] = p.rotation;
    s[o + SPIN] = p.spin;
    s[o + FADE_IN] = p.fadeIn;
    s[o + FADE_OUT] = p.fadeOut;
    s[o + R] = p.r;
    s[o + G] = p.g;
    s[o + B] = p.b;
    s[o + A] = p.alpha;
    s[o + END_R] = p.endR;
    s[o + END_G] = p.endG;
    s[o + END_B] = p.endB;
    for (let k = 0; k < 4; k++) s[o + CELL + k] = p.cell[k];
  }

  /** Moves, ages and redraws every particle; the dead ones go. */
  update(dt: number): void {
    const s = this.state;
    let n = this.live;
    for (let i = 0; i < n; ) {
      const o = i * STRIDE;
      const age = s[o + AGE] + dt;
      if (age >= s[o + LIFE]) {
        n--;
        if (i < n) s.copyWithin(o, n * STRIDE, n * STRIDE + STRIDE);
        continue;
      }
      s[o + AGE] = age;
      const damp = Math.exp(-s[o + DRAG] * dt);
      s[o + VX] *= damp;
      s[o + VY] = s[o + VY] * damp - s[o + GRAVITY] * dt;
      s[o + VZ] *= damp;
      s[o + PX] += s[o + VX] * dt;
      s[o + PY] += s[o + VY] * dt;
      s[o + PZ] += s[o + VZ] * dt;
      s[o + ROTATION] += s[o + SPIN] * dt;
      i++;
    }
    this.live = n;
    this.draw();
  }

  dispose(): void {
    this.quads.dispose(this.atlas);
  }

  private draw(): void {
    const s = this.state;
    for (let i = 0; i < this.live; i++) {
      const o = i * STRIDE;
      const t = s[o + AGE] / s[o + LIFE];
      const grown = 1 - (1 - t) * (1 - t);
      const size = s[o + SIZE] + (s[o + END_SIZE] - s[o + SIZE]) * grown;
      let alpha = s[o + A] * (1 - Math.pow(t, s[o + FADE_OUT]));
      if (s[o + FADE_IN] > 0) alpha *= Math.min(1, t / s[o + FADE_IN]);
      const r = s[o + R] + (s[o + END_R] - s[o + R]) * t;
      const g = s[o + G] + (s[o + END_G] - s[o + G]) * t;
      const b = s[o + B] + (s[o + END_B] - s[o + B]) * t;
      let x = s[o + PX];
      let y = s[o + PY];
      let z = s[o + PZ];
      const speed = Math.hypot(s[o + VX], s[o + VY], s[o + VZ]);
      if (s[o + STRETCH] > 0 && speed > 1e-4) {
        // Trailing its motion, its head at its position.
        const ax = s[o + VX] / speed;
        const ay = s[o + VY] / speed;
        const az = s[o + VZ] / speed;
        const half = Math.max(size, speed * s[o + STRETCH]) / 2;
        this.quads.put(i, x - ax * half, y - ay * half, z - az * half, size, ax, ay, az, half, r, g, b, alpha, s, o + CELL, 0);
      } else if (s[o + LENGTH] > 0) {
        // Out from its position along its axis, lengthening as it widens.
        const half = (s[o + LENGTH] + size - s[o + SIZE]) / 2;
        x += s[o + AX] * half;
        y += s[o + AY] * half;
        z += s[o + AZ] * half;
        this.quads.put(i, x, y, z, size, s[o + AX], s[o + AY], s[o + AZ], half, r, g, b, alpha, s, o + CELL, 0);
      } else {
        this.quads.put(i, x, y, z, size, 0, 0, 0, 0, r, g, b, alpha, s, o + CELL, s[o + ROTATION]);
      }
    }
    this.quads.commit(this.live);
  }
}

// A tracer's state, one stride each, kept dense like the particles'.
const T_FROM = 0;
const T_DIR = 3;
const T_DIST = 6;
const T_TRAVEL = 7;
const T_LENGTH = 8;
const T_SPEED = 9;
const T_WIDTH = 10;
const T_R = 11;
const T_STRIDE = 16;

export interface TracerSpec {
  /** Metres it flies along its path each second, and the length of its streak. */
  speed: number;
  length: number;
  /** The fewest seconds it takes over its path, slowing on a short one so that it is seen. */
  shortest: number;
  width: number;
  /** Linear RGB, above 1 for bright light, and alpha. */
  r: number;
  g: number;
  b: number;
  alpha: number;
}

/**
 * Tracers: glowing streaks that fly along a shot's path and stop where it ends, each appearing
 * whole with its tail at the start and shrinking into the end. Additive, in one draw call.
 */
export class TracerBatch {
  readonly capacity: number;
  private readonly quads: Quads;
  private readonly state: Float32Array;
  private live = 0;

  constructor(
    private readonly atlas: Atlas,
    options: BatchOptions,
  ) {
    this.capacity = options.capacity;
    this.quads = new Quads(options.capacity, atlas, options);
    this.state = new Float32Array(options.capacity * T_STRIDE);
  }

  get mesh(): THREE.Mesh {
    return this.quads.mesh;
  }

  get count(): number {
    return this.live;
  }

  /**
   * A tracer along the path from `from` to `to`, seen from `startAt` metres along it. When the
   * batch is full it takes the place of the one nearest its end.
   */
  add(from: THREE.Vector3, to: THREE.Vector3, startAt: number, spec: Readonly<TracerSpec>): void {
    const dx = to.x - from.x;
    const dy = to.y - from.y;
    const dz = to.z - from.z;
    const dist = Math.hypot(dx, dy, dz);
    if (dist - startAt < 1 || !(spec.speed > 0)) return;
    let i = this.live;
    if (i < this.capacity) {
      this.live++;
    } else {
      let spent = -1;
      for (let k = 0; k < this.capacity; k++) {
        const o = k * T_STRIDE;
        const share = (this.state[o + T_TRAVEL] - this.state[o + T_LENGTH]) / this.state[o + T_DIST];
        if (share > spent) {
          spent = share;
          i = k;
        }
      }
    }
    const s = this.state;
    const o = i * T_STRIDE;
    s[o + T_FROM] = from.x;
    s[o + T_FROM + 1] = from.y;
    s[o + T_FROM + 2] = from.z;
    s[o + T_DIR] = dx / dist;
    s[o + T_DIR + 1] = dy / dist;
    s[o + T_DIR + 2] = dz / dist;
    s[o + T_DIST] = dist;
    const length = Math.min(spec.length, dist - startAt);
    // Whole from the start: the head as far out as the length, the tail at the start.
    s[o + T_TRAVEL] = startAt + length;
    s[o + T_LENGTH] = length;
    s[o + T_SPEED] = Math.min(spec.speed, (dist - startAt) / spec.shortest);
    s[o + T_WIDTH] = spec.width;
    s[o + T_R] = spec.r;
    s[o + T_R + 1] = spec.g;
    s[o + T_R + 2] = spec.b;
    s[o + T_R + 3] = spec.alpha;
  }

  update(dt: number): void {
    const s = this.state;
    let n = this.live;
    for (let i = 0; i < n; ) {
      const o = i * T_STRIDE;
      const travel = s[o + T_TRAVEL] + s[o + T_SPEED] * dt;
      if (travel - s[o + T_LENGTH] >= s[o + T_DIST]) {
        n--;
        if (i < n) s.copyWithin(o, n * T_STRIDE, n * T_STRIDE + T_STRIDE);
        continue;
      }
      s[o + T_TRAVEL] = travel;
      i++;
    }
    this.live = n;
    for (let i = 0; i < n; i++) {
      const o = i * T_STRIDE;
      const head = Math.min(s[o + T_TRAVEL], s[o + T_DIST]);
      const tail = s[o + T_TRAVEL] - s[o + T_LENGTH];
      const mid = (head + tail) / 2;
      const dx = s[o + T_DIR];
      const dy = s[o + T_DIR + 1];
      const dz = s[o + T_DIR + 2];
      this.quads.put(
        i,
        s[o + T_FROM] + dx * mid,
        s[o + T_FROM + 1] + dy * mid,
        s[o + T_FROM + 2] + dz * mid,
        s[o + T_WIDTH],
        dx,
        dy,
        dz,
        (head - tail) / 2,
        s[o + T_R],
        s[o + T_R + 1],
        s[o + T_R + 2],
        s[o + T_R + 3],
        GLOW,
        0,
        0,
      );
    }
    this.quads.commit(n);
  }

  dispose(): void {
    this.quads.dispose(this.atlas);
  }
}
