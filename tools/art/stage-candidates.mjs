// Copies or converts the candidates listed in candidates.json from art-src/ into
// client/public/art/_candidates/ for the ?art&candidates gallery, measures each staged file and
// writes _candidates/index.json. Nothing there is committed or shipped: the folder is gitignored,
// scripts/check-art.mjs skips it and the production build drops it (client/vite.config.ts).
//
// Usage: node stage-candidates.mjs [--fbx2gltf <path to FBX2glTF>] [candidate-id ...]
//   Run fetch.mjs first. Only CC0-1.0 packs are staged; a pack marked excluded in sources.json is
//   refused. FBX candidates need the FBX2glTF binary (also read from the FBX2GLTF variable); without
//   it they are skipped. The binary only reads the FBX as data and writes a GLB.
//
// Changes made while staging, per candidate: external textures and buffers embedded into one GLB;
// keepClips drops other animations (and resamples the rest); recolor sets material colours by name;
// kitbash merges more models in; simplify decimates to a triangle count; images become WebP at size
// pixels, and luminanceAlpha turns a grey-on-black sprite into white with the grey as alpha.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Logger } from "@gltf-transform/core";
import { cloneDocument, mergeDocuments, prune, resample, simplify, unpartition, weld } from "@gltf-transform/functions";
import { MeshoptSimplifier } from "meshoptimizer";
import sharp from "sharp";
import { getIO, measure, readDocument } from "./inspect.mjs";
import { checkPackPaths, inside } from "./lib/paths.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const repo = join(here, "..", "..");
const artSrc = join(repo, "art-src");
const outRoot = join(repo, "client", "public", "art", "_candidates");

const args = process.argv.slice(2);
const converterAt = args.indexOf("--fbx2gltf");
const fbx2gltf = converterAt >= 0 ? args[converterAt + 1] : process.env.FBX2GLTF;
const wanted = args.filter((arg, i) => !arg.startsWith("--") && (converterAt < 0 || i !== converterAt + 1));

const sources = JSON.parse(readFileSync(join(here, "sources.json"), "utf8"));
const list = JSON.parse(readFileSync(join(here, "candidates.json"), "utf8"));
const packs = new Map(sources.packs.map((pack) => [checkPackPaths(pack).id, pack]));

const problems = [];
const skipped = [];
const staged = [];

function hexToLinear(hex) {
  const value = Number.parseInt(hex.slice(1), 16);
  return [16, 8, 0].map((shift) => (((value >> shift) & 255) / 255) ** 2.2);
}

/**
 * The pack a source is in and its licence: the pack and, for a pack of single files, the file entry
 * must both be CC0. Every file a candidate reads is checked, its kitbash parts included.
 */
function licensed(packId, source) {
  const pack = packs.get(packId);
  if (!pack) throw new Error(`pack ${packId} is not in sources.json`);
  if (pack.excluded) throw new Error(`pack ${pack.id} is excluded: ${pack.excluded}`);
  const sourceName = source.split("/")[0];
  const file = pack.files?.find((entry) => entry.name === sourceName);
  if (pack.files && !file) throw new Error(`${source} is not a file of pack ${pack.id}`);
  const spdx = file?.spdx ?? pack.spdx;
  if (spdx !== "CC0-1.0") throw new Error(`${pack.id}/${source} is ${spdx}; only CC0-1.0 is staged`);
  return { pack, license: file?.license ?? pack.license };
}

function sourcePath(packId, source) {
  const path = inside(join(artSrc, packId, "content"), source, `${packId} source`);
  if (!existsSync(path)) throw new Error(`${packId}/${source} is missing; run fetch.mjs first`);
  return path;
}

/** A model's external buffers and images may only come from its own pack, whose licence was checked. */
const packRoots = (packId) => [join(artSrc, packId)];

