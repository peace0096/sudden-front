// The missile in flight (projectiles.MISSILE), built here rather than taken from a pack: today's
// warhead from game.ts buildMissile (a 0.65 m body of radius 0.11 m and a 0.2 m red nose) with
// four fins and a nozzle added. It points along +Z, which is what lookAt aims.
import { Document } from "@gltf-transform/core";
import { addSolid, box, lathe, merge } from "../../lib/shapes.mjs";
import { paletteAndJoin, quiet, writeGlb } from "../../lib/scene.mjs";

const SEGMENTS = 10;
const TAIL = -0.325;

export default {
  output: "projectiles/missile.glb",
  pack: "procedural",
  sources: ["tools/art/recipes/projectiles/missile.mjs"],
  async build() {
    const doc = new Document();
    doc.setLogger(quiet);
    doc.createBuffer();
    doc.getRoot().setDefaultScene(doc.createScene("missile"));
    addSolid(doc, lathe([[0.11, TAIL], [0.11, 0.325]], SEGMENTS), { name: "body", color: "#c8ccc0" });
    addSolid(doc, lathe([[0.11, 0.325], [0, 0.525]], SEGMENTS, { back: false, front: false }), { name: "nose", color: "#c23a22" });
    addSolid(doc, lathe([[0.07, TAIL - 0.045], [0.085, TAIL]], SEGMENTS, { back: true, front: false }), { name: "nozzle", color: "#3d4038" });
    const fin = (x0, x1, y0, y1) => box([x0, y0, TAIL], [x1, y1, TAIL + 0.165]);
    const fins = merge(fin(0.08, 0.21, -0.007, 0.007), fin(-0.21, -0.08, -0.007, 0.007), fin(-0.007, 0.007, 0.08, 0.21), fin(-0.007, 0.007, -0.21, -0.08));
    addSolid(doc, fins, { name: "fins", color: "#3d4038" });
    await paletteAndJoin(doc);
    return {
      bytes: await writeGlb(doc),
      changes: [
        "스크립트로 만든 모델: 몸통(반지름 0.11 m, 길이 0.65 m)·빨간 탄두(0.2 m)는 지금의 game.ts 미사일과 같은 크기, 날개 4장과 노즐을 더함",
        "+Z를 향함, 재질 색을 팔레트 텍스처 하나로 모아 primitive 하나로 합침, meshopt 압축",
      ],
      info: {},
    };
  },
};
