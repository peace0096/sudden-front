// The ?art preview loads every file in the art manifest with no failures, no console errors and no
// [art] warnings, without ever connecting to the server, and the art fits the game: every WeaponKind
// has a model with a muzzle node, both team soldiers have the canonical bones and clips with the
// head bone at the hitbox's neck in the rest pose, every file is within its budget, the map kits
// fill their boxes and the effect atlases hold their cells. Its bench poses the game's real
// Character and holds it to the server's hitboxes: the neck (head bone) on the head box's base
// standing and crouched and over the hitbox column at the steepest aim, the head top at each head
// size, the soles on the floor, every gun pointing along the aim from in front of the character
// (the knife's blade forward and up instead, from a wrist within 20° of straight, and the grenade
// in the fist with its fuze up) with the left
// fist closed on it where the weapon says and its palm turned to hold it, and at most 4 draw calls
// a character. Its first-person bench drives the game's real ViewModel through every weapon's
// idle, kick, reload, bolt or pump cycle, knife swing and grenade throw: the right fist on the grip and, on a
// two-handed gun, the left fist on its support point (palm up into it) or second grip, each within
// 1.5 cm; the knife hand's wrist within 20° of straight; every muzzle on screen in idle, and the
// hands that hold the weapon there (the grip hand of a one-handed weapon, both akimbo hands, a long
// gun's support hand); and the draw calls at most 5 (arms, guns, flash while it shows, shield while
// it is up). The other weapons' wrist bends are printed as INFO. Its map bench
// builds a small map holding every BoxKind and PropKind with the game's buildWorld: every container,
// crate and barrel copy fills its server box within 1 cm (containers turned along Z included) in its
// kit's tint, each leaning container leans like the box it replaces within 1 cm, every lamp has its
// model, every other kind is drawn, every textured prop plane shows its texture (drawn alone under
// flat light and read back, its pixels follow the texture's pattern), and the map's fixed objects stay
// within the draw call budget. Its effects bench runs the game's real Effects at a fixed time step:
// every recipe at once (muzzle flash, tracer, impact, blood, explosion, missile trail) stays within
// the effects draw call budget; a flood far past every pool's size leaves each pool at exactly its
// cap; and a shot at each side of a box along every axis, the ground and a barrel leaves its mark
// lying on the face it struck and turned to it, and none on the barrel. Shader compile errors show
// as console errors.
// Usage: node e2e/art.mjs [url] [screenshot-dir]  (the game's URL; its ?debug becomes ?art)
import { chromium } from "playwright-core";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { defaultUrl } from "./server.mjs";

const url = new URL(process.argv[2] ?? defaultUrl);
url.search = "?art";
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

// Every WeaponKind, BoxKind and PropKind, read from the proto like scripts/check-art.mjs (protobuf-es
// drops the prefix).
const proto = readFileSync(fileURLToPath(new URL("../../proto/game.proto", import.meta.url)), "utf8").replace(/\/\/.*$/gm, "");
const enumNames = (name, prefix) =>
  [...(new RegExp(`\\benum\\s+${name}\\s*\\{([^}]*)\\}`).exec(proto)?.[1] ?? "").matchAll(new RegExp(`\\b${prefix}_(\\w+)\\s*=`, "g"))]
    .map((match) => match[1])
    .filter((value) => value !== "UNSPECIFIED");
const KINDS = enumNames("WeaponKind", "WEAPON_KIND");
const BOX_KINDS = enumNames("BoxKind", "BOX_KIND");
const PROP_KINDS = enumNames("PropKind", "PROP_KIND");
const BONES = ["head", "neck", "hips", "handL", "handR"];
const CLIPS = ["idle", "run", "death"];
const NECK = 1.5;
const CROUCHED_NECK = 1.0;
const MAX_PITCH = 1.45;
const HAND_TOLERANCE = 0.015;
// Degrees the knife hand's wrist may bend from straight in either view (client/src/game/art/preview.ts
// KNIFE_WRIST_LIMIT): its tip points where the arm puts it, not where a bent wrist does.
const KNIFE_WRIST_LIMIT = 20;
const FP_MAX_CALLS = 5;
const BUDGETS = JSON.parse(readFileSync(fileURLToPath(new URL("../src/game/art/manifest.json", import.meta.url)), "utf8")).budgets;
const ARMS_TRIANGLES = BUDGETS.triangles.arms;
const FIT_TOLERANCE = 0.01;
// A textured plane's drawn pixels against its texture: at least this correlation, and at least this
// share of the texture's luma spread. A plane that lost its texture or UVs draws one flat colour.
const PLANE_PROPS = ["SHUTTER", "BILLBOARD", "WINDOW", "LOUVER"];
const PLANE_CORRELATION = 0.8;
const PLANE_CONTRAST = 0.3;

