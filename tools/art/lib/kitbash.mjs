// Puts more pieces into a model before it is normalised, in the model's own coordinates: other
// pack models (the M249 is the rifle with the pack's bipod) and boxes built here (its magazine).
import { mergeDocuments } from "@gltf-transform/functions";
import { addSolid, box } from "./shapes.mjs";
import { readSource, sceneOf } from "./scene.mjs";

/** Merges the model at `path` into doc, under a node named `name` moved by `translate`. */
export async function mergePart(doc, { path, name, translate = [0, 0, 0] }) {
  const partDoc = await readSource(path);
  const partScene = sceneOf(partDoc);
  const copied = mergeDocuments(doc, partDoc);
  const copiedScene = copied.get(partScene);
  const holder = doc.createNode(name).setTranslation(translate);
  for (const child of copiedScene.listChildren()) {
    copiedScene.removeChild(child);
    holder.addChild(child);
  }
  sceneOf(doc).addChild(holder);
  copiedScene.dispose();
}

/** Adds a flat-coloured box from min to max. */
export function addBox(doc, { name, min, max, color }) {
  addSolid(doc, box(min, max), { name, color });
}
