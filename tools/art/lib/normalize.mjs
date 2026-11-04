// Puts models into the game's units and axes: metres, +Y up, a known origin. Static models have
// every node transform baked into their vertices; rigged ones keep their skeleton but lose the
// armature's rotation and scale, so bones, clips and vertices are all in metres.
import { simplify, transformMesh, weld, cloneDocument } from "@gltf-transform/functions";
import { MeshoptSimplifier } from "meshoptimizer";
import { fromQuaternion, invert, multiply, quatMultiply, transformPoint, transformVector } from "./math.mjs";
import { quiet, sceneOf, triangles } from "./scene.mjs";

/**
 * Bakes matrix × each mesh node's world transform into its vertices, then leaves only the mesh
 * nodes, as untransformed children of the scene. Other nodes are disposed.
 */
export function bakeStatic(doc, matrix) {
  const scene = sceneOf(doc);
  const meshNodes = [];
  scene.traverse((node) => node.getMesh() && meshNodes.push(node));
  const baked = new Set();
  for (const node of meshNodes) {
    if (baked.has(node.getMesh())) node.setMesh(node.getMesh().clone());
    baked.add(node.getMesh());
    transformMesh(node.getMesh(), multiply(matrix, node.getWorldMatrix()));
  }
  for (const node of doc.getRoot().listNodes()) {
    for (const child of node.listChildren()) node.removeChild(child);
  }
  for (const child of scene.listChildren()) scene.removeChild(child);
  for (const node of meshNodes) {
    node.setTranslation([0, 0, 0]).setRotation([0, 0, 0, 1]).setScale([1, 1, 1]);
    scene.addChild(node);
  }
  for (const node of doc.getRoot().listNodes()) if (!meshNodes.includes(node)) node.dispose();
}

/** A rest-pose vertex, placed by its joints as the glTF spec skins it. */
function skinnedPositions(node) {
  const skin = node.getSkin();
  const joints = skin.listJoints();
  const inverse = skin.getInverseBindMatrices();
  const jointMatrices = joints.map((joint, j) => multiply(joint.getWorldMatrix(), inverse.getElement(j, [])));
  const out = new Map();
  for (const primitive of node.getMesh().listPrimitives()) {
    const position = primitive.getAttribute("POSITION");
    const jointsAttr = primitive.getAttribute("JOINTS_0");
    const weightsAttr = primitive.getAttribute("WEIGHTS_0");
    if (primitive.getAttribute("JOINTS_1")) throw new Error("JOINTS_1 is not supported");
    const result = new Float32Array(position.getCount() * 3);
    const p = [0, 0, 0];
    const j4 = [0, 0, 0, 0];
    const w4 = [0, 0, 0, 0];
    for (let i = 0; i < position.getCount(); i++) {
      position.getElement(i, p);
      jointsAttr.getElement(i, j4);
      weightsAttr.getElement(i, w4);
      let x = 0;
      let y = 0;
      let z = 0;
      let total = 0;
      for (let k = 0; k < 4; k++) {
        if (!w4[k]) continue;
        const [tx, ty, tz] = transformPoint(jointMatrices[j4[k]], p);
        x += tx * w4[k];
        y += ty * w4[k];
        z += tz * w4[k];
        total += w4[k];
      }
      if (!total) throw new Error(`vertex ${i} of ${node.getName()} has no skin weight`);
      result.set([x / total, y / total, z / total], i * 3);
    }
    out.set(primitive, result);
  }
  return out;
}

/** An accessor this primitive can change without touching another user of it. */
function own(doc, primitive, semantic) {
  const accessor = primitive.getAttribute(semantic);
  if (accessor.listParents().filter((parent) => parent !== doc.getRoot()).length > 1) {
    const copy = accessor.clone();
    primitive.setAttribute(semantic, copy);
    return copy;
  }
  return accessor;
}

/**
 * Removes the armature's rotation and uniform scale from a rigged model and moves it into metres:
 * the model ends up scaled by `scale` and shifted by `offset` from where it stood, with every bone
 * at scale 1 and every translation (rest pose and clips) in metres. The armature's rotation moves
 * into its child bones; vertices move into the new rest pose, and the inverse bind matrices are
 * rebuilt from it. The armature's parent must be untransformed.
 */
