// Measures glTF/GLB models as data: the node tree and bones, skins, animation clips, triangles,
// materials and bounds in metres, and for a rigged character how many heads tall it stands once
// scaled so its head top is at the game's 1.76 m. Nothing in a model is ever run.
//
// Usage: node inspect.mjs [--json | --summary] [--root <dir>]... <file or directory> ...
//   (default)   a readable report per file
//   --json      one JSON array with every measurement
//   --summary   one Markdown table row per file
//   --root      external buffers and textures are only read from inside these directories
//               (repeatable; default: art-src/ and client/public/art/ at the repository root)
// A directory is searched for .glb and .gltf files.
//
// Bounds are in the rest pose: skinned vertices are placed by their joints as the glTF spec says,
// so a character's height is what the game would see before any clip plays.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { NodeIO } from "@gltf-transform/core";
import { ALL_EXTENSIONS } from "@gltf-transform/extensions";
import { MeshoptDecoder, MeshoptEncoder } from "meshoptimizer";

const repo = fileURLToPath(new URL("../..", import.meta.url));
const DEFAULT_ROOTS = [join(repo, "art-src"), join(repo, "client", "public", "art")];

/** The top of the server's standing head box (client/src/game/art/hitbox.ts: HEAD_BASE + HEAD_HEIGHT). */
export const HEAD_TOP = 1.76;
/** A vertex this much weighted to the head bone or its children counts as part of the head. */
const HEAD_WEIGHT = 0.5;

let io = null;
/** A glTF reader and writer that knows every extension and decodes and encodes meshopt compression. */
export async function getIO() {
  if (!io) {
    await Promise.all([MeshoptDecoder.ready, MeshoptEncoder.ready]);
    io = new NodeIO()
      .registerExtensions(ALL_EXTENSIONS)
      .registerDependencies({ "meshopt.decoder": MeshoptDecoder, "meshopt.encoder": MeshoptEncoder });
  }
  return io;
}

// ------------------------------------------------------------------ reading

function splitGlb(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.length < 20 || view.getUint32(0, true) !== 0x46546c67) throw new Error("not a GLB file");
  const jsonLength = view.getUint32(12, true);
  if (view.getUint32(16, true) !== 0x4e4f534a) throw new Error("GLB has no JSON chunk");
  const json = JSON.parse(new TextDecoder().decode(bytes.subarray(20, 20 + jsonLength)));
  const resources = {};
  const binStart = 20 + jsonLength;
  if (bytes.length >= binStart + 8 && view.getUint32(binStart + 4, true) === 0x004e4942) {
    resources["@glb.bin"] = bytes.subarray(binStart + 8, binStart + 8 + view.getUint32(binStart, true));
  }
  return { json, resources };
}

/** Every file under dir with this name. */
function findByName(dir, name, found = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) findByName(path, name, found);
    else if (entry.name === name) found.push(path);
  }
  return found;
}

/**
 * A buffer or image the file names by URI. It must lie inside one of roots. A file whose texture
 * sits in a sibling folder (KayKit's gltf/ beside textures/) is found by name inside the pack.
 */
function readResource(file, uri, roots) {
  const inside = (path) => roots.some((root) => path === root || path.startsWith(root + sep));
  const direct = resolve(dirname(file), decodeURIComponent(uri));
  if (!inside(direct)) throw new Error(`${uri} points outside ${roots.join(", ")}`);
  if (existsSync(direct)) return new Uint8Array(readFileSync(direct));
  const pack = resolve(dirname(file), "..");
  const matches = inside(pack) ? findByName(pack, basename(direct)) : [];
  if (matches.length === 1) return new Uint8Array(readFileSync(matches[0]));
  throw new Error(`${uri} not found next to ${basename(file)}${matches.length ? ` (${matches.length} files share its name)` : ""}`);
}

/** The generator each document's file named, which glTF-Transform replaces with its own on reading. */
const generators = new WeakMap();

