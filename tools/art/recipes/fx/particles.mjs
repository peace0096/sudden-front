// The particle atlas (manifest fx): Kenney Particle Pack sprites in 4 × 4 cells of 256 px, made
// white with their grey as alpha. Cells are named for their use in the game.
import { buildAtlas } from "../../lib/atlas.mjs";

const CELLS = [
  ["muzzle1", "PNG/muzzle_01.png"],
  ["muzzle2", "PNG/muzzle_02.png"],
  ["muzzle3", "PNG/muzzle_03.png"],
  ["smoke1", "PNG/smoke_01.png"],
  ["smoke2", "PNG/smoke_04.png"],
  ["smoke3", "PNG/smoke_07.png"],
  ["spark", "PNG/star_06.png"],
  ["arc", "PNG/spark_06.png"],
  ["fire", "PNG/fire_01.png"],
  ["flame", "PNG/flame_01.png"],
  ["flare", "PNG/flare_01.png"],
  ["impact", "PNG/scorch_01.png"],
  ["scorch", "PNG/scorch_03.png"],
  ["dirt", "PNG/dirt_01.png"],
  ["tracer", "PNG/trace_01.png"],
  ["ring", "PNG/light_01.png"],
];

export default {
  output: "fx/particles.webp",
  pack: "kenney-particles",
  sources: CELLS.map(([, path]) => path),
  async build(ctx) {
    const atlas = await buildAtlas({ cellSize: 256, columns: 4, rows: 4, cells: CELLS.map(([name, path]) => ({ name, path: ctx.file(path), mode: "luminance" })) });
    return {
      bytes: atlas.bytes,
      changes: ["512 px 스프라이트 16장을 256 px로 줄여 1024² 아틀라스(4×4)로 모음", "검은 바탕의 회색을 알파로 바꾸고 색은 흰색으로 (straight alpha)", "WebP로 저장"],
      info: { rects: atlas.rects, width: atlas.width, height: atlas.height },
    };
  },
};
