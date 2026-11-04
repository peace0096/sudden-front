// Loads every file manifest.json names, once per page, starting while the player is still in the
// lobby; the result is kept from match to match. A file that can't be loaded gets a console warning
// and an entry in failures, and whatever uses it shows a placeholder instead; the next call of
// loadArt() (main.ts makes one as each match starts) tries it once more. Nothing here logs an
// error, since e2e/play.mjs fails on any console error.
import * as THREE from "three";
import type { GLTF, GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { generatedInfo, manifestFiles } from "./manifest";
import { disposeLamberts } from "./materials";
import { disposePlaceholders } from "./placeholder";

/** A download, or the glTF reader's code, still unfinished after this is given up. */
const FETCH_TIMEOUT_MS = 60_000;

export interface ArtFailure {
  file: string;
  reason: string;
}

export interface ArtSet {
  /** Parsed .glb files, by manifest path. */
  models: Map<string, GLTF>;
  /** Decoded .webp files, by manifest path. */
  textures: Map<string, THREE.Texture>;
  failures: ArtFailure[];
}

export interface ArtStatus {
  ready: boolean;
  failures: ArtFailure[];
  counts: { files: number; models: number; textures: number; failed: number };
}

let loading: Promise<ArtSet> | null = null;
let loaded: ArtSet | null = null;
/** Files that failed and have had their one retry. */
const retried = new Set<string>();
let gltfLoader: Promise<GLTFLoader> | null = null;

/**
 * Starts loading on the first call; every call gets the same set, and it never rejects. A call
 * after that load is over also tries each file that failed once more, adding what loads to the set,
 * and resolves when that is done.
 */
export function loadArt(): Promise<ArtSet> {
  if (!loading) loading = loadAll().then((set) => (loaded = set));
  else if (loaded?.failures.some((failure) => !retried.has(failure.file))) loading = retry(loaded);
  return loading;
}

/** The set once loadArt() has finished, for code that runs after it was awaited; null before. */
export function loadedArt(): ArtSet | null {
  return loaded;
}

/** For debugging, as window.__art under ?debug. */
export function artStatus(): ArtStatus {
  return {
    ready: loaded !== null,
    failures: loaded ? loaded.failures.map((failure) => ({ ...failure })) : [],
    counts: {
      files: manifestFiles().length,
      models: loaded?.models.size ?? 0,
      textures: loaded?.textures.size ?? 0,
      failed: loaded?.failures.length ?? 0,
    },
  };
}

/**
 * Frees everything the art holds on the GPU: geometry, materials and textures, including the
 * shared Lambert materials and placeholders. The data stays loaded and is uploaded again when next
 * drawn. A match calls this before it disposes its renderer.
 */
export function releaseGpu(): void {
  if (loaded) {
    for (const model of loaded.models.values()) for (const scene of model.scenes) disposeTree(scene);
    for (const texture of loaded.textures.values()) texture.dispose();
  }
  disposeLamberts();
  disposePlaceholders();
}

/**
 * One model under public/art/ that the manifest need not name, loaded the same way but outside the
 * shared set: the ?art&candidates gallery uses it. It rejects when the file can't be loaded and
 * logs nothing; the caller decides how to report it.
 */
export async function loadArtModel(file: string): Promise<GLTF> {
  return parseModel(file, await download(file));
}

/** Like loadArtModel, for a .webp texture. */
export async function loadArtTexture(file: string): Promise<THREE.Texture> {
  return decodeTexture(await download(file));
}

async function loadAll(): Promise<ArtSet> {
  const set: ArtSet = { models: new Map(), textures: new Map(), failures: [] };
  set.failures = await loadFiles(set, manifestFiles());
  return set;
}

/** The set's failed files loaded once more; those that fail again stay failures for good. */
async function retry(set: ArtSet): Promise<ArtSet> {
  const files = set.failures.map((failure) => failure.file).filter((file) => !retried.has(file));
  for (const file of files) retried.add(file);
  const again = await loadFiles(set, files);
  set.failures = [...set.failures.filter((failure) => !files.includes(failure.file)), ...again];
  return set;
}

/** Loads files into the set and returns those that failed, each told on the console. */
async function loadFiles(set: ArtSet, files: string[]): Promise<ArtFailure[]> {
  const failures: ArtFailure[] = [];
  await Promise.all(
    files.map(async (file) => {
      try {
        if (file.endsWith(".glb")) set.models.set(file, await parseModel(file, await download(file)));
        else if (file.endsWith(".webp")) set.textures.set(file, await decodeTexture(await download(file)));
        else throw new Error("not a .glb or .webp file");
      } catch (error) {
        const reason = error instanceof Error ? error.message : String(error);
        failures.push({ file, reason });
        console.warn(`[art] ${file}: ${reason}`);
      }
    }),
  );
  return failures;
}

async function download(file: string): Promise<ArrayBuffer> {
  const hash = generatedInfo(file)?.hash;
  const url = `art/${encodeURI(file)}${hash ? `?v=${encodeURIComponent(hash)}` : ""}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.arrayBuffer();
}

// A server can answer a missing file with its index page and status 200; the magic bytes catch it.
function hasMagic(bytes: Uint8Array, offset: number, magic: string): boolean {
  if (bytes.length < offset + magic.length) return false;
  for (let i = 0; i < magic.length; i++) {
    if (bytes[offset + i] !== magic.charCodeAt(i)) return false;
  }
  return true;
}

async function parseModel(file: string, buffer: ArrayBuffer): Promise<GLTF> {
  if (!hasMagic(new Uint8Array(buffer), 0, "glTF")) throw new Error("not a GLB file");
  const loader = await gltf();
  return loader.parseAsync(buffer, `art/${file.slice(0, file.lastIndexOf("/") + 1)}`);
}

/**
 * Whether createImageBitmap can flip an image as it decodes it (imageOrientation "flipY"). As in
 * three's GLTFLoader, Safari and Firefox before 98 are left out: there an image decodes through an
 * <img> instead, which WebGL flips as it uploads it.
 */
const bitmapsFlip = ((): boolean => {
  if (typeof createImageBitmap === "undefined") return false;
  const agent = navigator.userAgent;
  if (/^((?!chrome|android).)*safari/i.test(agent)) return false;
  const firefox = /Firefox\/(\d+)\./.exec(agent);
  return !firefox || Number(firefox[1]) >= 98;
})();

async function decodeTexture(buffer: ArrayBuffer): Promise<THREE.Texture> {
  const bytes = new Uint8Array(buffer);
  if (!hasMagic(bytes, 0, "RIFF") || !hasMagic(bytes, 8, "WEBP")) throw new Error("not a WebP file");
  const blob = new Blob([buffer], { type: "image/webp" });
  let texture: THREE.Texture;
  if (bitmapsFlip) {
    // WebGL can't flip an ImageBitmap as it uploads it, so it is flipped while decoding.
    texture = new THREE.Texture(await createImageBitmap(blob, { imageOrientation: "flipY", premultiplyAlpha: "none" }));
    texture.flipY = false;
  } else {
    texture = new THREE.Texture(await decodeImage(blob));
    texture.flipY = true;
  }
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.needsUpdate = true;
  return texture;
}

/** An <img> holding the decoded image, read from a blob URL that is freed once it has decoded. */
async function decodeImage(blob: Blob): Promise<HTMLImageElement> {
  const url = URL.createObjectURL(blob);
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    return image;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * The glTF reader and its meshopt decoder, fetched on first use so an empty manifest costs nothing.
 * Like a download it is given up after FETCH_TIMEOUT_MS, so a stalled chunk can't hold a match on
 * the loading notice; a failure is forgotten, so the next model asks for the code again.
 */
function gltf(): Promise<GLTFLoader> {
  gltfLoader ??= withTimeout(
    Promise.all([
      import("three/examples/jsm/loaders/GLTFLoader.js"),
      import("three/examples/jsm/libs/meshopt_decoder.module.js"),
    ]).then(async ([{ GLTFLoader }, { MeshoptDecoder }]) => {
      await MeshoptDecoder.ready;
      return new GLTFLoader().setMeshoptDecoder(MeshoptDecoder);
    }),
    FETCH_TIMEOUT_MS,
    "the glTF reader",
  ).catch((error: unknown) => {
    gltfLoader = null;
    throw error;
  });
  return gltfLoader;
}

/** `promise`, or a rejection once `ms` have passed without it settling. */
function withTimeout<T>(promise: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} did not load within ${ms / 1000} s`)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function disposeTree(root: THREE.Object3D): void {
  root.traverse((object) => {
    if (object instanceof THREE.SkinnedMesh) object.skeleton.dispose();
    if (object instanceof THREE.Mesh || object instanceof THREE.Line || object instanceof THREE.Points) {
      object.geometry.dispose();
      const materials: THREE.Material[] = Array.isArray(object.material) ? object.material : [object.material];
      for (const material of materials) {
        for (const value of Object.values(material)) if (value instanceof THREE.Texture) value.dispose();
        material.dispose();
      }
    }
  });
}
