// Builds a game weapon from a pack model: metres at the role's length, barrel along −Z, +Y up,
// the grip at the origin, a `muzzle` node at the barrel tip, one draw call.
import { addBox, mergePart } from "./kitbash.mjs";
import { chain, rotation, round, scaling, translation } from "./math.mjs";
import { bakeStatic } from "./normalize.mjs";
import { paletteAndJoin, readSource, recolor, sceneOf, worldVertices, writeGlb } from "./scene.mjs";

/**
 * spec:
 *   path      the source model
 *   length    metres from end to end along the barrel
 *   grip      the grip's centre in the source's coordinates, which becomes the origin
 *   turn      [axis, degrees] rotations, in order, that point the barrel along −Z (default: +X → −Z)
 *   recolor   { material: "#rrggbb" }
 *   parts     [{ path, translate }]: more models merged in, in the source's coordinates
 *   boxes     [{ name, min, max, color }]: flat-coloured boxes added in the source's coordinates
 */
export async function buildWeapon(spec) {
  const doc = await readSource(spec.path);
  const changes = [];
  const scene = sceneOf(doc);

  // Measured on the model alone, before any parts are added.
  const points = worldVertices(doc);
  const min = [0, 1, 2].map((axis) => Math.min(...points.map((p) => p[axis])));
  const max = [0, 1, 2].map((axis) => Math.max(...points.map((p) => p[axis])));
  const size = max.map((value, axis) => value - min[axis]);
  const axis = size.indexOf(Math.max(...size));

  if (spec.recolor) {
    recolor(doc, spec.recolor);
    changes.push(`재질 색 변경: ${Object.entries(spec.recolor).map(([name, hex]) => `${name} → ${hex}`).join(", ")}`);
  }
  for (const part of spec.parts ?? []) {
    await mergePart(doc, part);
    changes.push(`${part.label} 모델을 붙임`);
  }
  for (const item of spec.boxes ?? []) {
    addBox(doc, item);
    changes.push(`${item.label}을 스크립트로 만들어 붙임`);
  }

  // Grip to the origin, barrel along −Z, then the role's length.
  const scale = spec.length / size[axis];
  const turns = (spec.turn ?? [["y", 90]]).map(([about, degrees]) => rotation(about, degrees));
  const matrix = chain(scaling([scale, scale, scale]), ...turns.reverse(), translation(spec.grip.map((v) => -v)));
  bakeStatic(doc, matrix);
  changes.push(`총구가 −Z를 향하고 손잡이가 원점에 오도록 돌리고 옮김, 길이 ${spec.length} m로 배율 조정 (×${round(scale, 4)})`);

  await paletteAndJoin(doc);
  changes.push("재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침");
  const tip = tipOf(worldVertices(doc), spec.length).map((value) => round(value, 4));
  scene.addChild(doc.createNode("muzzle").setTranslation(tip));
  changes.push("총구 끝에 muzzle 노드 추가, meshopt 압축");
  return { bytes: await writeGlb(doc), changes, info: { muzzle: tip, length: spec.length } };
}

/** The barrel tip, once it points along −Z: the middle of the vertices within 1.5 % of the length of the front. */
function tipOf(points, length) {
  const front = Math.min(...points.map((p) => p[2]));
  const end = points.filter((p) => p[2] <= front + length * 0.015);
  const mid = [0, 1].map((a) => (Math.min(...end.map((p) => p[a])) + Math.max(...end.map((p) => p[a]))) / 2);
  return [mid[0], mid[1], front];
}

/** A recipe for weapons/<file>: spec as for buildWeapon, with paths inside the pack. */
export function weaponRecipe({ output, pack, source, parts = [], ...spec }) {
  return {
    output,
    pack,
    sources: [source, ...parts.map((part) => part.source)],
    build: (ctx) =>
      buildWeapon({ ...spec, path: ctx.file(source), parts: parts.map((part) => ({ ...part, path: ctx.file(part.source) })) }),
  };
}