const checks = [];
function check(name, ok, detail = "") {
  checks.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  -> " + detail : ""}`);
}

const browser = await chromium.launch({
  channel: "msedge",
  headless: true,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
const warnings = [];
const sockets = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
  if (m.type() === "warning") warnings.push(m.text());
});
page.on("websocket", (socket) => sockets.push(socket.url()));

await page.goto(url.href);
await page.waitForSelector(".art-preview[data-state='ready']", { timeout: 120000 });
const shown = await page.evaluate(() => ({
  text: document.querySelector(".art-preview").textContent,
  failures: [...document.querySelectorAll(".art-failure")].map((item) => item.textContent),
  report: window.__artReport,
}));
// The page scrolls inside .lobby; let the window scroll so the screenshot holds all of it.
await page.addStyleTag({ content: "html, body { height: auto !important; overflow: visible !important; } .lobby { position: static !important; overflow: visible !important; }" });
await page.waitForTimeout(500);
await page.screenshot({ path: join(shots, "art-preview.png"), fullPage: true });
await browser.close();

const report = shown.report;
const artWarnings = warnings.filter((text) => text.startsWith("[art]"));
// Headless Chrome and Edge drawing WebGL in software log "GL Driver Message (..., Performance, ...):
// GPU stall due to ReadPixels" when the compositor reads a frame back. Any WebGL page gets it, a
// bare canvas that only clears included, so it is the browser's note rather than the page's warning.
const driverNotes = warnings.filter((text) => /GL Driver Message \([^)]*Performance/.test(text));
check("the preview finished loading", true, url.href);
check("every art file loaded", shown.failures.length === 0 && report.failures.length === 0, [...shown.failures, ...report.failures.map((f) => `${f.file}: ${f.reason}`)].slice(0, 3).join(" | "));
check("the manifest summary and budgets are shown", shown.text.includes("무기") && shown.text.includes("예산"));

const weapons = KINDS.map((kind) => [kind, report.weapons[kind]]);
check(`all ${KINDS.length} weapons resolve to a loaded model`, weapons.every(([, weapon]) => weapon?.loaded), weapons.filter(([, weapon]) => !weapon?.loaded).map(([kind]) => kind).join(", "));
check("every weapon has a muzzle node", weapons.every(([, weapon]) => weapon?.muzzle), weapons.filter(([, weapon]) => !weapon?.muzzle).map(([kind]) => kind).join(", "));

const teams = Object.entries(report.characters);
check("all three soldiers loaded (red, blue and the free-for-all's neutral)", teams.length === 3 && teams.every(([, soldier]) => soldier.loaded), teams.map(([team, soldier]) => `${team} ${soldier.file}`).join(", "));
for (const [team, soldier] of teams) {
  const missingBones = BONES.filter((bone) => !soldier.bones.includes(bone));
  check(`${team}: canonical bones ${BONES.join(", ")}`, missingBones.length === 0, missingBones.length ? `missing ${missingBones.join(", ")}` : `${soldier.bones.length} bones`);
  const missingClips = CLIPS.filter((clip) => !soldier.clips.includes(clip));
  check(`${team}: clips ${CLIPS.join(", ")}`, missingClips.length === 0, missingClips.length ? `missing ${missingClips.join(", ")}` : soldier.clips.join(", "));
  check(`${team}: rest-pose head joint at ${NECK} ± 0.03 m`, soldier.headJoint !== null && Math.abs(soldier.headJoint - NECK) <= 0.03, `${soldier.headJoint?.toFixed(3)} m`);
}