/** An FBX converted to GLB next to the pack's content, redone when the FBX is newer. */
function convertFbx(packId, path) {
  if (!fbx2gltf) return null;
  const outBase = join(artSrc, packId, "converted", basename(path).replace(/\.fbx$/i, ""));
  const glb = `${outBase}.glb`;
  if (!existsSync(glb) || statSync(glb).mtimeMs < statSync(path).mtimeMs) {
    mkdirSync(dirname(outBase), { recursive: true });
    execFileSync(fbx2gltf, ["--binary", "-i", path, "-o", outBase], { stdio: ["ignore", "ignore", "pipe"] });
  }
  return glb;
}

async function kitbash(doc, candidate) {
  const scene = doc.getRoot().getDefaultScene() ?? doc.getRoot().listScenes()[0];
  for (const part of candidate.kitbash) {
    const partDoc = await readDocument(sourcePath(candidate.pack, part.source), packRoots(candidate.pack));
    partDoc.setLogger(quiet);
    const partScene = partDoc.getRoot().getDefaultScene() ?? partDoc.getRoot().listScenes()[0];
    const copied = mergeDocuments(doc, partDoc);
    const copiedScene = copied.get(partScene);
    const holder = doc.createNode(`kitbash:${basename(part.source)}`);
    holder.setTranslation(part.translate ?? [0, 0, 0]);
    if (part.scale) holder.setScale([part.scale, part.scale, part.scale]);
    for (const child of copiedScene.listChildren()) {
      copiedScene.removeChild(child);
      holder.addChild(child);
    }
    scene.addChild(holder);
    copiedScene.dispose();
  }
  await doc.transform(unpartition());
}

function triangles(doc) {
  return measure(doc).triangles;
}

/**
 * Decimates to at most `target` triangles, trying smaller ratios until it fits. A flat-shaded
 * model keeps a separate copy of every vertex per face, which leaves nothing to collapse, so
 * dropNormals removes the normals first: glTF loaders shade a model without normals flat (three.js
 * sets flatShading), and welding can then join the faces.
 */
async function decimate(doc, { triangles: target, dropNormals = false }) {
  await MeshoptSimplifier.ready;
  if (dropNormals) {
    for (const mesh of doc.getRoot().listMeshes()) for (const primitive of mesh.listPrimitives()) primitive.setAttribute("NORMAL", null);
  }
  await doc.transform(weld());
  const start = triangles(doc);
  if (start <= target) return doc;
  let ratio = target / start;
  for (let attempt = 0; attempt < 8; attempt++) {
    const trial = cloneDocument(doc);
    trial.setLogger(quiet);
    await trial.transform(simplify({ simplifier: MeshoptSimplifier, ratio, error: 0.02, lockBorder: false }));
    const count = triangles(trial);
    if (count <= target) return trial;
    ratio *= (target / count) * 0.97;
  }
  throw new Error(`could not decimate below ${target} triangles`);
}

/** gltf-transform logs every step at INFO; only its warnings matter here. */
const quiet = new Logger(Logger.Verbosity.WARN);

