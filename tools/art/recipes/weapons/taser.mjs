// TASER: Ultimate Guns Pack Pistol_5 painted yellow; the arcs stay in code.
import { weaponRecipe } from "../../lib/weapon.mjs";

export default weaponRecipe({
  output: "weapons/taser.glb",
  pack: "pp-ultimate-guns",
  source: "pistol-J3i9KDQ3kt.glb",
  length: 0.2,
  grip: [-0.08, -0.15, 0],
  recolor: { Metal: "#d8b52c", LightMetal: "#e3c650" },
});