const over = report.files.filter((file) => !file.ok);
check(`every file is within its budget (${report.files.length} files)`, report.files.length > 0 && over.length === 0, over.map((file) => `${file.file} ${file.bytes} B ${file.triangles} tris`).join(" | "));
check("all files together are within the download budget", report.totalBytes <= report.downloadLimitBytes, `${(report.totalBytes / 1024).toFixed(1)} KB of ${(report.downloadLimitBytes / 1048576).toFixed(0)} MB`);
const misfit = Object.entries(report.kits).filter(([, kit]) => kit.size && kit.size.some((target, axis) => Math.abs(kit.measured[axis] - target) > target * 0.01));
check("every map kit fills its box within 1%", Object.keys(report.kits).length > 0 && misfit.length === 0, misfit.map(([name, kit]) => `${name} ${kit.measured.map((v) => v.toFixed(2)).join("×")}`).join(" | "));

// The map bench (preview.ts): the game's buildWorld on a small map with every kind of box and prop.
const map = report.map ?? { boxKinds: [], propKinds: [], calls: NaN, budget: NaN, undrawn: [], kits: [], tilted: [], lamps: { props: 0, copies: 0 }, planes: [] };
const absent = [...BOX_KINDS.filter((kind) => !map.boxKinds.includes(kind)), ...PROP_KINDS.filter((kind) => !map.propKinds.includes(kind))];
check(`the map bench holds every BoxKind and PropKind (${BOX_KINDS.length} + ${PROP_KINDS.length})`, report.map !== null && absent.length === 0, absent.length ? `missing ${absent.join(", ")}` : "");
const turned = map.kits.filter((kit) => kit.alongZ);
const misfits = map.kits.filter((kit) => !(kit.error <= FIT_TOLERANCE));
const fmtCm = (metres) => (Number.isFinite(metres) ? `${(metres * 100).toFixed(3)} cm` : "none");
check(
  `every container, crate and barrel copy fills its server box within ${FIT_TOLERANCE * 100} cm (${map.kits.length}, ${turned.length} turned along Z)`,
  map.kits.length > 0 && turned.length > 0 && misfits.length === 0,
  misfits.map((kit) => `${kit.kind} box ${kit.box.join("/")} drawn ${kit.bounds?.map((v) => v.toFixed(3)).join("/") ?? "nothing"}`).slice(0, 3).join(" | ") ||
    `worst ${fmtCm(Math.max(...map.kits.map((kit) => kit.error)))}`,
);
const untinted = map.kits.filter((kit) => !kit.tinted);
check("every copy has its kit's tint", map.kits.length > 0 && untinted.length === 0, untinted.map((kit) => `${kit.kind} ${kit.colours.join("/") || "no colour"}`).join(", ") || [...new Set(map.kits.flatMap((kit) => kit.colours))].join(", "));
const leaning = map.tilted.filter((item) => !(item.error <= FIT_TOLERANCE));
check(
  `every leaning container leans like the box it replaces, within ${FIT_TOLERANCE * 100} cm (${map.tilted.length})`,
  map.tilted.length > 0 && leaning.length === 0,
  map.tilted.map((item) => `rotation ${item.rotation.toFixed(2)}: ${fmtCm(item.error)}`).join(", "),
);
check("every lamp has its model", map.lamps.props > 0 && map.lamps.copies === map.lamps.props, `${map.lamps.copies} models for ${map.lamps.props} lamps`);
check("every other kind of box and prop is drawn", report.map !== null && map.undrawn.length === 0, map.undrawn.join(", "));
const planes = PLANE_PROPS.map((kind) => [kind, map.planes.find((plane) => plane.kind === kind)]);
const fullUv = (uv) => uv.length === 4 && uv[0] <= 0.001 && uv[1] >= 0.999 && uv[2] <= 0.001 && uv[3] >= 0.999;
const flat = planes.filter(([, plane]) => !plane || !plane.map || !fullUv(plane.uv) || !(plane.correlation >= PLANE_CORRELATION) || !(plane.drawnSpread >= PLANE_CONTRAST * plane.textureSpread));
check(
  `every textured prop plane shows its texture: a map, UVs over 0–1, and drawn pixels that follow it (correlation ≥ ${PLANE_CORRELATION}, contrast ≥ ${PLANE_CONTRAST * 100}%)`,
  flat.length === 0,
  planes.map(([kind, plane]) => (plane ? `${kind} ${plane.map ? "" : "no map "}uv ${plane.uv.map((v) => v.toFixed(2)).join("~")} r ${plane.correlation.toFixed(3)} spread ${plane.drawnSpread.toFixed(3)}/${plane.textureSpread.toFixed(3)}` : `${kind} missing`)).join(" | "),
);
check(`the map's fixed objects take at most ${BUDGETS.drawCalls.map} draw calls`, map.calls <= BUDGETS.drawCalls.map && map.budget === BUDGETS.drawCalls.map, `${map.calls} with everything in view`);

