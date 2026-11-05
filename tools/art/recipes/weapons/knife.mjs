// KNIFE: Toon Shooter Game Kit Knife_1. It stands blade-up along +Y, its edge toward −X and its
// flat facing ±Z. Held in the fist like a gun's grip, so its handle runs up the grip's line that
// tools/art/lib/character.mjs measures (gripAxis: 28.9° forward of straight up, 4.9° to the right):
// the blade comes out of the top of the fist, its edge forward and its flat facing sideways.
import { weaponRecipe } from "../../lib/weapon.mjs";

export default weaponRecipe({
  output: "weapons/knife.glb",
  pack: "pp-toon-shooter",
  source: "knife-YnC9RcBOWs.glb",
  length: 0.3,
  grip: [0, 0, 0],
  turn: [["y", -90], ["x", -28.9], ["z", -4.9]],
  tipAxis: [0.075, 0.873, -0.482],
});
