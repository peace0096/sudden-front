// Containers (BoxKind CONTAINER_*, PropKind TILTED_CONTAINER): Toon Shooter Game Kit Container_Long
// stretched to the server's 6.0 × 2.6 × 2.4 m box. The body is white so each instance takes one of
// today's four container colours; the frame keeps its grey.
import { kitRecipe } from "../../lib/kit.mjs";

export default kitRecipe({
  output: "map/container.glb",
  pack: "pp-toon-shooter",
  source: "shipping-container-dQXRtm5GbO.glb",
  size: [6.0, 2.6, 2.4],
  split: { body: ["Red"], frame: ["Grey"] },
});