async function stageModel(candidate, path) {
  let doc = await readDocument(path, packRoots(candidate.pack));
  doc.setLogger(quiet);
  const changes = [];
  if (candidate.kitbash) {
    await kitbash(doc, candidate);
    changes.push(`merged ${candidate.kitbash.map((part) => part.source).join(", ")}`);
  }
  if (candidate.keepClips) {
    const keep = new Set(candidate.keepClips);
    const animations = doc.getRoot().listAnimations();
    const missing = candidate.keepClips.filter((name) => !animations.some((animation) => animation.getName() === name));
    if (missing.length) throw new Error(`no clip ${missing.join(", ")}`);
    for (const animation of animations) if (!keep.has(animation.getName())) animation.dispose();
    // Drops keyframes that lie on the line between their neighbours; the motion is unchanged.
    await doc.transform(resample());
    changes.push(`kept ${keep.size} of ${animations.length} clips, resampled`);
  }
  if (candidate.recolor) {
    const table = typeof candidate.recolor === "string" ? list.recolor[candidate.recolor] : candidate.recolor;
    if (!table) throw new Error(`no recolor table ${candidate.recolor}`);
    let count = 0;
    for (const material of doc.getRoot().listMaterials()) {
      const hex = table[material.getName()];
      if (!hex) continue;
      material.setBaseColorFactor([...hexToLinear(hex), material.getBaseColorFactor()[3]]);
      count++;
    }
    changes.push(`recoloured ${count} material(s)`);
  }
  if (candidate.simplify) {
    const before = triangles(doc);
    doc = await decimate(doc, candidate.simplify);
    changes.push(`decimated ${before} to ${triangles(doc)} triangles${candidate.simplify.dropNormals ? ", normals dropped (flat shading)" : ""}`);
  }
  // Unused accessors and materials go; nodes stay as authored, bone ends included.
  await doc.transform(prune({ keepLeaves: true, keepAttributes: true }));
  const bytes = await (await getIO()).writeBinary(doc);
  const out = inside(outRoot, candidate.out, `${candidate.id} out`);
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, bytes);
  // Measure what was written, as the gallery will load it.
  const info = measure(await readDocument(out, [outRoot]));
  return { bytes: bytes.length, changes, info };
}

/**
 * Kenney's particles are grey shapes on opaque black, made for additive blending. luminanceAlpha
 * turns one into white with the grey as its alpha, so normal blending (smoke, dust) works too and
 * additive blending looks the same as before.
 */
async function luminanceToAlpha(image) {
  const { data, info } = await image.extractChannel(0).raw().toBuffer({ resolveWithObject: true });
  const rgba = Buffer.alloc(info.width * info.height * 4, 255);
  for (let i = 0; i < data.length; i++) rgba[i * 4 + 3] = data[i];
  return sharp(rgba, { raw: { width: info.width, height: info.height, channels: 4 } });
}

async function stageImage(candidate, path) {
  const out = inside(outRoot, candidate.out, `${candidate.id} out`);
  mkdirSync(dirname(out), { recursive: true });
  const size = candidate.size ?? 256;
  let image = sharp(path).resize(size, size, { fit: "inside", withoutEnlargement: true });
  const changes = [];
  if (candidate.luminanceAlpha) {
    image = await luminanceToAlpha(image);
    changes.push("white with luminance as alpha");
  }
  const { data, info } = await image.webp({ quality: 88, alphaQuality: 100, effort: 5 }).toBuffer({ resolveWithObject: true });
  writeFileSync(out, data);
  changes.unshift(`WebP ${info.width}x${info.height}`);
  return { bytes: data.length, changes, image: { width: info.width, height: info.height } };
}

function entry(candidate, pack, license, result) {
  const base = {
    id: candidate.id,
    category: candidate.category,
    role: candidate.role,
    title: candidate.title ?? basename(candidate.source),
    pick: candidate.pick === true,
    notes: candidate.notes ?? "",
    file: `_candidates/${candidate.out}`,
    bytes: result.bytes,
    pack: { id: pack.id, title: pack.title, author: pack.author, page: pack.page, license, spdx: "CC0-1.0" },
    source: `${pack.id}/${candidate.source}`,
    changes: result.changes,
  };
  if (result.image) return { ...base, image: result.image, ...(candidate.tile ? { tile: candidate.tile } : {}) };
  const info = result.info;
  return {
    ...base,
    triangles: info.triangles,
    drawCalls: info.drawCalls,
    materials: info.materials.map((material) => material.name),
    joints: Math.max(0, ...info.skins.map((skin) => skin.joints)),
    clips: info.clips.map((clip) => ({ name: clip.name, duration: clip.duration })),
    fit: info.fit,
    character: info.character,
    ...(candidate.uniform ? { uniform: candidate.uniform } : {}),
    ...(candidate.length ? { length: candidate.length } : {}),
    ...(candidate.target ? { target: candidate.target } : {}),
    ...(candidate.scale ? { scale: candidate.scale } : {}),
    ...(candidate.flip ? { flip: true } : {}),
    ...(candidate.turn ? { turn: candidate.turn } : {}),
  };
}