check("the effect atlases loaded with every cell inside", report.atlases.length > 0 && report.atlases.every((atlas) => atlas.loaded && atlas.inside), report.atlases.map((atlas) => `${atlas.file} ${atlas.width}×${atlas.height} ${atlas.cells} cells`).join(" | "));

// The effects bench (art/effects-bench.ts): the game's Effects at a fixed time step.
const fx = report.effects ?? { calls: NaN, budget: NaN, pools: {}, marks: [], frames: {} };
const pools = Object.entries(fx.pools);
check(
  `every effect at once takes at most ${BUDGETS.drawCalls.effects} draw calls`,
  report.effects !== null && fx.calls <= BUDGETS.drawCalls.effects && fx.budget === BUDGETS.drawCalls.effects && pools.length === 4 && pools.every(([, pool]) => pool.active > 0),
  `${fx.calls} calls with ${pools.map(([name, pool]) => `${name} ${pool.active}`).join(", ")} live`,
);
check(
  "a flood past every effect pool's size leaves each at its cap",
  pools.length === 4 && pools.every(([, pool]) => pool.flooded === pool.capacity),
  pools.map(([name, pool]) => `${name} ${pool.flooded}/${pool.capacity}`).join(", "),
);
const sameNormal = (a, b) => !!a && !!b && a.every((value, i) => Math.abs(value - b[i]) < 1e-3);
const misplaced = fx.marks.filter((mark) => (mark.expected === null ? mark.normal !== null : !(mark.onFace && sameNormal(mark.normal, mark.expected))));
const normals = new Set(fx.marks.filter((mark) => mark.expected).map((mark) => mark.expected.join(",")));
check(
  `every mark lies on the face it struck and turns to it, none on a barrel (${fx.marks.length} shots, ${normals.size} face directions)`,
  fx.marks.length > 0 && normals.size === 6 && fx.marks.some((mark) => mark.expected === null) && misplaced.length === 0,
  misplaced.map((mark) => `${mark.id}: ${mark.normal?.map((v) => v.toFixed(2)).join("/") ?? "none"} ${Number.isFinite(mark.error) ? `${(mark.error * 100).toFixed(2)} cm off` : ""}`).join(" | ") ||
    `furthest ${(Math.max(...fx.marks.filter((mark) => mark.normal).map((mark) => mark.error)) * 100).toFixed(2)} cm off its face (lifted over the floor or a model)`,
);
check("the effects bench drew its frames", Object.keys(fx.frames).length >= 15, Object.keys(fx.frames).join(", "));

