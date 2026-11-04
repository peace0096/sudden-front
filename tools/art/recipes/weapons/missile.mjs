// MISSILE (the launcher): Toon Shooter Game Kit RocketLauncher. Its rockets leave from its −X end,
// so it turns the other way from the guns.
import { weaponRecipe } from "../../lib/weapon.mjs";

export default weaponRecipe({
  output: "weapons/missile.glb",
  pack: "pp-toon-shooter",
  source: "rocket-launcher-GCqUvqleqN.glb",
  length: 1.0,
  grip: [-0.05, 0, 0],
  turn: [["y", -90]],
});
