// Build gate for the art files (`npm run build` runs it before tsc). Node built-ins only.
//
// client/public/art/LICENSES.json records where every file under public/art/ came from. Paths are
// relative to public/art/:
//   {
//     "packs": {
//       "<pack id>": { "title", "author", "url", "license", "licenseUrl", "retrieved",
//                      "archiveSha256", "attribution"? }
//     },
//     "files": {
//       "<path>": { "pack": "<pack id>", "sourcePath": "<path inside the pack>", "changes": ["..."] }
//     }
//   }
// A pack's license is "CC0-1.0", or "CC-BY-3.0"/"CC-BY-4.0" with a non-empty attribution.
//
// src/game/art/generated.json is the content lock tools/art/build-art.mjs writes: per file its
// sha256 and bytes (with `hash`, a prefix of the sha256, that the game sends as ?v=), and for a
// model its triangle count, which the budgets below read.
//
// The build fails, with every problem listed, when:
// - a file under public/art/ other than LICENSES.json is not in its files, or a listed file is missing;
// - a files entry lacks its pack, sourcePath or changes, or a pack is missing, has an empty field,
//   or has a license other than the above;
// - src/game/art/manifest.json names a file that is not under public/art/, or a file under
//   public/art/ other than LICENSES.json is not named by it;
// - a file under public/art/ other than LICENSES.json has no generated.json record, differs from
//   it in sha256 or size, or is a .glb without a triangle count; a record has no file; or a
//   record's hash is not a prefix of its sha256;
// - public/art/ outgrows budgets.downloadMB, a gun or character file outgrows budgets.fileKB, or
//   src/game/art/generated.json counts more triangles in a gun, character or arms file than
//   budgets.triangles allows;
// - manifest.weapons has entries but not exactly one per WeaponKind in proto/game.proto.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const client = fileURLToPath(new URL("..", import.meta.url));
const artDir = join(client, "public", "art");
// public/art/_candidates/ holds local-only picks for the ?art&candidates gallery, staged by
// tools/art/stage-candidates.mjs. It is gitignored and vite.config.ts drops it from production
// builds, so its files are neither registered in LICENSES.json nor counted against the budgets.
const CANDIDATES = "_candidates";
const KB = 1024;
const MB = 1024 * KB;
const PACK_FIELDS = ["title", "author", "url", "license", "licenseUrl", "retrieved", "archiveSha256"];
const ATTRIBUTION_LICENSES = ["CC-BY-3.0", "CC-BY-4.0"];

const problems = [];
const fail = (message) => problems.push(message);
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const filled = (value) => typeof value === "string" && value.trim() !== "";
const own = (record, key) => (isObject(record) && Object.hasOwn(record, key) ? record[key] : undefined);

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`${relative(client, path).split(sep).join("/")}: ${error.message}`);
    return undefined;
  }
}

/** Every file under dir, as a path relative to public/art/ with forward slashes. */
function listFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (dir === artDir && entry.name === CANDIDATES) return [];
    // macOS Finder writes a .DS_Store into the folders it shows. It is not art, git ignores it
    // (.gitignore) and a checkout never has one, so it is neither registered nor locked.
    if (entry.name === ".DS_Store") return [];
    return entry.isDirectory() ? listFiles(path) : [relative(artDir, path).split(sep).join("/")];
  });
}

/** Every file the manifest names: the value of each `file` key and each entry of a `files` map. */
function namedFiles(value, found = new Set()) {
  if (value === null || typeof value !== "object") return found;
  for (const [key, child] of Object.entries(value)) {
    if (key === "file" && typeof child === "string") found.add(child);
    else if (key === "files" && child !== null && typeof child === "object") {
      for (const file of Object.values(child)) if (typeof file === "string") found.add(file);
    } else namedFiles(child, found);
  }
  return found;
}

const art = listFiles(artDir).filter((path) => path !== "LICENSES.json");
const onDisk = new Set(art);
const size = (path) => statSync(join(artDir, path)).size;

// ------------------------------------------------------------ licenses

const licenses = readJson(join(artDir, "LICENSES.json"));
const packs = own(licenses, "packs");
const listed = own(licenses, "files");
const recorded = isObject(packs) && isObject(listed);
// An unreadable file is reported once, not once more for every art file.
if (licenses !== undefined && !recorded) fail("LICENSES.json needs a packs object and a files object");
if (recorded) {
  for (const path of art) {
    if (own(listed, path) === undefined) fail(`public/art/${path} is not in LICENSES.json files`);
  }
}
for (const [path, entry] of Object.entries(recorded ? listed : {})) {
  if (!onDisk.has(path)) fail(`LICENSES.json lists ${path}, which is not in public/art/`);
  if (!isObject(entry) || !filled(entry.pack) || !filled(entry.sourcePath) || !Array.isArray(entry.changes)) {
    fail(`LICENSES.json files["${path}"] needs a pack, a sourcePath and a changes list`);
  } else if (own(packs, entry.pack) === undefined) {
    fail(`LICENSES.json files["${path}"] is from pack "${entry.pack}", which is not in packs`);
  }
}
for (const [id, pack] of Object.entries(recorded ? packs : {})) {
  const empty = PACK_FIELDS.filter((field) => !filled(own(pack, field)));
  if (empty.length) {
    fail(`LICENSES.json pack "${id}" has no ${empty.join(", ")}`);
  } else if (ATTRIBUTION_LICENSES.includes(pack.license)) {
    if (!filled(pack.attribution)) fail(`LICENSES.json pack "${id}" is ${pack.license}, which needs a non-empty attribution`);
  } else if (pack.license !== "CC0-1.0") {
    fail(`LICENSES.json pack "${id}" is ${pack.license}; only CC0-1.0, or CC-BY-3.0/CC-BY-4.0 with an attribution, are allowed`);
  }
}

