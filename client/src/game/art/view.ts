// What the ?art page (preview.ts) and the candidate gallery (gallery.ts) draw with: one WebGL
// renderer on an OffscreenCanvas that renders every cell and hands the frame to the cell's own
// canvas as an ImageBitmap, so a page holds one WebGL context however many cells it shows, and no
// pixels are read back (a readback stalls software GL and makes the browser log a warning). Also
// the scene pieces both pages share. Animated cells redraw while on screen; still ones redraw when
// told something changed.
import * as THREE from "three";
import * as hitbox from "./hitbox";

export const HEAD_TOP = hitbox.HEAD_BASE + hitbox.HEAD_HEIGHT;

export interface View {
  camera: THREE.Camera;
  /** Left edge and width inside the cell, in CSS pixels. */
  x: number;
  width: number;
}

export interface Cell {
  canvas: HTMLCanvasElement;
  context: ImageBitmapRenderingContext;
  width: number;
  height: number;
  scene: THREE.Scene;
  views: View[];
  /** Advances the cell's own motion; cells without it are drawn only when something changes. */
  tick?: (dt: number, time: number) => void;
  /** Reacts to a change of the page's options; the cell is redrawn after. */
  configure?: () => void;
  dirty: boolean;
}

export class Viewer {
  private readonly surface = new OffscreenCanvas(1, 1);
  private readonly renderer = new THREE.WebGLRenderer({ canvas: this.surface, antialias: true });
  private readonly cells: Cell[] = [];
  private readonly onScreen = new Set<Cell>();
  private readonly observer: IntersectionObserver;
  private readonly pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
  private frames = 0;
  private last = performance.now();
  private time = 0;

  constructor() {
    this.renderer.setPixelRatio(1);
    // The game's tone mapping and exposure (game.ts).
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 0.95;
    this.renderer.setScissorTest(true);
    this.observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const cell = this.cells.find((candidate) => candidate.canvas === entry.target);
          if (!cell) continue;
          if (entry.isIntersecting) this.onScreen.add(cell);
          else this.onScreen.delete(cell);
        }
      },
      { rootMargin: "200px" },
    );
  }

  /** How many frames have been drawn, for scripts that wait on the animation (gallery-shots.mjs). */
  frameCount(): number {
    return this.frames;
  }

  addCell(cell: Omit<Cell, "dirty" | "context">): Cell {
    const added: Cell = { ...cell, context: cell.canvas.getContext("bitmaprenderer")!, dirty: true };
    added.canvas.width = Math.round(added.width * this.pixelRatio);
    added.canvas.height = Math.round(added.height * this.pixelRatio);
    added.canvas.style.width = `${added.width}px`;
    added.canvas.style.height = `${added.height}px`;
    this.cells.push(added);
    this.observer.observe(added.canvas);
    return added;
  }

  /** Renders a scene once, off screen, and returns the draw calls it took. */
  drawCalls(scene: THREE.Scene, camera: THREE.Camera): number {
    this.renderer.setScissorTest(false);
    this.renderer.setViewport(0, 0, this.surface.width, this.surface.height);
    this.renderer.render(scene, camera);
    this.renderer.setScissorTest(true);
    return this.renderer.info.render.calls;
  }

  setToneMapping(mapping: THREE.ToneMapping): void {
    this.renderer.toneMapping = mapping;
  }

  /** Lets every cell react to changed options, and redraws them. */
  reconfigure(): void {
    for (const cell of this.cells) {
      cell.configure?.();
      cell.dirty = true;
    }
  }

  /** Draws every cell once, on screen or not, then keeps the visible ones moving. */
  start(): void {
    for (const cell of this.cells) {
      cell.configure?.();
      cell.tick?.(0, 0);
      this.draw(cell);
    }
    const loop = (now: number) => {
      const dt = Math.min(0.1, (now - this.last) / 1000);
      this.last = now;
      this.time += dt;
      for (const cell of this.cells) {
        const visible = this.onScreen.has(cell);
        if (visible && cell.tick) {
          cell.tick(dt, this.time);
          cell.dirty = true;
        }
        if (visible && cell.dirty) this.draw(cell);
      }
      this.frames++;
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  }

  private draw(cell: Cell): void {
    const ratio = this.pixelRatio;
    const width = Math.round(cell.width * ratio);
    const height = Math.round(cell.height * ratio);
    // The frame is handed over whole, so the surface takes each cell's size; cells of one size
    // follow each other, so it rarely changes.
    if (this.surface.width !== width || this.surface.height !== height) this.renderer.setSize(width, height, false);
    for (const view of cell.views) {
      const x = Math.round(view.x * ratio);
      const w = Math.round(view.width * ratio);
      this.renderer.setViewport(x, 0, w, height);
      this.renderer.setScissor(x, 0, w, height);
      this.renderer.render(cell.scene, view.camera);
    }
    cell.context.transferFromImageBitmap(this.surface.transferToImageBitmap());
    cell.dirty = false;
  }
}

/** The game's outdoor light (world.ts) and a neutral backdrop. */
export function stageScene(background = 0xbfc8c6): THREE.Scene {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(background);
  scene.add(new THREE.HemisphereLight(0xfff1dc, 0xb09c7c, 2.1));
  const sun = new THREE.DirectionalLight(0xfff0d8, 2.2);
  sun.position.set(38, 70, 26);
  scene.add(sun);
  return scene;
}

