// Builds a game character from a Quaternius Ultimate Modular Men model: one skinned mesh in
// metres with its feet at y = 0 and its head bone at the server's neck height, canonical bone and
// clip names, the team colour painted into its uniform, ≤ 5,000 triangles in one draw call.
import { joinPrimitives, palette, resample } from "@gltf-transform/functions";
import { HEAD_TOP } from "../inspect.mjs";
import { eulerXYZ, invert, multiply, rotation, round, transformPoint } from "./math.mjs";
import { bakeSkinned, decimate, poseAtStart } from "./normalize.mjs";
import { hexToLinear, readSource, triangles, writeGlb } from "./scene.mjs";

/** The server's head box starts here (client/src/game/art/hitbox.ts HEAD_BASE); the head bone goes on it. */
export const HEAD_BASE = 1.5;
/** The modular men's bone names → the game's. Bones not listed keep a cleaned-up version of their name. */
const BONES = {
  Root: "root",
  Body: "body",
  Hips: "hips",
  Abdomen: "spine",
  Torso: "spine1",
  Chest: "chest",
  Neck: "neck",
  Head: "head",
  "Shoulder.L": "clavicleL",
  "UpperArm.L": "upperArmL",
  "LowerArm.L": "foreArmL",
  "Wrist.L": "handL",
  "Shoulder.R": "clavicleR",
  "UpperArm.R": "upperArmR",
  "LowerArm.R": "foreArmR",
  "Wrist.R": "handR",
  "UpperLeg.L": "thighL",
  "LowerLeg.L": "shinL",
  "Foot.L": "footL",
  "UpperLeg.R": "thighR",
  "LowerLeg.R": "shinR",
  "Foot.R": "footR",
  "PT.L": "kneePoleL",
  "PT.R": "kneePoleR",
};

/** "Index2.L" → "indexL2", "Head_end" → "headEnd". */
function boneName(name) {
  const end = name.endsWith("_end");
  const base = end ? name.slice(0, -4) : name;
  let canonical = BONES[base];
  if (!canonical) {
    const finger = /^([A-Za-z]+)(\d)\.([LR])$/.exec(base);
    canonical = finger ? `${finger[1].toLowerCase()}${finger[3]}${finger[2]}` : base.replace(/\W/g, "").replace(/^./, (c) => c.toLowerCase());
  }
  return end ? `${canonical}End` : canonical;
}

/**
 * Builds one team's character. `clips` maps the game's clip names to the source's, `uniform` is
 * the material that takes the team colour, mixed in linear light as the checkpoint A gallery did.
 */
