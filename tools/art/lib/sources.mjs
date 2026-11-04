// The packs in sources.json, the licence gate every build input passes, and the paths of the files
// fetch.mjs left in art-src/, each checked against the sha256 sources.json pins before it is used.
// Everything under art-src/ is data: it is read, never run.
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { checkPackPaths, inside } from "./paths.mjs";

export const toolsDir = fileURLToPath(new URL("..", import.meta.url));
export const repoDir = resolve(toolsDir, "..", "..");
export const artSrc = join(repoDir, "art-src");

const sources = JSON.parse(readFileSync(join(toolsDir, "sources.json"), "utf8"));
const packs = new Map(sources.packs.map((pack) => [checkPackPaths(pack).id, pack]));

/** Licences that require their attribution to be shown; client/scripts/check-art.mjs allows the same. */
const ATTRIBUTION_LICENSES = ["CC-BY-3.0", "CC-BY-4.0"];

export const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

/** Files hashed in this run, by path, so each is read once. */
const hashed = new Map();
function hashOf(path) {
  if (!hashed.has(path)) hashed.set(path, sha256(readFileSync(path)));
  return hashed.get(path);
}

/**
 * A pack from sources.json, refused if it is excluded or its licence is neither CC0-1.0 nor
 * CC-BY-3.0/4.0 with the attribution that licence requires.
 */
export function licensedPack(id) {
  const pack = packs.get(id);
  if (!pack) throw new Error(`pack ${id} is not in sources.json`);
  if (pack.excluded) throw new Error(`pack ${id} is excluded: ${pack.excluded}`);
  if (ATTRIBUTION_LICENSES.includes(pack.spdx)) {
    if (typeof pack.attribution !== "string" || !pack.attribution.trim()) throw new Error(`pack ${id} is ${pack.spdx}, which needs an attribution in sources.json`);
  } else if (pack.spdx !== "CC0-1.0") {
    throw new Error(`pack ${id} is ${pack.spdx}; only CC0-1.0, or CC-BY-3.0/CC-BY-4.0 with an attribution, is built`);
  }
  return pack;
}

/**
 * An archive pack's content/ may be used only if fetch.mjs unpacked it from the archive sources.json
 * pins (its fetched.json says so) and that archive is still there with that hash. Returns the hash.
 */
function verifiedArchive(pack) {
  const dir = join(artSrc, pack.id);
  const archive = inside(join(dir, "download"), pack.archive, `pack ${pack.id} archive`);
  const marker = join(dir, "fetched.json");
  if (!existsSync(archive) || !existsSync(marker)) throw new Error(`art-src/${pack.id} is not fetched; run fetch.mjs`);
  const unpacked = JSON.parse(readFileSync(marker, "utf8")).sha256;
  if (unpacked !== pack.sha256) throw new Error(`art-src/${pack.id}/content was unpacked from an archive with sha256 ${unpacked}, but sources.json pins ${pack.sha256}; run fetch.mjs`);
  const actual = hashOf(archive);
  if (actual !== pack.sha256) throw new Error(`art-src/${pack.id}/download/${pack.archive} has sha256 ${actual}, but sources.json pins ${pack.sha256}; run fetch.mjs`);
  return actual;
}

/** A file of a pack of single files, refused unless it has its pack's licence and the sha256 it is pinned to. Returns the hash. */
function verifiedFile(pack, name) {
  const entry = pack.files.find((file) => file.name === name);
  if (!entry) throw new Error(`${name} is not a file of pack ${pack.id}`);
  // LICENSES.json records one licence a pack, so every file used must have the pack's.
  if (entry.spdx !== pack.spdx) throw new Error(`${pack.id}/${name} is ${entry.spdx}, not the pack's ${pack.spdx}`);
  const path = inside(join(artSrc, pack.id, "content"), name, `pack ${pack.id} file`);
  if (!existsSync(path)) throw new Error(`art-src/${pack.id}/content/${name} is missing; run fetch.mjs`);
  const actual = hashOf(path);
  if (actual !== entry.sha256) throw new Error(`art-src/${pack.id}/content/${name} has sha256 ${actual}, but sources.json pins ${entry.sha256}; run fetch.mjs`);
  return actual;
}

/**
 * A file inside art-src/<pack>/content/, refused if the path leaves the pack, its licence is not one
 * a build may use, or its content no longer matches the sha256 sources.json pins.
 */
export function packFile(packId, path) {
  const pack = licensedPack(packId);
  const content = join(artSrc, packId, "content");
  const full = inside(content, path, `${packId} path`);
  if (pack.files) verifiedFile(pack, path);
  else verifiedArchive(pack);
  if (!existsSync(full)) throw new Error(`art-src/${packId}/content/${path} is missing; run fetch.mjs`);
  return full;
}

/**
 * The pack folder (art-src/<id>/) a file inside art-src/ belongs to: a glTF there may only name
 * buffers and images inside its own pack, which is the licence that was checked.
 */
export function packRoot(path) {
  const within = relative(artSrc, resolve(path));
  const id = within.split(sep)[0];
  if (!within || within.startsWith("..") || !packs.has(id)) throw new Error(`${path} is not inside a pack in art-src/`);
  return join(artSrc, id);
}

/** What a build used from a pack: the downloads it came from, with the sha256 measured now. */
export function downloadsUsed(packId, paths) {
  const pack = licensedPack(packId);
  if (!pack.files) return [{ file: pack.archive, url: pack.download, sha256: verifiedArchive(pack) }];
  return [...new Set(paths)].sort().map((path) => {
    const entry = pack.files.find((file) => file.name === path);
    return { file: path, url: entry?.download, sha256: verifiedFile(pack, path) };
  });
}