export function lineMaterial(color: number, opacity = 0.95): THREE.LineBasicMaterial {
  // Drawn over the model so the fit reads even where the model covers the box.
  return new THREE.LineBasicMaterial({ color, transparent: true, opacity, depthTest: false, depthWrite: false, toneMapped: false });
}

const unitEdges = new THREE.EdgesGeometry(new THREE.BoxGeometry(1, 1, 1));

/** Frees the wire boxes' shared geometry on the GPU; it stays usable. The match calls it on its way out. */
export function releaseWireGpu(): void {
  unitEdges.dispose();
}

/** A wireframe box from y0 to y1, `width` by `depth` around the centre. */
export function wireBox(width: number, y0: number, y1: number, depth: number, color: number): THREE.LineSegments {
  const box = new THREE.LineSegments(unitEdges, lineMaterial(color));
  box.scale.set(width, y1 - y0, depth);
  box.position.y = (y0 + y1) / 2;
  box.renderOrder = 10;
  return box;
}

/** A small cross drawn over everything, to mark a point such as a muzzle. */
export function marker(color: number, size: number): THREE.LineSegments {
  const s = size / 2;
  const points = [[-s, 0, 0], [s, 0, 0], [0, -s, 0], [0, s, 0], [0, 0, -s], [0, 0, s]].map(([x, y, z]) => new THREE.Vector3(x, y, z));
  const cross = new THREE.LineSegments(new THREE.BufferGeometry().setFromPoints(points), lineMaterial(color, 1));
  cross.renderOrder = 11;
  return cross;
}

export function ground(size: number, divisions: number): THREE.Object3D {
  const group = new THREE.Group();
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(size, size), new THREE.MeshBasicMaterial({ color: 0x9a9483, toneMapped: false }));
  floor.rotation.x = -Math.PI / 2;
  floor.position.y = -0.002;
  const grid = new THREE.GridHelper(size, divisions, 0x6f6a5c, 0x857f70);
  group.add(floor, grid);
  return group;
}

/**
 * The server's hitboxes as wireframes: red head, orange body, blue legs, and a shield bearer's
 * shield in grey. `pose` sizes them as `hitboxes` and `ray_shield` in server/src/game/systems/fire.rs
 * do; the shield turns with the player's yaw, the other boxes never turn.
 */
export interface HitboxFrame {
  group: THREE.Group;
  crouch(on: boolean): void;
  pose(crouching: boolean, headScale: number, shieldYaw?: number | null): void;
  /** Takes the frame out of its scene and frees its line materials; the box geometry is shared. */
  dispose(): void;
}

export function hitboxFrame(): HitboxFrame {
  const group = new THREE.Group();
  const head = wireBox(1, 0, 1, 1, 0xff5a4a);
  const body = wireBox(1, 0, 1, 1, 0xe7a42c);
  const legs = wireBox(1, 0, 1, 1, 0x5aa8ff);
  const shield = new THREE.Group();
  const plate = wireBox(1, 0, 1, 1, 0xd0d4d6);
  shield.add(plate);
  group.add(head, body, legs, shield);
  const fit = (box: THREE.Object3D, halfWidth: number, y0: number, y1: number) => {
    box.scale.set(halfWidth * 2, y1 - y0, halfWidth * 2);
    box.position.y = (y0 + y1) / 2;
  };
  const frame: HitboxFrame = {
    group,
    // The server's crouch: every top drops by CROUCH_DROP and the legs shorten with it.
    crouch: (on) => frame.pose(on, 1),
    pose(crouching, headScale, shieldYaw = null) {
      const drop = crouching ? hitbox.CROUCH_DROP : 0;
      // A resized head still sits on the same neck.
      fit(head, hitbox.HEAD_HALF_WIDTH * headScale, hitbox.HEAD_BASE - drop, hitbox.HEAD_BASE + hitbox.HEAD_HEIGHT * headScale - drop);
      fit(body, hitbox.BODY_HALF_WIDTH, hitbox.BODY_BOTTOM - drop, hitbox.BODY_TOP - drop);
      fit(legs, hitbox.LEGS_HALF_WIDTH, hitbox.LEGS_BOTTOM, hitbox.LEGS_TOP - drop);
      shield.visible = shieldYaw !== null;
      if (shieldYaw !== null) {
        shield.rotation.y = shieldYaw;
        const top = hitbox.SHIELD_TOP - drop;
        plate.scale.set(hitbox.SHIELD_HALF_WIDTH * 2, top, hitbox.SHIELD_FAR - hitbox.SHIELD_NEAR);
        plate.position.set(0, top / 2, -(hitbox.SHIELD_NEAR + hitbox.SHIELD_FAR) / 2);
      }
    },
    dispose() {
      group.removeFromParent();
      group.traverse((object) => {
        if (object instanceof THREE.LineSegments) (object.material as THREE.Material).dispose();
      });
    },
  };
  frame.pose(false, 1);
  return frame;
}

export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** One labelled fact; `state` marks it as within or over a limit. */
export function fact(list: HTMLElement, term: string, value: string, state?: "ok" | "over"): void {
  list.append(el("dt", "", term), el("dd", state ?? "", value));
}

export const number = (value: number) => value.toLocaleString("ko-KR");
export const metres = (values: number[], digits = 2) => values.map((value) => value.toFixed(digits)).join(" × ");
