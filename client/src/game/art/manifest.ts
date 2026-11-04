// Typed access to manifest.json, the hand-edited list of every art file the game loads and how each
// is fitted, and to generated.json, which the art pipeline writes. Paths are relative to
// client/public/art/. Every path is the value of a `file` key or an entry of a `files` map; the
// loader and scripts/check-art.mjs both find the files by that rule.
//
// JSON imports type strings as string and arrays as number[], so the types below use those and
// list the expected values in comments; tsc then checks the manifest's shape as it is.
import type { EulerOrder, Object3D } from "three";
import { WeaponKind } from "../../gen/game_pb";
import generatedJson from "./generated.json";
import manifestJson from "./manifest.json";

/** A WeaponKind key such as "RIFLE", which is how manifest.json names weapons. */
export type WeaponKindName = Exclude<keyof typeof WeaponKind, "UNSPECIFIED">;

/** [x, y, z]: metres for a position, radians in XYZ order for a rotation. */
export type Vec3 = number[];

/** Where a model sits relative to whatever holds it; each part defaults to none. */
export interface Placement {
  position?: Vec3;
  rotation?: Vec3;
  scale?: number;
}

export interface WeaponArt {
  file: string;
  /** How the gun is held, which picks the arms' pose: "rifle", "pistol", "knife", "launcher" or "akimbo". */
  hold: string;
  /**
   * Third person, relative to the right hand's socket. `support` is where the left wrist takes
   * hold, in the gun's own frame (grip at the origin, barrel along −Z); without it the left arm
   * keeps its hold's pose.
   */
  tp: Placement & { support?: Vec3 };
  /**
   * First person, relative to the view model's rig: the grip's place there. The right hand holds
   * the grip as `tp` places it in the palm.
   */
  fp: Placement & {
    /**
     * Where the left wrist holds the gun, in the gun's own frame like tp.support, which it
     * replaces in first person: the view shows the left hand further forward, under the handguard.
     */
    support?: Vec3;
    /** Held in the right hand alone; the left arm stays out of view. */
    oneHanded?: boolean;
    /**
     * For a second gun in the left hand: its grip's sideways place in the rig (x). It is placed as
     * the mirror image of the first one, and held as the right hand holds that one, mirrored.
     */
    handX?: number;
    /** Metres (rig) the left hand racks back along the barrel after each shot: a pump action. */
    pump?: number;
    /** Moves the whole rig, hands included, for guns held away from the rifle pose. */
    rigOffset?: Vec3;
    /** How hard a shot throws the gun back; 1 is the rifle's kick. */
    kickScale: number;
    /** Size of the muzzle flash; 1 is the rifle's. */
    flashScale?: number;
  };
}

export interface CharacterArt {
  /** One rigged soldier, painted once per team. Metres, feet at y = 0, head bone at the hitbox's neck. */
  files: { red: string; blue: string };
  /** Radians about +Y that turn the model to face −Z, the way players face; π for a model made facing +Z. */
  forwardYaw: number;
  /**
   * Clip names in the files, by state; the lower body plays the movement ones. A state without a
   * clip is left to rig code: until a crouch clip exists, crouching bends the legs by IK.
   */
  clips: {
    idle: string;
    run: string;
    runBack?: string;
    strafeL?: string;
    strafeR?: string;
    walk?: string;
    death: string;
    crouchIdle?: string;
    crouchWalk?: string;
    shoot?: string;
  };
  /** The upper body for each hold (see WeaponArt.hold). */
  holds?: Record<string, HoldArt>;
}

/**
 * A hold's upper-body pose: the clip the chest and arms play, rotations added to named bones
 * after it (XYZ radians in the bone's own frame), and for two guns the left arm mirroring the
 * right. The left hand then goes to the gun's tp.support, if it has one.
 */
export interface HoldArt {
  clip?: string;
  bones?: Record<string, Vec3>;
  mirror?: boolean;
}

/**
 * First-person arms: the character's own files, of which only the arms are drawn (the triangles
 * skinned to the clavicles and the bones below them), so no head, torso or legs come into view.
 */
export interface ArmsArt {
  files: { red: string; blue: string };
  /** Their size in the view model's rig, next to guns that fp.scale enlarges; 1 is the character's own. */
  scale: number;
}

/** A model that flies, such as the missile; it points along +Z, which is what lookAt aims. */
export interface ProjectileArt {
  file: string;
  placement?: Placement;
}

/** A tiling WebP texture for the merged wall, floor and roof boxes. */
export interface SurfaceArt {
  file: string;
  /** Metres one repeat of the texture covers. */
  tile: number;
}