export async function readDocument(file, roots = DEFAULT_ROOTS) {
  const bytes = new Uint8Array(readFileSync(file));
  const jsonDoc = file.toLowerCase().endsWith(".gltf")
    ? { json: JSON.parse(new TextDecoder().decode(bytes)), resources: {} }
    : splitGlb(bytes);
  for (const item of [...(jsonDoc.json.buffers ?? []), ...(jsonDoc.json.images ?? [])]) {
    if (!item.uri || item.uri.startsWith("data:") || item.uri in jsonDoc.resources) continue;
    jsonDoc.resources[item.uri] = readResource(resolve(file), item.uri, roots.map((root) => resolve(root)));
  }
  const doc = await (await getIO()).readJSON(jsonDoc);
  generators.set(doc, jsonDoc.json.asset?.generator ?? "");
  return doc;
}

// ------------------------------------------------------------------ maths (column-major 4×4)

function multiply(a, b) {
  const out = new Array(16);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      out[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
    }
  }
  return out;
}

const transform = (m, x, y, z) => [
  m[0] * x + m[4] * y + m[8] * z + m[12],
  m[1] * x + m[5] * y + m[9] * z + m[13],
  m[2] * x + m[6] * y + m[10] * z + m[14],
];

const round = (value, places = 3) => Math.round(value * 10 ** places) / 10 ** places;

// ------------------------------------------------------------------ measuring

export function primitiveTriangles(primitive) {
  const count = primitive.getIndices()?.getCount() ?? primitive.getAttribute("POSITION")?.getCount() ?? 0;
  const mode = primitive.getMode();
  if (mode === 4) return Math.floor(count / 3);
  if (mode === 5 || mode === 6) return Math.max(0, count - 2);
  return 0;
}

/** The bone a name pattern picks: the shortest matching name, so "Head" wins over "HeadTop_End". */
function pickBone(joints, include, exclude) {
  const matches = joints.filter((joint) => include.test(joint.getName()) && !exclude.test(joint.getName()));
  return matches.sort((a, b) => a.getName().length - b.getName().length)[0] ?? null;
}

function descendants(node, found = new Set()) {
  found.add(node);
  for (const child of node.listChildren()) descendants(child, found);
  return found;
}

function textureInfo(texture) {
  if (!texture) return null;
  const size = texture.getSize();
  return {
    name: texture.getName() || texture.getURI() || "(embedded)",
    mimeType: texture.getMimeType(),
    size: size ? `${size[0]}x${size[1]}` : "?",
  };
}

const hex = (rgb) => "#" + rgb.slice(0, 3).map((c) => Math.round(Math.min(1, Math.max(0, c ** (1 / 2.2))) * 255).toString(16).padStart(2, "0")).join("");

/**
 * Calls visit(x, y, z, weights) for every vertex of every mesh in the scene, in world space and
 * the rest pose. weights[i] is the share of the vertex skinned to a node in jointSets[i].
 */
