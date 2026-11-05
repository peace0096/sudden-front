// GRENADE: a hand grenade built here, as no pack has one: an olive body round the middle of the
// fist with a darker band, the fuze on top, the spoon down the palm side and the pin's ring on the
// other. Built with its axis along +Z (lathe's), then laid along the grip's line through the fist
// that tools/art/lib/character.mjs measures (gripAxis: 28.9° forward of straight up, 4.9° to the
// right), as the knife's handle is: the fuze shows above the fist. The body's middle is the origin,
// where the fist closes. The same file is the grenade in flight (manifest projectiles.GRENADE).
import { Document } from "@gltf-transform/core";
import { chain, rotation, round, transformPoint, transformVector, translation } from "../../lib/math.mjs";
import { bakeStatic } from "../../lib/normalize.mjs";
import { addSolid, box, lathe, merge } from "../../lib/shapes.mjs";
import { paletteAndJoin, quiet, sceneOf, writeGlb } from "../../lib/scene.mjs";

const SEGMENTS = 12;
/** Where the fuze's top is along the axis, which the `muzzle` node marks. */
const TOP = 0.058;

/** A solid moved by a rotation-and-translation matrix (normals turned with it). */
function moved(solid, matrix) {
  const positions = [];
  const normals = [];
  for (let i = 0; i < solid.positions.length; i += 3) {
    positions.push(...transformPoint(matrix, solid.positions.slice(i, i + 3)));
    normals.push(...transformVector(matrix, solid.normals.slice(i, i + 3)));
  }
  return { positions, normals, indices: solid.indices };
}

/** A ring round the Z axis: a tube of radius `tube` along a circle of radius `radius`. */
function ring(radius, tube, around, sides) {
  // Counter-clockwise in (r, z), which lathe turns into faces looking out of the tube.
  const profile = Array.from({ length: sides + 1 }, (_, i) => {
    const t = (i / sides) * Math.PI * 2;
    return [radius + tube * Math.cos(t), tube * Math.sin(t)];
  });
  return lathe(profile, around, { back: false, front: false });
}

/** A box from `from` to `to` along its length, `width` across in y and `thick` in the xz plane. */
function strip(from, to, width, thick) {
  const [dx, dz] = [to[0] - from[0], to[2] - from[2]];
  const length = Math.hypot(dx, dz);
  const turn = (Math.atan2(dx, dz) * 180) / Math.PI;
  const piece = box([-thick / 2, -width / 2, 0], [thick / 2, width / 2, length]);
  return moved(piece, chain(translation(from), rotation("y", turn)));
}

export default {
  output: "weapons/grenade.glb",
  pack: "procedural",
  sources: ["tools/art/recipes/weapons/grenade.mjs"],
  async build() {
    const doc = new Document();
    doc.setLogger(quiet);
    doc.createBuffer();
    doc.getRoot().setDefaultScene(doc.createScene("grenade"));

    const body = lathe(
      [[0, -0.04], [0.016, -0.038], [0.027, -0.03], [0.032, -0.016], [0.033, 0], [0.032, 0.014], [0.027, 0.026], [0.018, 0.034], [0.012, 0.036]],
      SEGMENTS,
    );
    addSolid(doc, body, { name: "body", color: "#4f5d2f" });
    addSolid(doc, lathe([[0.0338, -0.005], [0.0338, 0.005]], SEGMENTS, { back: false, front: false }), { name: "band", color: "#3a4523" });
    addSolid(doc, lathe([[0.011, 0.034], [0.011, 0.05], [0.009, 0.055], [0, TOP - 0.002]], SEGMENTS, { back: false, front: true }), {
      name: "fuze",
      color: "#8b8f87",
    });
    // The spoon hugs the body down the palm's side (−X): up from the widest part, in over the
    // shoulder and across the fuze's top.
    const spoon = merge(
      strip([-0.0345, 0, -0.012], [-0.0345, 0, 0.012], 0.011, 0.004),
      strip([-0.0345, 0, 0.012], [-0.014, 0, 0.052], 0.011, 0.004),
      box([-0.016, -0.0055, 0.052], [0.006, 0.0055, 0.056]),
    );
    addSolid(doc, spoon, { name: "spoon", color: "#a3a69d" });
    // The pin's ring stands on the fuze's other side.
    const pin = moved(ring(0.009, 0.0018, 10, 6), chain(translation([0.021, 0, 0.045]), rotation("x", 90)));
    addSolid(doc, pin, { name: "pin", color: "#c9c6b8" });

    // +Z (the axis) onto +Y, then forward and right onto the fist's grip line.
    const lay = chain(rotation("z", -4.9), rotation("x", -28.9), rotation("x", -90));
    bakeStatic(doc, lay);
    await paletteAndJoin(doc);
    const tipAxis = transformVector(lay, [0, 0, 1]).map((value) => round(value, 3));
    const tip = transformPoint(lay, [0, 0, TOP]).map((value) => round(value, 4));
    sceneOf(doc).addChild(doc.createNode("muzzle").setTranslation(tip));
    return {
      bytes: await writeGlb(doc),
      changes: [
        "스크립트로 만든 모델: 지름 6.6 cm 몸통(어두운 띠 하나), 신관, 안전손잡이, 안전핀 고리",
        "몸통 가운데를 원점에, 축을 주먹이 쥐는 손잡이 축(앞으로 28.9°, 오른쪽으로 4.9°)에 맞춤, 신관 끝에 muzzle 노드 추가",
        "재질 색을 팔레트 텍스처 하나로 모아 primitive 하나로 합침, meshopt 압축",
      ],
      info: { muzzle: tip, length: TOP + 0.04, tipAxis },
    };
  },
};
