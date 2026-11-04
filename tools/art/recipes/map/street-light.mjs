// Lamps (PropKind LAMP): Toon Shooter Game Kit StreetLight at today's lamp size, a 5.6 m pole with
// the arm reaching 1.95 m along +Z, the pole at the origin. Its light glows like today's lamp head.
import { kitRecipe } from "../../lib/kit.mjs";

export default kitRecipe({
  output: "map/street-light.glb",
  pack: "pp-toon-shooter",
  source: "street-light-nFwrlcLvM5.glb",
  anchor: "origin",
  // The pole keeps its round section: x scales with the arm.
  scale: (size, { max }) => [1.95 / max[2], 5.6 / size[1], 1.95 / max[2]],
  emissive: { Light: "#77735f" },
});
