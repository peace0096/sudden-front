// KNIFE: Toon Shooter Game Kit Knife_1. It stands blade-up along +Y; turned so the blade points
// along −Z with its flat facing sideways, edge down.
import { weaponRecipe } from "../../lib/weapon.mjs";

export default weaponRecipe({
  output: "weapons/knife.glb",
  pack: "pp-toon-shooter",
  source: "knife-YnC9RcBOWs.glb",
  length: 0.3,
  grip: [0, 0, 0],
  turn: [["x", -90], ["z", 90]],
});
