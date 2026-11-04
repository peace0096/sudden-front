// Checks for the paths that come from this folder's data files (sources.json, candidates.json).
// fetch.mjs writes and deletes under art-src/ by them and build-art.mjs imports recipes/, so a
// crafted value must never name a path outside the folder it belongs in.
import { resolve, sep } from "node:path";

/** A pack id: lowercase letters and digits, joined by single hyphens. */
const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
/** A file name with no folder in it, starting with a letter or digit (so never "." or ".."). */
const FILE_NAME = /^[A-Za-z0-9][A-Za-z0-9 ._()+-]*$/;

export function slug(value, what) {
  if (typeof value !== "string" || !SLUG.test(value)) throw new Error(`${what} ${JSON.stringify(value)} is not a slug (a-z, 0-9 and single hyphens)`);
  return value;
}

export function fileName(value, what) {
  if (typeof value !== "string" || !FILE_NAME.test(value)) throw new Error(`${what} ${JSON.stringify(value)} is not a plain file name`);
  return value;
}

/** `path` resolved against `root`, refused unless it lies below it (the root itself is refused too). */
export function inside(root, path, what) {
  if (typeof path !== "string" || path === "" || path.includes("\0")) throw new Error(`${what} ${JSON.stringify(path)} is not a path`);
  const base = resolve(root);
  const full = resolve(base, path);
  if (!full.startsWith(base + sep)) throw new Error(`${what} ${JSON.stringify(path)} leaves ${base}`);
  return full;
}

/** Refuses a sources.json pack whose id, archive or file names could lead out of art-src/<id>/. */
export function checkPackPaths(pack) {
  const id = slug(pack?.id, "pack id");
  if (pack.archive !== undefined) fileName(pack.archive, `pack ${id} archive`);
  for (const file of pack.files ?? []) fileName(file?.name, `pack ${id} file`);
  return pack;
}