export async function buildCharacter({ path, clips, uniform, team, mix, maxTriangles }) {
  const doc = await readSource(path);
  const root = doc.getRoot();
  const changes = [];

  // Clips: the ones the game plays, under its names; the rest go.
  const animations = root.listAnimations();
  const kept = new Set();
  for (const [name, source] of Object.entries(clips)) {
    const animation = animations.find((item) => item.getName() === source);
    if (!animation) throw new Error(`no clip ${source}`);
    animation.setName(name);
    kept.add(animation);
  }
  for (const animation of animations) if (!kept.has(animation)) animation.dispose();
  changes.push(`클립 ${animations.length}개 중 ${kept.size}개만 남기고 이름 변경 (${Object.entries(clips).map(([name, source]) => `${source.replace(/^.*\|/, "")}→${name}`).join(", ")})`);

  // One skinned mesh: the four modular parts share their joints and bind matrices.
  const parts = root.listNodes().filter((node) => node.getSkin() && node.getMesh());
  const skin = parts[0].getSkin();
  const ibm = Array.from(skin.getInverseBindMatrices().getArray());
  for (const part of parts) {
    const other = part.getSkin();
    const sameJoints = other.listJoints().every((joint, i) => joint === skin.listJoints()[i]);
    const sameBind = Array.from(other.getInverseBindMatrices().getArray()).every((value, i) => Math.abs(value - ibm[i]) < 1e-6);
    if (!sameJoints || !sameBind) throw new Error(`${part.getName()} has its own skeleton`);
  }
  const body = parts[0];
  for (const part of parts.slice(1)) {
    const mesh = part.getMesh();
    for (const primitive of mesh.listPrimitives()) {
      // Moved, not shared: a primitive in two meshes would be simplified twice.
      mesh.removePrimitive(primitive);
      body.getMesh().addPrimitive(primitive);
    }
    mesh.dispose();
    part.dispose();
  }
  body.setName("mesh").getMesh().setName("mesh");
  body.setSkin(skin);
  for (const other of root.listSkins()) if (other !== skin) other.dispose();
  changes.push(`부위 메시 ${parts.length}개를 스킨 메시 하나로 합침`);

  // Team colour in the uniform.
  const material = root.listMaterials().find((item) => item.getName() === uniform);
  if (!material) throw new Error(`no material ${uniform}`);
  const base = material.getBaseColorFactor();
  const tint = hexToLinear(team);
  material.setBaseColorFactor([0, 1, 2].map((i) => base[i] + (tint[i] - base[i]) * mix).concat(base[3]));
  changes.push(`${uniform} 재질에 팀 색 ${team}을 ${Math.round(mix * 100)}% 섞음`);

  // Metres: the head bone on the server's neck height, feet on the floor.
  const head = root.listNodes().find((node) => node.getName() === "Head");
  const armature = root.listNodes().find((node) => node.listChildren().some((child) => child.getName() === "Root"));
  const before = measureRest(doc, head);
  const scale = HEAD_BASE / (before.headY - before.feetY);
  bakeSkinned(doc, { armature, scale, offset: [0, -scale * before.feetY, 0] });
  changes.push(`미터 단위로 정규화: 머리 뼈 ${HEAD_BASE} m, 발 y = 0, 아마추어의 회전·100배 배율을 뼈에 합침 (×${round(scale, 4)})`);

  // Canonical names, after the bake looked bones up by their own.
  for (const node of root.listNodes()) {
    if (node === body) continue;
    if (node === armature) node.setName("armature");
    else if (node.getName() === "RootNode") node.setName("character");
    else node.setName(boneName(node.getName()));
  }
  changes.push("뼈 이름을 게임 표준 이름으로 바꿈 (hips·spine·chest·neck·head·clavicle/upperArm/foreArm/hand·thigh/shin/foot + L/R)");

  // ≤ maxTriangles, flat-shaded like the source.
  const sourceTriangles = triangles(doc);
  const reduced = await decimate(doc, { triangles: maxTriangles, dropNormals: true });
  changes.push(`삼각형 ${sourceTriangles}개를 ${triangles(reduced)}개로 감량 (면별 법선을 지우고 용접, 평면 셰이딩 유지)`);

  // One material and one primitive.
  for (const item of reduced.getRoot().listMaterials()) item.setMetallicFactor(0).setRoughnessFactor(1);
  await reduced.transform(palette({ min: 2 }));
  const mesh = reduced.getRoot().listMeshes()[0];
  const primitives = mesh.listPrimitives();
  if (primitives.length > 1) {
    const joined = joinPrimitives(primitives);
    for (const primitive of primitives) {
      mesh.removePrimitive(primitive);
      primitive.dispose();
    }
    mesh.addPrimitive(joined);
  }
  await reduced.transform(resample());
  changes.push("재질 색을 팔레트 텍스처 하나로 모으고 primitive 하나로 합침, 키프레임 정리(resample), meshopt 압축");

  const info = describeRig(reduced);
  return { bytes: await writeGlb(reduced), changes, info };
}

