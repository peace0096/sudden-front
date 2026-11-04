// Builds every runtime art file from the packs fetch.mjs put in art-src/, one recipe per output
// (recipes/), into client/public/art/. It also writes what the game and the build check read about
// those files: client/src/game/art/generated.json (hash, sha256, bytes, triangles, image sizes),
// client/public/art/LICENSES.json (each file's pack, source and changes) and docs/credits.md.
// What a recipe reads from a pack is first held to the sha256 sources.json pins (lib/sources.mjs),
// and LICENSES.json records the hashes measured there. The same inputs always give the same bytes,
// so a rerun changes nothing.
//
// Usage: node build-art.mjs [--check]
//   --check  builds everything in memory and fails if any file on disk differs, writing nothing.
//
// Afterwards it holds client/src/game/art/manifest.json (written by hand) to what it built: every
// file the manifest names is built and every built file is named, the fx cells match the atlases,
// and the character's facing, clips and gun-in-hand placement match the rig. A mismatch prints the
// values the manifest should have and fails.
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { pathToFileURL } from "node:url";
import sharp from "sharp";
import { getIO, measure } from "./inspect.mjs";
import { creditsMarkdown } from "./lib/credits.mjs";
import { downloadsUsed, licensedPack, packFile, repoDir, sha256, toolsDir } from "./lib/sources.mjs";

const check = process.argv.includes("--check");
const artDir = join(repoDir, "client", "public", "art");
const generatedPath = join(repoDir, "client", "src", "game", "art", "generated.json");
const licensesPath = join(artDir, "LICENSES.json");
const creditsPath = join(repoDir, "docs", "credits.md");
const manifestPath = join(repoDir, "client", "src", "game", "art", "manifest.json");
const CC0_URL = "https://creativecommons.org/publicdomain/zero/1.0/";

/** Models this repository's scripts build from nothing; recipes/ and lib/shapes.mjs are their source. */
function proceduralPack(recipes) {
  const code = [...recipes.filter((recipe) => recipe.pack === "procedural").map((recipe) => recipe.path), join(toolsDir, "lib", "shapes.mjs")];
  return {
    title: "Sudden Front procedural models",
    author: "Sudden Front",
    url: "tools/art/recipes/",
    license: "CC0-1.0",
    licenseText: "CC0 1.0 (이 저장소의 tools/art 스크립트가 만든 모델)",
    licenseUrl: CC0_URL,
    retrieved: "2026-11-04",
    archiveSha256: sha256(code.map((path) => readFileSync(path)).reduce((all, bytes) => Buffer.concat([all, bytes]), Buffer.alloc(0))),
  };
}

/** A pack's record: its licence and any attribution as sources.json gives them, and the hashes measured in art-src/. */
function licensePack(id, sources) {
  const pack = licensedPack(id);
  const downloads = downloadsUsed(id, sources.map((source) => source.split("/")[0]));
  return {
    title: pack.title,
    author: pack.author,
    url: pack.page,
    license: pack.spdx,
    licenseText: pack.license,
    licenseUrl: pack.licenseUrl,
    ...(pack.attribution ? { attribution: pack.attribution } : {}),
    retrieved: pack.retrieved,
    // A pack fetched file by file has no archive; this hashes the list of the files used and their hashes.
    archiveSha256: pack.files ? sha256(downloads.map((item) => `${item.sha256}  ${item.file}\n`).join("")) : downloads[0].sha256,
    downloads,
  };
}

async function loadRecipes() {
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith(".mjs")) found.push(path);
    }
  };
  walk(join(toolsDir, "recipes"));
  const recipes = [];
  for (const path of found) recipes.push({ ...(await import(pathToFileURL(path).href)).default, path });
  const outputs = recipes.map((recipe) => recipe.output);
  const twice = outputs.filter((output, i) => outputs.indexOf(output) !== i);
  if (twice.length) throw new Error(`more than one recipe writes ${twice.join(", ")}`);
  return recipes;
}

async function describe(output, bytes) {
  const digest = sha256(bytes);
  // hash, a prefix, is what the game sends as ?v=; sha256 and bytes lock the content for check-art.mjs.
  const entry = { hash: digest.slice(0, 16), sha256: digest, bytes: bytes.length };
  if (output.endsWith(".glb")) {
    const doc = await (await getIO()).readBinary(new Uint8Array(bytes));
    entry.triangles = measure(doc).triangles;
    const images = doc.getRoot().listTextures().map((texture) => texture.getSize());
    if (images.length) entry.images = images.map(([width, height]) => ({ width, height }));
  } else {
    const { width, height } = await sharp(bytes).metadata();
    entry.images = [{ width, height }];
  }
  return entry;
}

/** Every file the manifest names: the value of each `file` key and each entry of a `files` map. */
function namedFiles(value, found = new Set()) {
  if (value === null || typeof value !== "object") return found;
  for (const [key, child] of Object.entries(value)) {
    if (key === "file" && typeof child === "string") found.add(child);
    else if (key === "files" && child && typeof child === "object") for (const file of Object.values(child)) found.add(file);
    else namedFiles(child, found);
  }
  return found;
}

