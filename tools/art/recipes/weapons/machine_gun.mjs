// MACHINE_GUN (M249): the rifle with the pack's bipod under its barrel and a box magazine built
// here, at the M249's length.
import { weaponRecipe } from "../../lib/weapon.mjs";

export default weaponRecipe({
  output: "weapons/machine_gun.glb",
  pack: "pp-ultimate-guns",
  source: "assault-rifle-Bgvuu4CUMV.glb",
  length: 1.04,
  grip: [-0.05, 0.05, 0],
  // Under the handguard, whose underside is at y ≈ 0.586 there; the bipod's mount is at y ≈ 0.09.
  parts: [{ source: "bipod-Ugy8tjR3Ju.glb", name: "bipod", label: "Bipod", translate: [2.95, 0.5, 0] }],
  // Over the rifle's own curved magazine, a little to the gun's left like the M249's.
  boxes: [{ name: "boxMagazine", label: "박스 탄창", min: [0.62, -0.82, -0.32], max: [1.45, 0.35, 0.2], color: "#3d4436" }],
});