// ------------------------------------------------------------ manifest

const manifest = readJson(join(client, "src", "game", "art", "manifest.json"));
const generated = readJson(join(client, "src", "game", "art", "generated.json"));

const named = namedFiles(manifest);
for (const path of named) {
  if (!onDisk.has(path)) fail(`manifest.json names ${path}, which is not in public/art/`);
}
for (const path of art) {
  if (manifest !== undefined && !named.has(path)) fail(`public/art/${path} is not named by manifest.json`);
}

// ------------------------------------------------------------ content lock

const records = own(generated, "files");
if (generated !== undefined && !isObject(records)) fail("generated.json needs a files object");
if (isObject(records)) {
  for (const path of art) {
    const record = own(records, path);
    if (!isObject(record)) {
      fail(`public/art/${path} has no record in generated.json; run tools/art/build-art.mjs`);
      continue;
    }
    if (!filled(record.sha256) || !Number.isInteger(record.bytes)) {
      fail(`generated.json files["${path}"] needs its sha256 and bytes`);
      continue;
    }
    const bytes = readFileSync(join(artDir, path));
    const sha256 = createHash("sha256").update(bytes).digest("hex");
    if (sha256 !== record.sha256 || bytes.length !== record.bytes) {
      fail(`public/art/${path} is ${bytes.length} bytes with sha256 ${sha256}, not the ${record.bytes} bytes with sha256 ${record.sha256} build-art.mjs made`);
    }
    if (!filled(record.hash) || !record.sha256.startsWith(record.hash)) fail(`generated.json files["${path}"].hash must be a prefix of its sha256`);
    if (path.endsWith(".glb") && !(Number.isInteger(record.triangles) && record.triangles >= 0)) fail(`generated.json has no triangle count for ${path}`);
  }
  for (const path of Object.keys(records)) {
    if (!onDisk.has(path)) fail(`generated.json has a record for ${path}, which is not in public/art/`);
  }
}

const budgets = own(manifest, "budgets");
/** A budget from the manifest, such as "fileKB.gun". */
function budget(name) {
  const value = name.split(".").reduce((node, key) => own(node, key), budgets);
  if (typeof value === "number" && value > 0) return value;
  fail(`manifest.json budgets.${name} must be a positive number`);
  return Infinity;
}

const total = art.reduce((sum, path) => sum + size(path), 0);
const downloadMB = budget("downloadMB");
if (total > downloadMB * MB) fail(`public/art/ holds ${(total / MB).toFixed(2)} MB, over budgets.downloadMB (${downloadMB})`);

const valuesOf = (record) => (isObject(record) ? Object.values(record) : []);
const weapons = own(manifest, "weapons");
const groups = [
  {
    name: "gun",
    files: valuesOf(weapons).map((weapon) => own(weapon, "file")),
    kb: budget("fileKB.gun"),
    triangles: budget("triangles.gun"),
  },
  {
    name: "character",
    files: valuesOf(own(own(manifest, "character"), "files")),
    kb: budget("fileKB.character"),
    triangles: budget("triangles.character"),
  },
  { name: "arms", files: [own(own(manifest, "arms"), "file")], kb: Infinity, triangles: budget("triangles.arms") },
];
for (const group of groups) {
  for (const path of new Set(group.files)) {
    if (typeof path !== "string" || !onDisk.has(path)) continue;
    const bytes = size(path);
    if (bytes > group.kb * KB) fail(`${path} is ${(bytes / KB).toFixed(1)} KB, over budgets.fileKB.${group.name} (${group.kb})`);
    const triangles = own(own(own(generated, "files"), path), "triangles");
    if (typeof triangles === "number" && triangles > group.triangles) {
      fail(`${path} has ${triangles} triangles, over budgets.triangles.${group.name} (${group.triangles})`);
    }
  }
}

// ------------------------------------------------------------ weapon coverage

// protobuf-es drops the WEAPON_KIND_ prefix, so the generated enum, and manifest.json with it, say RIFLE.
const proto = readFileSync(join(client, "..", "proto", "game.proto"), "utf8").replace(/\/\/.*$/gm, "");
const enumBody = /\benum\s+WeaponKind\s*\{([^}]*)\}/.exec(proto)?.[1] ?? "";
const kinds = [...enumBody.matchAll(/\bWEAPON_KIND_(\w+)\s*=/g)].map((match) => match[1]).filter((name) => name !== "UNSPECIFIED");
if (kinds.length === 0) fail("proto/game.proto has no enum WeaponKind values");
const drawn = Object.keys(isObject(weapons) ? weapons : {});
if (drawn.length > 0) {
  const missing = kinds.filter((kind) => !drawn.includes(kind));
  const unknown = drawn.filter((kind) => !kinds.includes(kind));
  if (missing.length) fail(`manifest.json weapons has no ${missing.join(", ")}: once one weapon has art, every WeaponKind needs it`);
  if (unknown.length) fail(`manifest.json weapons has ${unknown.join(", ")}, which are not WeaponKind names`);
}

const plural = (count, noun) => `${count} ${noun}${count === 1 ? "" : "s"}`;
if (problems.length) {
  for (const problem of problems) console.error(`check-art: ${problem}`);
  console.error(`check-art: failed with ${plural(problems.length, "problem")}`);
  process.exit(1);
}
console.log(
  `check-art: OK, ${plural(art.length, "art file")} (${(total / KB).toFixed(1)} KB of ${downloadMB} MB), ` +
    `${drawn.length}/${kinds.length} weapons, ${plural(Object.keys(packs).length, "license pack")}`,
);