const near = (a, b) => Array.isArray(a) && a.length === b.length && a.every((value, i) => Math.abs(value - b[i]) < 1e-3);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function checkManifest(manifest, built) {
  const problems = [];
  const named = namedFiles(manifest);
  for (const file of named) if (!built.has(file)) problems.push(`manifest.json names ${file}, which no recipe builds`);
  for (const file of built.keys()) if (!named.has(file)) problems.push(`${file} is built but manifest.json never names it`);

  const atlas = (file) => built.get(file)?.info?.rects;
  if (manifest.fx && !same(manifest.fx.cells, atlas(manifest.fx.file))) problems.push(`fx.cells should be ${JSON.stringify(atlas(manifest.fx.file))}`);
  if (manifest.fx?.decals && !same(manifest.fx.decals.cells, atlas(manifest.fx.decals.file))) {
    problems.push(`fx.decals.cells should be ${JSON.stringify(atlas(manifest.fx.decals.file))}`);
  }

  const character = manifest.character;
  const rig = character && built.get(character.files.red)?.info;
  if (rig) {
    if (Math.abs(character.forwardYaw - rig.forwardYaw) > 1e-4) problems.push(`character.forwardYaw should be ${rig.forwardYaw}`);
    for (const [state, clip] of Object.entries(character.clips)) if (!rig.clips.includes(clip)) problems.push(`character.clips.${state} names ${clip}, which the rig lacks (it has ${rig.clips.join(", ")})`);
    for (const [kind, weapon] of Object.entries(manifest.weapons)) {
      const { position, rotation } = rig.gripInHand;
      if (!near(weapon.tp.position, position) || !near(weapon.tp.rotation, rotation)) {
        problems.push(`weapons.${kind}.tp should be ${JSON.stringify({ position, rotation })} (the grip in the right palm during holdRifle)`);
      }
    }
  }
  return problems;
}

const recipes = await loadRecipes();
const built = new Map();
for (const recipe of recipes) {
  if (recipe.pack !== "procedural") licensedPack(recipe.pack);
  const ctx = { file: (path) => packFile(recipe.pack, path) };
  const started = Date.now();
  const result = await recipe.build(ctx);
  built.set(recipe.output, { ...result, recipe });
  console.log(`${recipe.output}: ${(result.bytes.length / 1024).toFixed(1)} KB (${Date.now() - started} ms)`);
}

// Records, in a fixed order so the files only change when the art does.
const outputs = [...built.keys()].sort();
const generated = { files: {} };
for (const output of outputs) generated.files[output] = await describe(output, built.get(output).bytes);
const packIds = [...new Set(recipes.map((recipe) => recipe.pack))].sort();
const licenses = {
  $comment: "Written by tools/art/build-art.mjs from tools/art/sources.json and the recipes; do not edit. Each file lists its pack, the source path inside the pack and what the pipeline changed.",
  packs: Object.fromEntries(
    packIds.map((id) => [
      id,
      id === "procedural" ? proceduralPack(recipes) : licensePack(id, recipes.filter((recipe) => recipe.pack === id).flatMap((recipe) => recipe.sources)),
    ]),
  ),
  files: Object.fromEntries(
    outputs.map((output) => {
      const { recipe, changes } = built.get(output);
      return [output, { pack: recipe.pack, sourcePath: recipe.sources.join(", "), changes }];
    }),
  ),
};
const texts = new Map([
  [generatedPath, JSON.stringify(generated, null, 2) + "\n"],
  [licensesPath, JSON.stringify(licenses, null, 2) + "\n"],
  [creditsPath, creditsMarkdown(licenses)],
]);

const managed = [...new Set(outputs.map((output) => output.split("/")[0]))];
if (check) {
  const differ = [];
  for (const output of outputs) {
    const path = join(artDir, output);
    if (!existsSync(path) || sha256(readFileSync(path)) !== sha256(built.get(output).bytes)) differ.push(`client/public/art/${output}`);
  }
  for (const [path, text] of texts) if (!existsSync(path) || readFileSync(path, "utf8") !== text) differ.push(relative(repoDir, path));
  for (const dir of managed) {
    for (const file of readdirSync(join(artDir, dir), { recursive: true, withFileTypes: true })) {
      if (!file.isFile()) continue;
      const path = relative(artDir, join(file.parentPath, file.name)).split(sep).join("/");
      if (!built.has(path)) differ.push(`client/public/art/${path} (no recipe builds it)`);
    }
  }
  if (differ.length) {
    for (const line of differ) console.error(`differs: ${line}`);
    process.exitCode = 1;
  } else console.log(`check: all ${outputs.length} files and the three records match what is on disk`);
} else {
  // Files a recipe no longer builds go, but only inside the folders recipes write to.
  for (const dir of managed) {
    if (!existsSync(join(artDir, dir))) continue;
    for (const file of readdirSync(join(artDir, dir), { recursive: true, withFileTypes: true })) {
      if (!file.isFile()) continue;
      const path = relative(artDir, join(file.parentPath, file.name)).split(sep).join("/");
      if (!built.has(path)) rmSync(join(artDir, path));
    }
  }
  for (const output of outputs) {
    mkdirSync(dirname(join(artDir, output)), { recursive: true });
    writeFileSync(join(artDir, output), built.get(output).bytes);
  }
  for (const [path, text] of texts) writeFileSync(path, text);
  const total = outputs.reduce((sum, output) => sum + built.get(output).bytes.length, 0);
  console.log(`wrote ${outputs.length} files (${(total / 1024).toFixed(1)} KB), generated.json, LICENSES.json and docs/credits.md`);
}

const problems = checkManifest(JSON.parse(readFileSync(manifestPath, "utf8")), built);
const rig = [...built.values()].find((item) => item.info?.gripInHand)?.info;
if (rig) {
  const { bones, clips, ...facts } = rig;
  console.log(`rig: ${JSON.stringify(facts)}`);
  console.log(`rig bones (${bones.length}): ${bones.join(", ")}`);
  console.log(`rig clips: ${clips.join(", ")}`);
}
for (const problem of problems) console.error(`manifest: ${problem}`);
if (problems.length) process.exitCode = 1;
