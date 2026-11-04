// Builds a map piece from a pack model, baked to the exact size of the box the server collides
// with (or to given proportions), standing on y = 0 with its long side along X.
import { chain, identity, rotation, round, scaling, translation } from "./math.mjs";
import { bakeStatic } from "./normalize.mjs";
import { hexToLinear, paletteAndJoin, readSource, recolor, sceneOf, worldVertices, writeGlb } from "./scene.mjs";

/**
 * spec:
 *   path      the source model
 *   size      [x, y, z] metres to fit exactly, x the long side; or
 *   scale     (size) => [sx, sy, sz], given the source's size after any turn
 *   anchor    "center" (default): the bottom centre goes to the origin; "origin": only the
 *             bottom moves to y = 0 and the source's own x/z origin stays (a lamp's pole)
 *   recolor   { material: "#rrggbb" }
 *   emissive  { material: "#rrggbb" }
 *   split     { node: [material, ...] }: one node and mesh per entry, for instanced tinting; the
 *             material named first becomes white so an instance colour shows unchanged
 */
export async function buildKit(spec) {
  const doc = await readSource(spec.path);
  const changes = [];
  if (spec.recolor) {
    recolor(doc, spec.recolor);
    changes.push(`재질 색 변경: ${Object.entries(spec.recolor).map(([name, hex]) => `${name} → ${hex}`).join(", ")}`);
  }
  for (const [name, hex] of Object.entries(spec.emissive ?? {})) {
    const material = doc.getRoot().listMaterials().find((item) => item.getName() === name);
    if (!material) throw new Error(`no material ${name}`);
    material.setEmissiveFactor(hexToLinear(hex));
    changes.push(`${name} 재질에 발광색 ${hex}`);
  }

  const points = worldVertices(doc);
  const min = [0, 1, 2].map((axis) => Math.min(...points.map((p) => p[axis])));
  const max = [0, 1, 2].map((axis) => Math.max(...points.map((p) => p[axis])));
  const size = max.map((value, axis) => value - min[axis]);
  const turned = !!spec.size && size[0] >= size[2] !== spec.size[0] >= spec.size[2];
  const turnedSize = turned ? [size[2], size[1], size[0]] : size;
  const scale = spec.size ? spec.size.map((target, axis) => target / turnedSize[axis]) : spec.scale(turnedSize, { min, max });
  const anchor = spec.anchor === "origin" ? [0, min[1], 0] : [(min[0] + max[0]) / 2, min[1], (min[2] + max[2]) / 2];
  const matrix = chain(scaling(scale), turned ? rotation("y", 90) : identity(), translation(anchor.map((v) => -v)));
  bakeStatic(doc, matrix);
  const final = turnedSize.map((value, axis) => round(value * scale[axis], 4));
  changes.push(
    `${final.join(" × ")} m에 맞춤 (축별 배율 ${scale.map((value) => round(value, 4)).join(" × ")}${turned ? ", Y축으로 90° 돌림" : ""}), 바닥을 y = 0에`,
  );

  if (spec.split) splitByMaterial(doc, spec.split, changes);
  else {
    await paletteAndJoin(doc);
    changes.push("재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침");
  }
  changes.push("meshopt 압축");
  return { bytes: await writeGlb(doc), changes, info: { size: final } };
}

/** One node and mesh per group of materials, so a runtime can draw and tint each on its own. */
function splitByMaterial(doc, groups, changes) {
  const scene = sceneOf(doc);
  const primitives = doc.getRoot().listMeshes().flatMap((mesh) => mesh.listPrimitives().map((primitive) => [mesh, primitive]));
  for (const [name, materials] of Object.entries(groups)) {
    const mesh = doc.createMesh(name);
    for (const [owner, primitive] of primitives) {
      const material = primitive.getMaterial();
      if (!materials.includes(material?.getName())) continue;
      owner.removePrimitive(primitive);
      for (const semantic of primitive.listSemantics()) if (semantic.startsWith("TEXCOORD_")) primitive.setAttribute(semantic, null);
      mesh.addPrimitive(primitive);
    }
    if (!mesh.listPrimitives().length) throw new Error(`no primitive uses ${materials.join(", ")}`);
    const material = mesh.listPrimitives()[0].getMaterial().setName(name).setMetallicFactor(0).setRoughnessFactor(1);
    if (name === Object.keys(groups)[0]) material.setBaseColorFactor([1, 1, 1, 1]);
    scene.addChild(doc.createNode(name).setMesh(mesh));
  }
  for (const node of scene.listChildren()) if (!(node.getName() in groups)) node.dispose();
  changes.push(`재질별로 ${Object.keys(groups).join("·")} 노드를 나눔 (${Object.keys(groups)[0]}은 흰색: 인스턴스마다 색을 곱해 칠함)`);
}

/** A recipe for map/<file>: spec as for buildKit, with the source's path inside the pack. */
export function kitRecipe({ output, pack, source, ...spec }) {
  return { output, pack, sources: [source], build: (ctx) => buildKit({ ...spec, path: ctx.file(source) }) };
}