/** A model fitted into map boxes of one fixed size (container, crate, barrel), drawn as instances. */
export interface KitArt {
  file: string;
  /** [x, y, z] metres the model was built to fill, x its long side, standing on y = 0 around x = z = 0. */
  size?: Vec3;
  /** "#rrggbb" multiplied into tintNode, as an instance colour; the model's other nodes keep their colours. */
  tint?: string;
  tintNode?: string;
}

/** A decorative model standing where the map data puts a prop; the same fields as a kit. */
export type PropArt = KitArt;

export interface MapArt {
  surfaces: Record<string, SurfaceArt>;
  kits: Record<string, KitArt>;
  props: Record<string, PropArt>;
}

/**
 * An atlas: one WebP and its named cells as [x, y, width, height] in pixels from the image's top
 * left. Cells are white with the shape in a straight alpha; the runtime gives them colour.
 */
export interface AtlasArt {
  file: string;
  cells: Record<string, number[]>;
}

/** The particle atlas, and the decals (blood, bullet holes) in an atlas of their own. */
export interface FxArt extends AtlasArt {
  decals?: AtlasArt;
}

/**
 * Limits for integrated graphics. check-art.mjs enforces downloadMB, fileKB and the per-model
 * triangles; the rest are runtime limits to hold Game.perf() to.
 */
export interface Budgets {
  /**
   * Per frame with a full room, every pass together; `map` for the map's fixed objects alone
   * (world.ts), everything in view; `effects` for the effects alone (effects.ts), whatever is going
   * on: one call for each of its four batches (smoke and dust, light, tracers, marks).
   */
  drawCalls: { typical: number; max: number; map: number; effects: number };
  /** Per model, and in view at once for scene. */
  triangles: { character: number; arms: number; gun: number; scene: number };
  /** Shader programs compiled at once. */
  programs: number;
  textureMB: number;
  /** Everything under public/art/. */
  downloadMB: number;
  /** Per file. */
  fileKB: { character: number; gun: number };
}

export interface ArtManifest {
  /** Empty, or one entry for every WeaponKind: check-art fails the build on a partial set. */
  weapons: Partial<Record<WeaponKindName, WeaponArt>>;
  character: CharacterArt | null;
  arms: ArmsArt | null;
  projectiles: Partial<Record<WeaponKindName, ProjectileArt>>;
  map: MapArt;
  fx: FxArt | null;
  budgets: Budgets;
}

/** What the art pipeline measured in one file. */
export interface GeneratedFile {
  /** A prefix of sha256, sent as ?v= so a changed file is never served from a stale cache. */
  hash: string;
  /** The whole content hash; with bytes, scripts/check-art.mjs holds the file on disk to it. */
  sha256: string;
  bytes: number;
  /** Over all of a model's meshes. */
  triangles?: number;
  /** A model's textures, or the image itself. */
  images?: { width: number; height: number }[];
}

export const manifest: ArtManifest = manifestJson;
const generated: { files: Record<string, GeneratedFile> } = generatedJson;
export const budgets: Budgets = manifest.budgets;

/** A record's own entry: names like "constructor" that every object inherits are not entries. */
function entry<T>(record: Partial<Record<string, T>>, key: string): T | undefined {
  return Object.hasOwn(record, key) ? record[key] : undefined;
}

/** Puts an object where a placement says, relative to its parent. */
export function place(object: Object3D, placement: Placement): void {
  object.position.fromArray(placement.position ?? [0, 0, 0]);
  object.rotation.fromArray([...(placement.rotation ?? [0, 0, 0]), "XYZ"] as [number, number, number, EulerOrder]);
  object.scale.setScalar(placement.scale ?? 1);
}

export function weaponArt(kind: WeaponKind): WeaponArt | undefined {
  return entry(manifest.weapons, WeaponKind[kind]);
}

export function projectileArt(kind: WeaponKind): ProjectileArt | undefined {
  return entry(manifest.projectiles, WeaponKind[kind]);
}

export function characterArt(): CharacterArt | undefined {
  return manifest.character ?? undefined;
}

export function armsArt(): ArmsArt | undefined {
  return manifest.arms ?? undefined;
}

export function kitArt(name: string): KitArt | undefined {
  return entry(manifest.map.kits, name);
}

export function propArt(name: string): PropArt | undefined {
  return entry(manifest.map.props, name);
}

export function fxArt(): FxArt | undefined {
  return manifest.fx ?? undefined;
}

export function generatedInfo(file: string): GeneratedFile | undefined {
  return entry(generated.files, file);
}

/** Every file the manifest names, once each. */
export function manifestFiles(): string[] {
  const found = new Set<string>();
  const walk = (value: unknown): void => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "file" && typeof child === "string") found.add(child);
      else if (key === "files" && child && typeof child === "object") {
        for (const file of Object.values(child)) if (typeof file === "string") found.add(file);
      } else walk(child);
    }
  };
  walk(manifest);
  return [...found];
}