export function bakeSkinned(doc, { armature, scale, offset }) {
  const root = doc.getRoot();
  const parent = armature.getParentNode();
  if (parent && parent.getWorldMatrix().some((value, i) => Math.abs(value - [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1][i]) > 1e-6)) {
    throw new Error("the armature's parent has a transform");
  }
  const [sx, sy, sz] = armature.getScale();
  if (Math.abs(sx - sy) > 1e-6 || Math.abs(sx - sz) > 1e-6) throw new Error("the armature's scale is not uniform");
  const k = sx * scale;
  const qa = armature.getRotation();
  const ra = fromQuaternion(qa);
  const ta = armature.getTranslation();
  const top = new Set(armature.listChildren());

  // Rest-pose vertices of every skinned mesh, in the target space.
  const target = multiply([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, ...offset, 1], [scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, 1]);
  const skinned = root.listNodes().filter((node) => node.getSkin() && node.getMesh());
  const positions = skinned.map((node) => skinnedPositions(node));

  // Bones: the armature's rotation and offset go into its children; every translation scales by k.
  const moveTop = (t) => {
    const r = transformVector(ra, t);
    return [offset[0] + scale * ta[0] + k * r[0], offset[1] + scale * ta[1] + k * r[1], offset[2] + scale * ta[2] + k * r[2]];
  };
  const below = new Set();
  const walk = (node) => {
    for (const child of node.listChildren()) {
      below.add(child);
      walk(child);
    }
  };
  walk(armature);
  for (const node of below) {
    if (node.getSkin()) continue;
    if (top.has(node)) {
      node.setTranslation(moveTop(node.getTranslation()));
      node.setRotation(quatMultiply(qa, node.getRotation()));
    } else {
      node.setTranslation(node.getTranslation().map((value) => value * k));
    }
  }
  armature.setTranslation([0, 0, 0]).setRotation([0, 0, 0, 1]).setScale([1, 1, 1]);

  // Clips follow the same rule.
  const outputs = new Set();
  for (const animation of root.listAnimations()) {
    for (const channel of animation.listChannels()) {
      const node = channel.getTargetNode();
      const path = channel.getTargetPath();
      if (node === armature) throw new Error("a clip animates the armature itself");
      if (!below.has(node) || path === "scale" || path === "weights") continue;
      if (path === "rotation" && !top.has(node)) continue;
      const sampler = channel.getSampler();
      let output = sampler.getOutput();
      if (outputs.has(output) || output.listParents().filter((p) => p !== root).length > 1) {
        output = output.clone();
        sampler.setOutput(output);
      }
      outputs.add(output);
      const value = [];
      for (let i = 0; i < output.getCount(); i++) {
        output.getElement(i, value);
        if (path === "translation") output.setElement(i, top.has(node) ? moveTop(value) : value.map((v) => v * k));
        else output.setElement(i, quatMultiply(qa, value));
      }
    }
  }

  // Meshes: the new rest pose is the bind pose; their nodes stop transforming them.
  skinned.forEach((node, n) => {
    for (const [primitive, world] of positions[n]) {
      const accessor = own(doc, primitive, "POSITION");
      for (let i = 0; i < accessor.getCount(); i++) accessor.setElement(i, transformPoint(target, [world[i * 3], world[i * 3 + 1], world[i * 3 + 2]]));
    }
    node.setTranslation([0, 0, 0]).setRotation([0, 0, 0, 1]).setScale([1, 1, 1]);
  });
  for (const skin of new Set(skinned.map((node) => node.getSkin()))) {
    const ibm = skin.getInverseBindMatrices();
    skin.listJoints().forEach((joint, j) => ibm.setElement(j, invert(joint.getWorldMatrix())));
  }
  return { target };
}

/**
 * Decimates to at most `target` triangles, trying smaller ratios until it fits. A flat-shaded
 * model keeps a separate copy of every vertex per face, which leaves nothing to collapse, so
 * dropNormals removes the normals (and unused texture coordinates) first: glTF loaders shade a
 * model without normals flat (three.js sets flatShading), and welding can then join the faces.
 */
export async function decimate(doc, { triangles: target, dropNormals = false }) {
  await MeshoptSimplifier.ready;
  if (dropNormals) {
    for (const mesh of doc.getRoot().listMeshes()) {
      for (const primitive of mesh.listPrimitives()) {
        primitive.setAttribute("NORMAL", null);
        if (!primitive.getMaterial()?.getBaseColorTexture()) primitive.setAttribute("TEXCOORD_0", null);
      }
    }
  }
  await doc.transform(weld());
  const start = triangles(doc);
  if (start <= target) return doc;
  let ratio = target / start;
  for (let attempt = 0; attempt < 8; attempt++) {
    const trial = cloneDocument(doc);
    trial.setLogger(quiet);
    await trial.transform(simplify({ simplifier: MeshoptSimplifier, ratio, error: 0.02, lockBorder: false }));
    const count = triangles(trial);
    if (count <= target) return trial;
    ratio *= (target / count) * 0.97;
  }
  throw new Error(`could not decimate below ${target} triangles`);
}

/** A copy of the document posed at the clip's first keyframe. */
export function poseAtStart(doc, clipName) {
  const copy = cloneDocument(doc);
  copy.setLogger(quiet);
  const animation = copy.getRoot().listAnimations().find((item) => item.getName() === clipName);
  if (!animation) throw new Error(`no clip ${clipName}`);
  for (const channel of animation.listChannels()) {
    const value = channel.getSampler().getOutput().getElement(0, []);
    const node = channel.getTargetNode();
    if (channel.getTargetPath() === "translation") node.setTranslation(value);
    else if (channel.getTargetPath() === "rotation") node.setRotation(value);
    else if (channel.getTargetPath() === "scale") node.setScale(value);
  }
  return copy;
}