/** The head bone's and the lowest vertex's heights in the rest pose, in the source's units. */
function measureRest(doc, head) {
  const skin = doc.getRoot().listSkins()[0];
  const joints = skin.listJoints();
  const inverse = skin.getInverseBindMatrices();
  const matrices = joints.map((joint, j) => multiply(joint.getWorldMatrix(), inverse.getElement(j, [])));
  let feetY = Infinity;
  for (const node of doc.getRoot().listNodes()) {
    if (!node.getSkin() || !node.getMesh()) continue;
    for (const primitive of node.getMesh().listPrimitives()) {
      const position = primitive.getAttribute("POSITION");
      const jointsAttr = primitive.getAttribute("JOINTS_0");
      const weights = primitive.getAttribute("WEIGHTS_0");
      const p = [];
      const j4 = [];
      const w4 = [];
      for (let i = 0; i < position.getCount(); i++) {
        position.getElement(i, p);
        jointsAttr.getElement(i, j4);
        weights.getElement(i, w4);
        let y = 0;
        let total = 0;
        for (let k = 0; k < 4; k++) {
          if (!w4[k]) continue;
          y += transformPoint(matrices[j4[k]], p)[1] * w4[k];
          total += w4[k];
        }
        feetY = Math.min(feetY, y / total);
      }
    }
  }
  return { headY: head.getWorldTranslation()[1], feetY };
}

/**
 * What the manifest and Phase 3 need from the finished rig: which way it faces, its head height,
 * and where a gun sits in the right hand while holding the rifle pose, as a placement relative
 * to the handR bone. The gun's grip is at its origin and its barrel along −Z.
 */
function describeRig(doc) {
  const find = (d, name) => d.getRoot().listNodes().find((node) => node.getName() === name);
  const footL = find(doc, "footL").getWorldTranslation();
  const hips = find(doc, "hips").getWorldTranslation();
  const head = find(doc, "head").getWorldTranslation();
  const posed = poseAtStart(doc, "holdRifle");
  const hand = find(posed, "handR").getWorldMatrix();
  // The palm: the middle finger's first bone spans it (handR → middleR1 is only the wrist joint).
  const palmStart = find(posed, "middleR1").getWorldTranslation();
  const palmEnd = find(posed, "middleR2").getWorldTranslation();
  const palm = palmStart.map((value, i) => (value + palmEnd[i]) / 2);
  // The model faces +Z; a gun pointing that way has its −Z turned half a circle about Y.
  const gun = rotation("y", 180);
  const handRotation = hand.slice();
  handRotation[12] = handRotation[13] = handRotation[14] = 0;
  const local = multiply(invert(handRotation), gun);
  return {
    forwardYaw: round(Math.PI, 6),
    headJoint: round(head[1], 4),
    headTop: HEAD_TOP,
    hips: hips.map((value) => round(value, 4)),
    feet: round(footL[1], 4),
    gripInHand: {
      position: transformPoint(invert(hand), palm).map((value) => round(value, 4)),
      rotation: eulerXYZ(local).map((value) => round(value, 4)),
    },
    palmInRifleHold: palm.map((value) => round(value, 4)),
    bones: doc.getRoot().listSkins()[0].listJoints().map((joint) => joint.getName()),
    clips: doc.getRoot().listAnimations().map((animation) => animation.getName()),
    triangles: triangles(doc),
  };
}

/** The game's clips in the Ultimate Modular Men rig. */
const MODULAR_MEN_CLIPS = {
  idle: "CharacterArmature|Idle",
  run: "CharacterArmature|Run",
  runBack: "CharacterArmature|Run_Back",
  strafeL: "CharacterArmature|Run_Left",
  strafeR: "CharacterArmature|Run_Right",
  walk: "CharacterArmature|Walk",
  death: "CharacterArmature|Death",
  // The right arm raised to aim; the left hand is brought to the gun by bone offsets (Phases 3–4).
  holdRifle: "CharacterArmature|Idle_Gun_Pointing",
  shoot: "CharacterArmature|Gun_Shoot",
};

/** A recipe for the SWAT soldier in one team's colour (checkpoint A: ≤ 5,000 triangles, colour on `Swat`). */
export function swatRecipe({ output, team }) {
  const source = "swat-Btfn3G5Xv4.glb";
  return {
    output,
    pack: "pp-ultimate-modular-men",
    sources: [source],
    build: (ctx) => buildCharacter({ path: ctx.file(source), clips: MODULAR_MEN_CLIPS, uniform: "Swat", team, mix: 0.7, maxTriangles: 4800 }),
  };
}
