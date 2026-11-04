// Small helpers every recipe shares: reading a source quietly, walking its meshes, merging flat
// materials into a palette, and writing the finished GLB with meshopt compression.
import { Logger } from "@gltf-transform/core";
import { dedup, join, meshopt, palette, prune, unpartition } from "@gltf-transform/functions";
import { MeshoptEncoder } from "meshoptimizer";
import { getIO, measure, readDocument } from "../inspect.mjs";
import { packRoot } from "./sources.mjs";

/** gltf-transform logs every step at INFO; only its warnings matter here. */
export const quiet = new Logger(Logger.Verbosity.WARN);

/** A pack's model, whose external buffers and images must lie inside that same pack. */
export async function readSource(path) {
  const doc = await readDocument(path, [packRoot(path)]);
  doc.setLogger(quiet);
  return doc;
}

export function sceneOf(doc) {
  const root = doc.getRoot();
  return root.getDefaultScene() ?? root.listScenes()[0];
}

/** Every vertex of every static mesh in the scene, in world space. */
export function worldVertices(doc) {
  const points = [];
  sceneOf(doc).traverse((node) => {
    const mesh = node.getMesh();
    if (!mesh) return;
    const m = node.getWorldMatrix();
    for (const primitive of mesh.listPrimitives()) {
      const position = primitive.getAttribute("POSITION");
      const p = [0, 0, 0];
      for (let i = 0; i < position.getCount(); i++) {
        position.getElement(i, p);
        points.push([
          m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
          m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
          m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]);
      }
    }
  });
  return points;
}

export const triangles = (doc) => measure(doc).triangles;

/** Sets each named material's base colour, given as sRGB hex. */
export function recolor(doc, table) {
  let count = 0;
  for (const material of doc.getRoot().listMaterials()) {
    const hex = table[material.getName()];
    if (!hex) continue;
    material.setBaseColorFactor([...hexToLinear(hex), material.getBaseColorFactor()[3]]);
    count++;
  }
  if (count !== Object.keys(table).length) throw new Error(`recolor matched ${count} of ${Object.keys(table).join(", ")}`);
}

export function hexToLinear(hex) {
  const value = Number.parseInt(hex.slice(1), 16);
  return [16, 8, 0].map((shift) => (((value >> shift) & 255) / 255) ** 2.2);
}

/**
 * Turns every flat-coloured material into one palette texture and material, then joins the
 * primitives of sibling static meshes that now share it: one draw call per model. Metalness and
 * roughness are levelled first, since the game draws Lambert and would only get a second palette.
 * join's cleanup also drops the empty nodes left behind, so named empties (a muzzle) go in after.
 */
export async function paletteAndJoin(doc) {
  for (const material of doc.getRoot().listMaterials()) material.setMetallicFactor(0).setRoughnessFactor(1);
  // Texture coordinates no texture reads would keep merged-in parts from joining the rest.
  for (const mesh of doc.getRoot().listMeshes()) {
    for (const primitive of mesh.listPrimitives()) {
      const material = primitive.getMaterial();
      if (material?.getBaseColorTexture() || material?.getEmissiveTexture()) continue;
      for (const semantic of primitive.listSemantics()) if (semantic.startsWith("TEXCOORD_")) primitive.setAttribute(semantic, null);
    }
  }
  await doc.transform(palette({ min: 2 }), join({ keepNamed: false }));
}

/** Prunes, then quantizes and meshopt-compresses the document and returns the GLB bytes. */
export async function writeGlb(doc) {
  await MeshoptEncoder.ready;
  await doc.transform(
    // A merged-in model brings its own buffer; a GLB holds one.
    unpartition(),
    prune({ keepLeaves: true, keepAttributes: false }),
    dedup(),
    meshopt({ encoder: MeshoptEncoder, level: "high" }),
  );
  return (await getIO()).writeBinary(doc);
}