// The character bench (preview.ts): the real Character, measured in each state.
const bench = report.bench ?? {};
const states = Object.entries(bench);
const still = states.filter(([, s]) => s.alive && !s.moving);
const deg = (radians) => ((radians * 180) / Math.PI).toFixed(1);
const at = (id) => bench[id] ?? { neck: [NaN, NaN, NaN], neckOffset: NaN, headTop: NaN, feet: [NaN, NaN] };
check(`the bench measured every state (${states.length})`, states.length >= 20 + KINDS.length, states.map(([id]) => id).join(", "));
check(`standing neck at ${NECK} ± 0.03 m`, Math.abs(at("stand").neck[1] - NECK) <= 0.03, `${at("stand").neck[1]?.toFixed(3)} m`);
check(`crouched neck at ${CROUCHED_NECK} ± 0.04 m`, Math.abs(at("crouch").neck[1] - CROUCHED_NECK) <= 0.04, `${at("crouch").neck[1]?.toFixed(3)} m`);
const steep = ["aimUp", "aimDown", "crouchAimUp", "crouchAimDown"].map((id) => [id, at(id)]);
check(
  `the neck stays within 0.15 m of the hitbox column at pitch ±${MAX_PITCH}, standing and crouched`,
  steep.every(([, s]) => s.neckOffset <= 0.15 && Math.abs(Math.abs(s.pitch) - MAX_PITCH) < 1e-6),
  steep.map(([id, s]) => `${id} ${s.neckOffset?.toFixed(3)}`).join(", "),
);
const heads = [0.5, 1, 2].map((scale) => [scale, at(`head${scale}`)]);
check(
  "the head top is at 1.5 + 0.26·s (± 0.05·s) for s = 0.5, 1 and 2",
  heads.every(([scale, s]) => Math.abs(s.headTop - (1.5 + 0.26 * scale)) <= 0.05 * scale),
  heads.map(([scale, s]) => `${scale}: ${s.headTop?.toFixed(3)} (box ${(1.5 + 0.26 * scale).toFixed(3)})`).join(", "),
);
const lifted = still.filter(([, s]) => s.feet.some((y) => !(y >= -0.03 && y <= 0.05)));
check(`both soles between -0.03 and 0.05 m in every still state (${still.length})`, still.length > 0 && lifted.length === 0, lifted.map(([id, s]) => `${id} ${s.feet.map((y) => y.toFixed(3)).join("/")}`).join(", ") || "ok");
const holds = KINDS.map((kind) => [kind, bench[`hold:${kind}`]]);
check(
  "every weapon's muzzle is in front of the character",
  holds.every(([, s]) => s && s.muzzleAhead >= 0.3),
  holds.map(([kind, s]) => `${kind} ${s?.muzzleAhead.toFixed(2)}`).join(", "),
);
// The knife and the grenade are held in the fist along the grip's line, not along the aim; the arm
// held low points the knife's blade ahead.
const FIST_HELD = ["KNIFE", "GRENADE"];
const guns = states.filter(([, s]) => s.alive && !FIST_HELD.includes(s.weapon));
const askew = guns.filter(([, s]) => s.gunToAim > (5 * Math.PI) / 180);
check("the gun points along the aim (within 5°) in every live state, aiming up, down and moving", guns.length > 0 && askew.length === 0, askew.map(([id, s]) => `${id} ${deg(s.gunToAim)}°`).join(", ") || `max ${deg(Math.max(...guns.map(([, s]) => s.gunToAim)))}°`);
const knife = at("hold:KNIFE");
// The bench faces −Z: ahead is down z.
const blade = knife.muzzle && knife.grip ? { ahead: knife.grip[2] - knife.muzzle[2], up: knife.muzzle[1] - knife.grip[1] } : null;
check(
  "the knife's blade points ahead of the fist and up from it",
  blade !== null && blade.ahead > 0.1 && blade.up > 0 && blade.up < blade.ahead,
  blade ? `tip ${(blade.ahead * 100).toFixed(1)} cm ahead, ${(blade.up * 100).toFixed(1)} cm up` : "not measured",
);
const grenade = at("hold:GRENADE");
check(
  "the grenade sits in the fist with its fuze up",
  !!grenade.muzzle && !!grenade.grip && grenade.muzzle[1] > grenade.grip[1],
  grenade.muzzle && grenade.grip ? `fuze ${((grenade.muzzle[1] - grenade.grip[1]) * 100).toFixed(1)} cm above the middle of the fist` : "not measured",
);
const knifeWrist = knife.wrist?.right;
check(
  `the knife hand's wrist bends at most ${KNIFE_WRIST_LIMIT}° from straight`,
  typeof knifeWrist === "number" && Number(deg(knifeWrist)) <= KNIFE_WRIST_LIMIT,
  typeof knifeWrist === "number" ? `${deg(knifeWrist)}°` : "not measured",
);
// The guns' holds bend the wrists further; shown, not held to a limit (yet).
const widest = (list, hand) => list.filter(([, s]) => typeof s.wrist?.[hand] === "number").reduce((best, item) => (!best || item[1].wrist[hand] > best[1].wrist[hand] ? item : best), null);
for (const hand of ["right", "left"]) {
  const most = widest(guns.filter(([id]) => id.startsWith("hold:")), hand);
  if (most) console.log(`INFO  third person: the most bent ${hand} wrist on a gun is ${deg(most[1].wrist[hand])}° (${most[0]})`);
}
const loose = states.filter(([, s]) => s.alive && s.support !== null && s.support > 0.03);
check("the left fist closes on the gun where the weapon says (within 3 cm)", loose.length === 0, loose.map(([id, s]) => `${id} ${(s.support * 100).toFixed(1)} cm`).join(", ") || `${states.filter(([, s]) => s.support !== null && s.alive).length} states`);
// The fist's place alone is measured through the same turn that put it there; the palm, read from
// the knuckles, shows the turn itself: up into a handguard, or toward the grip from its left side.
const supported = states.filter(([, s]) => s.alive && s.support !== null);
const palmAway = supported.filter(([, s]) => !(s.supportFacing > 0.7));
check(
  "the left palm faces the gun it holds (facing · wanted way > 0.7)",
  supported.length > 0 && palmAway.length === 0,
  palmAway.map(([id, s]) => `${id} ${s.supportFacing?.toFixed(2)}`).join(", ") || `min ${Math.min(...supported.map(([, s]) => s.supportFacing)).toFixed(2)} over ${supported.length} states`,
);
const plain = states.filter(([, s]) => s.alive && !s.shielded && !s.stunned);
const costly = plain.filter(([, s]) => s.calls > (s.weapon === "AKIMBO_SMG" ? 5 : 4));
check("at most 4 draw calls a character (body, gun, blob, name; 5 with two guns)", plain.length > 0 && costly.length === 0, costly.map(([id, s]) => `${id} ${s.calls}`).join(", ") || `max ${Math.max(...plain.map(([, s]) => s.calls))}`);
check("the shield adds one draw call, only while it is up", at("shield").calls === at("hold:PISTOL").calls + 1, `${at("shield").calls} with, ${at("hold:PISTOL").calls} without`);
console.log(`INFO  taser sparks: ${at("stunned").calls} draw calls while stunned (${at("stand").calls} without)`);