const README = `# _candidates (로컬 전용)

\`?art&candidates\` 갤러리에서 쓰는 후보 에셋입니다. \`tools/art/fetch.mjs\`가 \`art-src/\`에 받은
팩에서 \`tools/art/stage-candidates.mjs\`가 복사·변환했습니다. \`index.json\`에 파일마다 팩, 라이선스,
삼각형 수, 크기가 있습니다.

- 커밋하지 않습니다: 이 폴더는 \`.gitignore\`에 있습니다.
- 배포하지 않습니다: \`client/vite.config.ts\`가 \`vite build\` 뒤에 \`dist/art/_candidates\`를 지우고,
  \`client/scripts/check-art.mjs\`가 이 폴더를 건너뛰므로 \`LICENSES.json\`에 등록할 필요가 없습니다.
- 갤러리는 Vite 개발 서버(\`npm run dev\`)의 \`/?art&candidates\`에서 엽니다.
- \`tools/art/\`에서 \`node stage-candidates.mjs\`로 언제든 다시 만들 수 있고, 폴더를 지워도 됩니다.
`;

const selected = list.candidates.filter((candidate) => !wanted.length || wanted.includes(candidate.id));
if (!wanted.length) rmSync(outRoot, { recursive: true, force: true });
mkdirSync(outRoot, { recursive: true });
const previous = wanted.length && existsSync(join(outRoot, "index.json")) ? JSON.parse(readFileSync(join(outRoot, "index.json"), "utf8")).candidates : [];

for (const candidate of selected) {
  try {
    const { pack, license } = licensed(candidate.pack, candidate.source);
    for (const part of candidate.kitbash ?? []) licensed(candidate.pack, part.source);
    let path = sourcePath(candidate.pack, candidate.source);
    if (candidate.convert === "fbx") {
      path = convertFbx(candidate.pack, path);
      if (!path) {
        skipped.push(`${candidate.id}: no FBX2glTF (pass --fbx2gltf or set FBX2GLTF)`);
        continue;
      }
    }
    const result = /\.(png|jpe?g)$/i.test(path) ? await stageImage(candidate, path) : await stageModel(candidate, path);
    staged.push(entry(candidate, pack, license, result));
    const info = result.info;
    console.log(
      `${candidate.id}: ${candidate.out} ${(result.bytes / 1024).toFixed(0)} KB` +
        (info ? `, ${info.triangles} tris, ${info.drawCalls} prims` : "") +
        (result.changes.length ? ` (${result.changes.join("; ")})` : ""),
    );
  } catch (error) {
    problems.push(`${candidate.id}: ${error.message}`);
    console.error(`${candidate.id}: FAILED ${error.message}`);
  }
}

const byId = new Map(previous.map((item) => [item.id, item]));
for (const item of staged) byId.set(item.id, item);
const order = new Map(list.candidates.map((candidate, i) => [candidate.id, i]));
const candidates = [...byId.values()].filter((item) => order.has(item.id)).sort((a, b) => order.get(a.id) - order.get(b.id));
writeFileSync(
  join(outRoot, "index.json"),
  JSON.stringify({ $comment: "Written by tools/art/stage-candidates.mjs; local only, never committed or shipped.", generated: new Date().toISOString(), candidates }, null, 2) + "\n",
);
writeFileSync(join(outRoot, "README.md"), README);

for (const line of skipped) console.log(`skipped ${line}`);
console.log(`staged ${staged.length} candidate(s) into client/public/art/_candidates/ (${candidates.length} in index.json)`);
if (problems.length) {
  console.error(`stage: ${problems.length} candidate(s) failed`);
  process.exitCode = 1;
}
