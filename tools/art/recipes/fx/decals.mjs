// The decal atlas (manifest fx.decals): Kenney Splat Pack shapes in 2 × 2 cells of 256 px, tinted
// by the game: three blood splats and one compact splat for bullet holes.
import { buildAtlas } from "../../lib/atlas.mjs";

const CELLS = [
  ["bloodA", "PNG/Double (512px)/splat19.png"],
  ["bloodB", "PNG/Double (512px)/splat02.png"],
  ["bloodC", "PNG/Double (512px)/splat33.png"],
  ["bulletHole", "PNG/Double (512px)/splat11.png"],
];

export default {
  output: "fx/decals.webp",
  pack: "kenney-splat",
  sources: CELLS.map(([, path]) => path),
  async build(ctx) {
    const atlas = await buildAtlas({ cellSize: 256, columns: 2, rows: 2, cells: CELLS.map(([name, path]) => ({ name, path: ctx.file(path), mode: "alpha" })) });
    return {
      bytes: atlas.bytes,
      changes: ["512 px 얼룩 4장을 256 px로 줄여 512² 아틀라스(2×2)로 모음", "색은 흰색, 모양은 알파로 (straight alpha)", "WebP로 저장"],
      info: { rects: atlas.rects, width: atlas.width, height: atlas.height },
    };
  },
};