// The first-person bench (preview.ts): the real ViewModel, in each weapon's moments.
const fp = Object.entries(report.firstPerson ?? {});
const cm = (metres) => (metres === null || metres === undefined ? "none" : `${(metres * 100).toFixed(3)} cm`);
// Idle red and blue for every weapon, kick and reload for every gun, the knife's swing, the grenade's
// throw, two cycles, the shield and the knife in the free-for-all's neutral sleeves.
const fpExpected = KINDS.length * 2 + (KINDS.length - 2) * 2 + 1 + 1 + 2 + 1 + 1;
const fpIdle = (kind) => fp.some(([, s]) => s.weapon === kind && s.pose === "idle");
check(`the first-person bench measured every weapon's moments (${fp.length})`, fp.length >= fpExpected && KINDS.every(fpIdle), `${fp.length} of ${fpExpected}; ${KINDS.filter((kind) => !fpIdle(kind)).join(", ") || "every weapon idle"}`);
const fpLoose = fp.filter(([, s]) => s.right === null || s.right > HAND_TOLERANCE || (s.twoHanded && (s.left === null || s.left > HAND_TOLERANCE)));
const worst = (key) => cm(Math.max(...fp.map(([, s]) => s[key] ?? 0)));
check(
  `the right fist closes on every grip and the left fist on every two-handed gun, within ${HAND_TOLERANCE * 100} cm`,
  fp.length > 0 && fpLoose.length === 0,
  fpLoose.map(([id, s]) => `${id} right ${cm(s.right)} left ${cm(s.left)}`).slice(0, 4).join(", ") || `worst right ${worst("right")}, left ${worst("left")}`,
);
const fpKnife = fp.filter(([, s]) => s.weapon === "KNIFE");
const fpBent = fpKnife.filter(([, s]) => !(typeof s.wrist?.right === "number" && Number(deg(s.wrist.right)) <= KNIFE_WRIST_LIMIT));
check(
  `the knife hand's wrist bends at most ${KNIFE_WRIST_LIMIT}° from straight in first person (${fpKnife.length} states)`,
  fpKnife.length > 0 && fpBent.length === 0,
  fpBent.map(([id, s]) => `${id} ${typeof s.wrist?.right === "number" ? deg(s.wrist.right) + "°" : "none"}`).join(", ") || `max ${deg(Math.max(...fpKnife.map(([, s]) => s.wrist.right)))}°`,
);
for (const hand of ["right", "left"]) {
  const most = widest(fp.filter(([, s]) => s.weapon !== "KNIFE"), hand);
  if (most) console.log(`INFO  first person: the most bent ${hand} wrist on a gun is ${deg(most[1].wrist[hand])}° (${most[0]})`);
}
const offGun = fp.filter(([, s]) => !s.twoHanded && s.left !== null);
check("one-handed weapons keep the left hand off the gun", offGun.length === 0, offGun.map(([id]) => id).join(", ") || `${fp.filter(([, s]) => !s.twoHanded).length} states`);
const fpSupported = fp.filter(([, s]) => s.twoHanded && s.weapon !== "AKIMBO_SMG");
const fpTurned = fpSupported.filter(([, s]) => !(s.supportFacing > 0.7));
check(
  "the left palm faces up into every handguard it holds (facing · gun up > 0.7)",
  fpSupported.length > 0 && fpTurned.length === 0,
  fpTurned.map(([id, s]) => `${id} ${s.supportFacing?.toFixed(2) ?? "none"}`).join(", ") || `min ${Math.min(...fpSupported.map(([, s]) => s.supportFacing)).toFixed(2)} over ${fpSupported.length} states`,
);
const fpIdleStates = fp.filter(([, s]) => s.pose === "idle");
// The hands that hold the weapon in view: a one-handed weapon's grip hand, both akimbo hands, and a
// long gun's support hand (its grip hand may sit below the view). Knuckles within the screen.
const shownHand = (ndc) => !!ndc && ndc.every((value) => Math.abs(value) < 1);
const heldHands = (s) => (s.weapon === "AKIMBO_SMG" ? [s.hands.right, s.hands.left] : [s.twoHanded ? s.hands.left : s.hands.right]);
const handsOff = fpIdleStates.filter(([, s]) => !s.hands || !heldHands(s).every(shownHand));
check(
  "the hands holding each weapon are on screen in idle",
  fpIdleStates.length > 0 && handsOff.length === 0,
  handsOff.map(([id, s]) => `${id} ${heldHands(s).map((ndc) => (ndc ? ndc.slice(0, 2).map((v) => v.toFixed(2)).join("/") : "none")).join(" ")}`).join(", ") || `${fpIdleStates.length} idle states`,
);
const offScreen = fpIdleStates.filter(([, s]) => !s.muzzles.length || s.muzzles.some(([x, y, z]) => !(Math.abs(x) < 1 && Math.abs(y) < 1 && Math.abs(z) < 1)));
check(
  "every muzzle is on screen in idle",
  fpIdleStates.length > 0 && offScreen.length === 0,
  offScreen.map(([id, s]) => `${id} ${s.muzzles.map((m) => m.map((v) => v.toFixed(2)).join("/")).join(" ")}`).join(", ") || `${fpIdleStates.length} idle states`,
);
// The arms, one call per gun, the flash while it shows (just after a shot) and the shield's two parts.
const fpCalls = (s) => 1 + s.muzzles.length + (s.pose === "kick" ? 1 : 0) + (s.shield ? 2 : 0);
const fpWrong = fp.filter(([, s]) => s.calls > FP_MAX_CALLS || s.calls !== fpCalls(s));
check(
  `first-person draw calls: arms, guns, flash and shield, at most ${FP_MAX_CALLS}`,
  fp.length > 0 && fpWrong.length === 0,
  fpWrong.map(([id, s]) => `${id} ${s.calls} (expected ${fpCalls(s)})`).join(", ") || `max ${Math.max(...fp.map(([, s]) => s.calls))}`,
);
const armTriangles = [...new Set(fp.map(([, s]) => s.armTriangles))];
check(`the first-person arms draw at most ${ARMS_TRIANGLES} triangles`, armTriangles.length > 0 && armTriangles.every((count) => count > 0 && count <= ARMS_TRIANGLES), armTriangles.join(", "));

check("no console errors", errors.length === 0, errors.slice(0, 3).join(" | "));
check("no [art] warnings", artWarnings.length === 0, artWarnings.slice(0, 3).join(" | "));
const otherWarnings = warnings.filter((text) => !artWarnings.includes(text) && !driverNotes.includes(text));
check("no other console warnings", otherWarnings.length === 0, otherWarnings.length ? otherWarnings.slice(0, 3).join(" | ") : `${driverNotes.length} software-GL driver notes not counted`);
check("the preview never connects to the server", sockets.length === 0, sockets.join(" | "));

const failed = checks.filter((ok) => !ok).length;
console.log(`\n${checks.length - failed}/${checks.length} checks passed`);
process.exit(failed ? 1 : 0);