function eachVertex(scene, jointSets, visit) {
  const ibm = new Array(16);
  const none = jointSets.map(() => 0);
  const nodes = [];
  scene.traverse((node) => nodes.push(node));
  for (const node of nodes) {
    const mesh = node.getMesh();
    if (!mesh) continue;
    const skin = node.getSkin();
    const nodeWorld = node.getWorldMatrix();
    let jointMatrices = null;
    let membership = null;
    if (skin) {
      const joints = skin.listJoints();
      const inverse = skin.getInverseBindMatrices();
      jointMatrices = joints.map((joint, j) => {
        const bind = inverse ? inverse.getElement(j, ibm) : [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
        return multiply(joint.getWorldMatrix(), bind.slice());
      });
      membership = joints.map((joint) => jointSets.map((set) => set.has(joint)));
    }
    for (const primitive of mesh.listPrimitives()) {
      const position = primitive.getAttribute("POSITION");
      if (!position) continue;
      const sets = [0, 1]
        .map((set) => [primitive.getAttribute(`JOINTS_${set}`), primitive.getAttribute(`WEIGHTS_${set}`)])
        .filter(([joints, weights]) => joints && weights);
      const p = [0, 0, 0];
      const j4 = [0, 0, 0, 0];
      const w4 = [0, 0, 0, 0];
      for (let i = 0; i < position.getCount(); i++) {
        position.getElement(i, p);
        let x = 0;
        let y = 0;
        let z = 0;
        let total = 0;
        const shares = jointSets.map(() => 0);
        if (jointMatrices) {
          for (const [joints, weights] of sets) {
            joints.getElement(i, j4);
            weights.getElement(i, w4);
            for (let k = 0; k < 4; k++) {
              const weight = w4[k];
              if (!weight) continue;
              const [tx, ty, tz] = transform(jointMatrices[j4[k]], p[0], p[1], p[2]);
              x += tx * weight;
              y += ty * weight;
              z += tz * weight;
              total += weight;
              membership[j4[k]].forEach((member, s) => member && (shares[s] += weight));
            }
          }
        }
        if (total > 0) visit(x / total, y / total, z / total, shares.map((share) => share / total));
        else {
          const [wx, wy, wz] = transform(nodeWorld, p[0], p[1], p[2]);
          visit(wx, wy, wz, none);
        }
      }
    }
  }
}

/**
 * Joints parked far from all the others, as animators do with parts that only appear in one clip
 * (the speed loader of a revolver sits hundreds of metres below it). A joint is parked when it is
 * further from the joints' median position than ten times the joints' median distance from it.
 */
function parkedJoints(joints) {
  if (joints.length < 3) return new Set();
  const positions = joints.map((joint) => joint.getWorldTranslation());
  const median = [0, 1, 2].map((axis) => positions.map((p) => p[axis]).sort((a, b) => a - b)[Math.floor(positions.length / 2)]);
  const distances = positions.map((p) => Math.hypot(p[0] - median[0], p[1] - median[1], p[2] - median[2]));
  const typical = [...distances].sort((a, b) => a - b)[Math.floor(distances.length / 2)];
  return new Set(joints.filter((_, j) => distances[j] > 10 * Math.max(typical, 1e-6)));
}

export function measure(doc) {
  const root = doc.getRoot();
  const scene = root.getDefaultScene() ?? root.listScenes()[0];
  const skins = root.listSkins();
  const jointSet = new Set(skins.flatMap((skin) => skin.listJoints()));
  const joints = [...jointSet];

  // Node tree, from the scene's roots.
  const tree = (node) => ({
    name: node.getName(),
    ...(node.getMesh() ? { mesh: node.getMesh().getName() } : {}),
    ...(node.getSkin() ? { skin: skins.indexOf(node.getSkin()) } : {}),
    ...(jointSet.has(node) ? { joint: true } : {}),
    ...(node.listChildren().length ? { children: node.listChildren().map(tree) } : {}),
  });
  const nodes = scene ? scene.listChildren().map(tree) : [];

  // Triangles: per mesh once, and in total for every node that draws one.
  const meshes = root.listMeshes().map((mesh) => ({
    name: mesh.getName(),
    primitives: mesh.listPrimitives().length,
    triangles: mesh.listPrimitives().reduce((sum, primitive) => sum + primitiveTriangles(primitive), 0),
    vertexColors: mesh.listPrimitives().some((primitive) => !!primitive.getAttribute("COLOR_0")),
    morphTargets: Math.max(0, ...mesh.listPrimitives().map((primitive) => primitive.listTargets().length)),
  }));
  let triangles = 0;
  let drawCalls = 0;
  scene?.traverse((node) => {
    const mesh = node.getMesh();
    if (!mesh) return;
    const info = meshes[root.listMeshes().indexOf(mesh)];
    triangles += info.triangles;
    drawCalls += info.primitives;
  });

  const materials = root.listMaterials().map((material) => {
    const extensions = material.listExtensions().map((extension) => extension.extensionName);
    const color = material.getBaseColorFactor();
    return {
      name: material.getName(),
      type: extensions.includes("KHR_materials_unlit") ? "unlit" : "pbr",
      extensions,
      color: hex(color),
      alpha: round(color[3], 2),
      alphaMode: material.getAlphaMode(),
      doubleSided: material.getDoubleSided(),
      metallic: round(material.getMetallicFactor(), 2),
      roughness: round(material.getRoughnessFactor(), 2),
      emissive: hex(material.getEmissiveFactor()),
      textures: Object.fromEntries(
        [
          ["baseColor", material.getBaseColorTexture()],
          ["normal", material.getNormalTexture()],
          ["metallicRoughness", material.getMetallicRoughnessTexture()],
          ["occlusion", material.getOcclusionTexture()],
          ["emissive", material.getEmissiveTexture()],
        ]
          .filter(([, texture]) => texture)
          .map(([slot, texture]) => [slot, textureInfo(texture)]),
      ),
    };
  });

  const clips = root.listAnimations().map((animation) => ({
    name: animation.getName(),
    duration: round(Math.max(0, ...animation.listSamplers().map((sampler) => sampler.getInput()?.getMax([])[0] ?? 0)), 3),
    channels: animation.listChannels().length,
  }));

  // Bones a character is fitted by.
  const head = pickBone(joints, /head/i, /end|top|nub|tip|eye|jaw|brow|hair|lid/i);
  const neck = pickBone(joints, /neck/i, /end|nub|tip/i);
  const headSet = head ? descendants(head) : new Set();
  // Feet and toes: a foot's mesh reaches forward from its ankle, which tells which way the model faces.
  const feet = joints.filter((joint) => /foot|toe|ball/i.test(joint.getName()) && !/end|nub|tip/i.test(joint.getName()));
  const footSet = new Set(feet.flatMap((foot) => [...descendants(foot)]));

  const parked = parkedJoints(joints);
  const box = () => ({ min: [Infinity, Infinity, Infinity], max: [-Infinity, -Infinity, -Infinity] });
  const all = box();
  const kept = box();
  const grow = (b, x, y, z) => {
    b.min = [Math.min(b.min[0], x), Math.min(b.min[1], y), Math.min(b.min[2], z)];
    b.max = [Math.max(b.max[0], x), Math.max(b.max[1], y), Math.max(b.max[2], z)];
  };
  let headMinY = Infinity;
  let headVertices = 0;
  let footZ = 0;
  let footVertices = 0;
  if (scene) {
    eachVertex(scene, [headSet, footSet, parked], (x, y, z, [headWeight, footWeight, parkedWeight]) => {
      grow(all, x, y, z);
      if (parkedWeight === 0) grow(kept, x, y, z);
      if (headWeight >= HEAD_WEIGHT) {
        headMinY = Math.min(headMinY, y);
        headVertices++;
      }
      if (footWeight >= HEAD_WEIGHT) {
        footZ += z;
        footVertices++;
      }
    });
  }
  const finish = (b, extra = {}) =>
    Number.isFinite(b.min[0])
      ? { min: b.min.map((v) => round(v)), max: b.max.map((v) => round(v)), size: [0, 1, 2].map((axis) => round(b.max[axis] - b.min[axis])), ...extra }
      : null;
  const bounds = finish(all);
  const fit = parked.size && Number.isFinite(kept.min[0])
    ? finish(kept, { cut: true, parkedJoints: [...parked].map((joint) => joint.getName()) })
    : bounds && { ...bounds, cut: false };

  let character = null;
  if (head && fit && fit.size[1] > 0) {
    const feetAt = fit.min[1];
    const scale = HEAD_TOP / fit.size[1];
    const height = (node) => (node ? round((node.getWorldTranslation()[1] - feetAt) * scale) : null);
    const chin = headVertices ? round((headMinY - feetAt) * scale) : null;
    // The feet's vertices against their ankles: the way the model faces.
    const ankles = feet.filter((foot) => !feet.includes(foot.getParentNode()));
    const forward = footVertices && ankles.length
      ? footZ / footVertices - ankles.reduce((sum, foot) => sum + foot.getWorldTranslation()[2], 0) / ankles.length
      : 0;
    character = {
      headBone: head.getName(),
      neckBone: neck?.getName() ?? null,
      scale: round(scale, 4),
      headTop: HEAD_TOP,
      headJoint: height(head),
      neckJoint: height(neck),
      chin,
      headHeight: chin === null ? null : round(HEAD_TOP - chin),
      headsTall: chin === null ? null : round(HEAD_TOP / (HEAD_TOP - chin), 2),
      facing: Math.abs(forward) < 0.01 * fit.size[1] ? "?" : forward > 0 ? "+Z" : "-Z",
      crouchClips: clips.map((clip) => clip.name).filter((name) => /crouch/i.test(name)),
    };
  }

  const images = root.listTextures().map(textureInfo);
  return {
    generator: generators.get(doc) ?? doc.getRoot().getAsset().generator ?? "",
    extensions: root.listExtensionsUsed().map((extension) => extension.extensionName),
    nodes,
    nodeCount: root.listNodes().length,
    bones: joints.map((joint) => joint.getName()),
    skins: skins.map((skin) => ({ name: skin.getName(), joints: skin.listJoints().length, skeleton: skin.getSkeleton()?.getName() ?? null })),
    clips,
    meshes,
    triangles,
    drawCalls,
    materials,
    textures: images,
    bounds,
    /** The bounds without parts parked far away; equal to bounds unless outliers were cut. */
    fit,
    character,
  };
}

export async function inspectFile(file, roots) {
  const doc = await readDocument(file, roots);
  return { file, bytes: statSync(file).size, ...measure(doc) };
}

// ------------------------------------------------------------------ output

function treeLines(node, depth, lines) {
  const tags = [node.mesh !== undefined && `mesh ${node.mesh}`, node.skin !== undefined && `skin ${node.skin}`, node.joint && "joint"].filter(Boolean);
  lines.push(`${"  ".repeat(depth)}${node.name || "(unnamed)"}${tags.length ? `  [${tags.join(", ")}]` : ""}`);
  for (const child of node.children ?? []) treeLines(child, depth + 1, lines);
  return lines;
}

const metres = (size) => size.map((v) => v.toFixed(2)).join(" × ");

export function report(info, label = info.file) {
  const lines = [`${label}  (${(info.bytes / 1024).toFixed(1)} KB, ${info.generator || "unknown generator"})`];
  if (info.extensions.length) lines.push(`  extensions: ${info.extensions.join(", ")}`);
  lines.push(`  bounds: ${info.bounds ? `${metres(info.bounds.size)} m (x × y × z), min ${info.bounds.min.join(", ")}, max ${info.bounds.max.join(", ")}` : "none"}`);
  if (info.fit?.cut) lines.push(`  fit (far-parked parts left out): ${metres(info.fit.size)} m, min ${info.fit.min.join(", ")}, max ${info.fit.max.join(", ")}`);
  lines.push(`  triangles: ${info.triangles} in ${info.drawCalls} primitive(s)`);
  for (const mesh of info.meshes) {
    lines.push(`    ${mesh.name || "(unnamed)"}: ${mesh.triangles} tris, ${mesh.primitives} prim${mesh.vertexColors ? ", vertex colours" : ""}${mesh.morphTargets ? `, ${mesh.morphTargets} morph targets` : ""}`);
  }
  lines.push(`  materials (${info.materials.length}):`);
  for (const material of info.materials) {
    const textures = Object.entries(material.textures).map(([slot, texture]) => `${slot} ${texture.name} ${texture.size}`);
    lines.push(
      `    ${material.name || "(unnamed)"}: ${material.type} ${material.color}${material.alpha < 1 ? ` a${material.alpha}` : ""} ${material.alphaMode}` +
        `${material.doubleSided ? " double-sided" : ""} metal ${material.metallic} rough ${material.roughness}${textures.length ? `; ${textures.join(", ")}` : ""}`,
    );
  }
  if (info.skins.length) {
    lines.push(`  skins: ${info.skins.map((skin) => `${skin.name || "(unnamed)"} ${skin.joints} joints`).join("; ")}`);
    lines.push(`  bones (${info.bones.length}): ${info.bones.join(", ")}`);
  }
  if (info.clips.length) lines.push(`  clips (${info.clips.length}): ${info.clips.map((clip) => `${clip.name} ${clip.duration}s`).join(", ")}`);
  if (info.character) {
    const c = info.character;
    lines.push(
      `  character: ×${c.scale} puts the head top at ${c.headTop} m; neck joint ${c.neckJoint ?? "-"} m, head joint ${c.headJoint} m, ` +
        `chin ${c.chin ?? "-"} m, head ${c.headHeight ?? "-"} m, ${c.headsTall ?? "-"} heads tall; faces ${c.facing}; ` +
        `crouch clips: ${c.crouchClips.length ? c.crouchClips.join(", ") : "none"}`,
    );
  }
  lines.push(`  nodes (${info.nodeCount}):`);
  for (const node of info.nodes) treeLines(node, 2, lines);
  return lines.join("\n");
}

export const SUMMARY_HEADER = [
  "| file | tris | prims | mats | tex | joints | clips | size m (x×y×z) | heads tall | crouch | KB |",
  "|---|---:|---:|---:|---:|---:|---:|---|---:|---|---:|",
];

export function summaryRow(info, label = info.file) {
  const joints = Math.max(0, ...info.skins.map((skin) => skin.joints));
  return `| ${label} | ${info.triangles} | ${info.drawCalls} | ${info.materials.length} | ${info.textures.length} | ${joints || "-"} | ${info.clips.length || "-"} | ${info.fit ? metres(info.fit.size) + (info.fit.cut ? " (cut)" : "") : "-"} | ${info.character?.headsTall ?? "-"} | ${info.character ? (info.character.crouchClips.length ? "yes" : "no") : "-"} | ${(info.bytes / 1024).toFixed(0)} |`;
}

function collect(path, found = []) {
  if (statSync(path).isDirectory()) {
    for (const entry of readdirSync(path).sort()) collect(join(path, entry), found);
  } else if (/\.(glb|gltf)$/i.test(path)) found.push(path);
  return found;
}

async function main() {
  const args = process.argv.slice(2);
  const mode = args.includes("--json") ? "json" : args.includes("--summary") ? "summary" : "text";
  const rootValues = new Set(args.flatMap((arg, i) => (arg === "--root" && i + 1 < args.length ? [i + 1] : [])));
  const roots = rootValues.size ? [...rootValues].map((i) => resolve(args[i])) : DEFAULT_ROOTS;
  const inputs = args.filter((arg, i) => !arg.startsWith("--") && !rootValues.has(i));
  if (inputs.length === 0) {
    console.error("usage: node inspect.mjs [--json | --summary] [--root <dir>]... <file or directory> ...");
    process.exitCode = 2;
    return;
  }
  let failed = 0;
  const files = inputs.flatMap((input) => {
    if (existsSync(input)) return collect(input);
    failed++;
    console.error(`${input}: no such file or directory`);
    return [];
  });
  const results = [];
  for (const file of files) {
    const label = relative(process.cwd(), file) || file;
    try {
      results.push({ label, info: await inspectFile(file, roots) });
    } catch (error) {
      failed++;
      console.error(`${label}: ${error.message}`);
    }
  }
  if (mode === "json") console.log(JSON.stringify(results.map(({ info }) => info), null, 2));
  else if (mode === "summary") console.log([...SUMMARY_HEADER, ...results.map(({ label, info }) => summaryRow(info, label))].join("\n"));
  else console.log(results.map(({ label, info }) => report(info, label)).join("\n\n"));
  // exitCode rather than exit(): exit() can cut off output still being written to a pipe.
  if (failed) process.exitCode = 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) await main();
