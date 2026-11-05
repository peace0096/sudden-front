// HAMMER: the Big Head mode King's toy squeaky hammer, built here as no pack has one: a cylindrical
// head of two red plastic caps with a yellow bellows between them, across the top of a blue handle
// with a navy grip and end knob. Built with the handle along +Z (lathe's) and the head's long axis
// along +Y, then laid along the grip's line through the fist that tools/art/lib/character.mjs
// measures (gripAxis: 28.9° forward of straight up, 4.9° to the right), as the knife's handle is:
// the head shows above the fist, square to the handle, its front cap (the striking face) forward.
// The handle's lower part, where the fist closes, is the origin; the `muzzle` node marks the
// middle of the head.
import { Document } from "@gltf-transform/core";
import { chain, rotation, round, transformPoint, transformVector, translation } from "../../lib/math.mjs";
import { bakeStatic } from "../../lib/normalize.mjs";
import { addSolid, lathe } from "../../lib/shapes.mjs";
import { paletteAndJoin, quiet, sceneOf, writeGlb } from "../../lib/scene.mjs";

const HEAD_SEGMENTS = 14;
const HANDLE_SEGMENTS = 10;
/** Where the head's middle is along the handle, which the `muzzle` node marks. */
const HEAD = 0.22;
/** The head's half length and the bellows' half length, along its own axis. */
const HEAD_HALF = 0.11;
const BELLOWS_HALF = 0.06;
/** The end knob's lowest point along the handle. */
const BOTTOM = -0.085;
/** The caps' radius, the head's widest. */
const CAP_RADIUS = 0.052;
const RED = "#e2342c";
const YELLOW = "#ffc31f";
const BLUE = "#2f6fd6";
const NAVY = "#1e3566";

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

export default {
  output: "weapons/hammer.glb",
  pack: "procedural",
  sources: ["tools/art/recipes/weapons/hammer.mjs"],
  async build() {
    const doc = new Document();
    doc.setLogger(quiet);
    doc.createBuffer();
    doc.getRoot().setDefaultScene(doc.createScene("hammer"));

    // The head, built along +Z from its back cap to its front one, then turned onto +Y at the
    // handle's top.
    const head = chain(translation([0, 0, HEAD]), rotation("x", -90));
    // Each cap bevelled round its outer face and closed at both ends: the bellows is narrower.
    const back = [[0.032, -HEAD_HALF], [0.044, -0.1085], [0.05, -0.104], [CAP_RADIUS, -0.097], [CAP_RADIUS, -0.064], [0.049, -BELLOWS_HALF]];
    const front = back.map(([r, z]) => [r, -z]).reverse();
    addSolid(doc, moved(lathe(back, HEAD_SEGMENTS), head), { name: "capBack", color: RED });
    addSolid(doc, moved(lathe(front, HEAD_SEGMENTS), head), { name: "capFront", color: RED });
    // The bellows: six folds, ridge and valley every centimetre.
    const folds = Array.from({ length: 13 }, (_, i) => [i % 2 ? 0.049 : 0.041, -BELLOWS_HALF + i * 0.01]);
    addSolid(doc, moved(lathe(folds, HEAD_SEGMENTS, { back: false, front: false }), head), { name: "bellows", color: YELLOW });

    // The handle: a red collar where it goes into the head, the blue shaft, the navy grip round
    // the origin and the navy knob below the fist.
    addSolid(doc, lathe([[0.021, HEAD - 0.064], [0.021, HEAD - 0.036]], HANDLE_SEGMENTS), { name: "collar", color: RED });
    addSolid(doc, lathe([[0.0155, 0.035], [0.0155, HEAD - 0.03]], HANDLE_SEGMENTS, { back: false, front: false }), { name: "shaft", color: BLUE });
    addSolid(doc, lathe([[0.019, -0.066], [0.019, 0.034], [0.0165, 0.042]], HANDLE_SEGMENTS, { back: false, front: true }), { name: "grip", color: NAVY });
    addSolid(doc, lathe([[0.011, BOTTOM], [0.019, -0.0835], [0.0225, -0.078], [0.0225, -0.069], [0.0185, -0.0635]], HANDLE_SEGMENTS), { name: "knob", color: NAVY });

    // +Z (the handle) onto +Y and +Y (the head's front) onto −Z, then forward and right onto the
    // fist's grip line.
    const lay = chain(rotation("z", -4.9), rotation("x", -28.9), rotation("x", -90));
    bakeStatic(doc, lay);
    await paletteAndJoin(doc);
    const tipAxis = transformVector(lay, [0, 0, 1]).map((value) => round(value, 3));
    const middle = transformPoint(lay, [0, 0, HEAD]).map((value) => round(value, 4));
    sceneOf(doc).addChild(doc.createNode("muzzle").setTranslation(middle));
    return {
      bytes: await writeGlb(doc),
      changes: [
        "스크립트로 만든 모델: 길이 22 cm·지름 10 cm 머리(빨간 양끝 마개, 가운데 노란 주름 여섯 겹), 파란 손잡이(빨간 이음 고리, 남색 손잡이 감개와 끝 손잡이), 끝에서 머리 가운데까지 30 cm",
        "손잡이의 쥐는 곳을 원점에, 손잡이를 주먹이 쥐는 손잡이 축(앞으로 28.9°, 오른쪽으로 4.9°)에 맞추고 머리는 손잡이와 직각으로 앞뒤로 놓음(앞 마개가 때리는 면), 머리 가운데에 muzzle 노드 추가",
        "재질 색을 팔레트 텍스처 하나로 모아 primitive 하나로 합침, meshopt 압축",
      ],
      info: { muzzle: middle, length: HEAD + CAP_RADIUS - BOTTOM, tipAxis },
    };
  },
};
