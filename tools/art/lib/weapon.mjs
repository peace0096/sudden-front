// Builds a game weapon from a pack model: metres at the role's length, barrel along −Z, +Y up,
// the grip at the origin, a `muzzle` node at the barrel tip, one draw call. A weapon held by a
// handle along its length (the knife) is turned instead so the handle runs up the grip's line
// through the fist, and names the way its tip points.
import { addBox, mergePart } from "./kitbash.mjs";
import { add, chain, dot, normalize, rotation, round, scale as times, scaling, translation } from "./math.mjs";
import { bakeStatic } from "./normalize.mjs";
import { paletteAndJoin, readSource, recolor, sceneOf, worldVertices, writeGlb } from "./scene.mjs";

/**
 * spec:
 *   path      the source model
 *   length    metres from end to end along the barrel
 *   grip      the grip's centre in the source's coordinates, which becomes the origin
 *   turn      [axis, degrees] rotations, in order, that point the barrel along −Z (default: +X → −Z)
 *   tipAxis   [x, y, z] the way the tip points once turned, where it is not −Z (the knife's blade)
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
  const aim = spec.tipAxis ? `끝이 (${spec.tipAxis.join(", ")}) 방향(주먹이 쥐는 손잡이 축)을 향하고` : "총구가 −Z를 향하고";
  changes.push(`${aim} 손잡이가 원점에 오도록 돌리고 옮김, 길이 ${spec.length} m로 배율 조정 (×${round(scale, 4)})`);

  await paletteAndJoin(doc);
  changes.push("재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침");
  const finished = worldVertices(doc);
  const tip = (spec.tipAxis ? tipAlong(finished, spec.length, normalize(spec.tipAxis)) : tipOf(finished, spec.length)).map((value) => round(value, 4));
  // The turn must have laid the weapon along tipAxis: its far end then lies that way from the grip.
  if (spec.tipAxis && dot(normalize(tip), normalize(spec.tipAxis)) < Math.cos((5 * Math.PI) / 180)) {
    throw new Error(`the turn does not lay the weapon along tipAxis ${JSON.stringify(spec.tipAxis)}: its tip is at ${JSON.stringify(tip)}`);
  }
  scene.addChild(doc.createNode("muzzle").setTranslation(tip));
  changes.push(spec.tipAxis ? "그 방향의 끝에 muzzle 노드 추가, meshopt 압축" : "총구 끝에 muzzle 노드 추가, meshopt 압축");
  return { bytes: await writeGlb(doc), changes, info: { muzzle: tip, length: spec.length, ...(spec.tipAxis ? { tipAxis: spec.tipAxis } : {}) } };
}

/** The barrel tip, once it points along −Z: the middle of the vertices within 1.5 % of the length of the front. */
function tipOf(points, length) {
  const front = Math.min(...points.map((p) => p[2]));
  const end = points.filter((p) => p[2] <= front + length * 0.015);
  const mid = [0, 1].map((a) => (Math.min(...end.map((p) => p[a])) + Math.max(...end.map((p) => p[a]))) / 2);
  return [mid[0], mid[1], front];
}

/**
 * The tip along the unit direction `axis`: the middle of the vertices within 1.5 % of the length
 * of the furthest one that way, moved out to the furthest's distance.
 */
function tipAlong(points, length, axis) {
  const front = Math.max(...points.map((p) => dot(p, axis)));
  const end = points.filter((p) => dot(p, axis) >= front - length * 0.015);
  const mid = [0, 1, 2].map((a) => (Math.min(...end.map((p) => p[a])) + Math.max(...end.map((p) => p[a]))) / 2);
  return add(mid, times(axis, front - dot(mid, axis)));
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
