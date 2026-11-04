// Rendering cost: a bot-filled match on each map, sampled through Game.perf() once a second with the
// ?debug&perf overlay on. Each map gets a fresh browser, so its first frame compiles every shader
// cold. Prints the numbers and saves them as perf.json in the screenshot folder. Checks the full
// room's draw calls against the art manifest's budgets (drawCalls.typical for the mean, .max for the
// largest sample), and the map's fixed objects alone against drawCalls.map: the scene's "world"
// group (world.ts) drawn once from above with everything in view and nothing else shown. Then it
// holds the trigger down for ten seconds in the full room: the effects alone (the scene's "effects"
// group, effects.ts, drawn with nothing else shown) stay within drawCalls.effects every second, and
// the renderer's geometry count never grows, so no shot allocates any. Every gun model is drawn
// once before counting, so a bot respawning with a gun not yet seen isn't taken for growth.
// Usage: node e2e/perf.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { defaultUrl } from "./server.mjs";

const url = process.argv[2] ?? defaultUrl;
const perfUrl = url + (url.includes("?") ? "&perf" : "?perf");
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

const BUDGET = JSON.parse(readFileSync(fileURLToPath(new URL("../src/game/art/manifest.json", import.meta.url)), "utf8")).budgets.drawCalls;
const SAMPLE_MS = 20000;
const SAMPLE_EVERY_MS = 1000;
const FIRE_SECONDS = 10;
// The create form's map select: [option value, MapId name, name in the room header].
const MAPS = [
  ["1", "SUPPLY_YARD", "보급창고"],
  ["2", "WAREHOUSE", "웨어하우스"],
];

const checks = [];
function check(name, ok, detail = "") {
  checks.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  -> " + detail : ""}`);
}

const report = { url: perfUrl, sampleMs: SAMPLE_MS, maps: {} };
for (const [value, map, label] of MAPS) {
  const browser = await chromium.launch({
    channel: "msedge",
    headless: true,
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
  });
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.goto(perfUrl);
  await page.fill(".login input", `perf ${value}`);
  await page.click(".login button");
  await page.waitForSelector(".create-room");
  await page.selectOption(".map-select", value);
  await page.click(".create-room button");
  await page.waitForSelector(".actions .primary");
  const header = await page.textContent(".panel-head");
  check(`${map}: the room is on ${label}`, header.includes(label), header);
  await page.click(".actions .primary");
  await page.waitForFunction(() => window.__game?.hasFirstSnapshot && window.__game.perf().firstFrameMs !== null, null, { timeout: 60000 });

  const samples = [];
  const until = Date.now() + SAMPLE_MS;
  while (Date.now() < until) {
    await page.waitForTimeout(SAMPLE_EVERY_MS);
    samples.push(await page.evaluate(() => ({ ...window.__game.perf(), players: window.__game.lastSnapshot.players.length })));
  }
  // The map's fixed objects alone, every one of them in view from 140 m up; the shadow map was
  // baked in the first frame, so this is one pass.
  const fixed = await page.evaluate(() => {
    const g = window.__game;
    const world = g.scene.getObjectByName("world");
    if (!world) return null;
    const hidden = g.scene.children.filter((child) => child !== world && child.visible);
    for (const child of hidden) child.visible = false;
    const camera = g.camera.clone();
    camera.fov = 60;
    camera.position.set(0, 140, 0.001);
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
    g.renderer.info.reset();
    g.renderer.render(g.scene, camera);
    const { calls, triangles } = g.renderer.info.render;
    for (const child of hidden) child.visible = true;
    return { calls, triangles };
  });
  const fire = await page.evaluate(async (seconds) => {
    const g = window.__game;
    const effects = g.scene.getObjectByName("effects");
    if (!effects) return null;
    // Every gun once, so its geometry is on the GPU before anything is counted.
    const held = g.weaponKind;
    for (const { kind } of g.config.weapons) {
      g.viewModel.setWeapon(kind);
      g.renderer.render(g.viewModel.scene, g.viewModel.camera);
    }
    g.viewModel.setWeapon(held);
    // The effects alone: every other child of the scene hidden for one render.
    const effectCalls = () => {
      const shown = g.scene.children.filter((child) => child !== effects && child.visible);
      for (const child of shown) child.visible = false;
      g.renderer.info.reset();
      g.renderer.render(g.scene, g.camera);
      const calls = g.renderer.info.render.calls;
      for (const child of shown) child.visible = true;
      return calls;
    };
    const shots = { mine: 0, all: 0 };
    const onShot = g.onShot.bind(g);
    g.onShot = (shot) => {
      shots.all++;
      if (shot.shooterId === g.myId) shots.mine++;
      onShot(shot);
    };
    // The trigger held as a player holds it, with the mouse captured; a death lets go of neither.
    const hold = () => {
      g.locked = true;
      g.trigger = true;
    };
    const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    hold();
    await wait(500);
    const before = g.renderer.info.memory.geometries;
    const samples = [];
    for (let second = 0; second < seconds; second++) {
      await wait(1000);
      hold();
      const live = Object.fromEntries(Object.entries(g.effects.measure().pools).map(([name, pool]) => [name, pool.count]));
      samples.push({ calls: effectCalls(), geometries: g.renderer.info.memory.geometries, live });
    }
    g.trigger = false;
    g.locked = false;
    delete g.onShot;
    return { before, after: g.renderer.info.memory.geometries, samples, shots };
  }, FIRE_SECONDS);
  const overlay = await page.textContent(".perf-overlay").catch(() => null);
  await page.screenshot({ path: join(shots, `perf-${map.toLowerCase()}.png`) });
  await browser.close();

  const last = samples.at(-1);
  const column = (key) => samples.map((sample) => sample[key]);
  const mean = (values, digits = 0) => +(values.reduce((sum, v) => sum + v, 0) / values.length).toFixed(digits);
  const result = {
    samples: samples.length,
    players: last.players,
    calls: { mean: mean(column("calls")), max: Math.max(...column("calls")) },
    triangles: { mean: mean(column("triangles")), max: Math.max(...column("triangles")) },
    programs: Math.max(...column("programs")),
    geometries: Math.max(...column("geometries")),
    textures: Math.max(...column("textures")),
    // Each sample covers the last 120 frames; their mean covers the whole run.
    avgFrameMs: mean(column("avgFrameMs").filter((ms) => ms !== null), 1),
    p95FrameMs: mean(column("p95FrameMs").filter((ms) => ms !== null), 1),
    firstFrameMs: last.firstFrameMs,
    staticMap: fixed,
    sustainedFire: fire,
    raw: samples,
  };
  report.maps[map] = result;
  console.log(
    `${map}: calls ${result.calls.mean} (max ${result.calls.max}), triangles ${result.triangles.mean} (max ${result.triangles.max}), ` +
      `programs ${result.programs}, geometries ${result.geometries}, textures ${result.textures}, ` +
      `frame ${result.avgFrameMs} ms (p95 ${result.p95FrameMs}), first frame ${result.firstFrameMs} ms; ` +
      `static map ${fixed?.calls ?? "-"} draw calls, ${fixed?.triangles ?? "-"} triangles`,
  );

  check(`${map}: a full room of ten`, last.players === 10, `${last.players} players`);
  const counts = ["calls", "triangles", "programs", "geometries", "textures"];
  check(`${map}: every count is a positive number`, samples.every((s) => counts.every((key) => Number.isFinite(s[key]) && s[key] > 0)));
  check(
    `${map}: frame times are there`,
    last.avgFrameMs > 0 && last.p95FrameMs > 0 && last.firstFrameMs > 0,
    `avg ${last.avgFrameMs}, p95 ${last.p95FrameMs}, first ${last.firstFrameMs} ms`,
  );
  check(`${map}: the ?debug&perf overlay shows them`, overlay?.includes("draw call") ?? false, overlay?.replaceAll("\n", " | ") ?? "no overlay");
  check(`${map}: mean draw calls with a full room at most ${BUDGET.typical}`, result.calls.mean <= BUDGET.typical, `${result.calls.mean}`);
  check(`${map}: every sampled frame at most ${BUDGET.max} draw calls`, result.calls.max <= BUDGET.max, `max ${result.calls.max}`);
  check(`${map}: the map's fixed objects take at most ${BUDGET.map} draw calls`, fixed !== null && fixed.calls <= BUDGET.map, fixed ? `${fixed.calls} with everything in view` : "no world group");
  const fireCalls = fire ? Math.max(...fire.samples.map((sample) => sample.calls)) : NaN;
  const fireGeometries = fire ? Math.max(fire.after, ...fire.samples.map((sample) => sample.geometries)) : NaN;
  const busiest = fire ? Object.keys(fire.samples[0]?.live ?? {}).map((name) => `${name} ${Math.max(...fire.samples.map((sample) => sample.live[name]))}`).join(", ") : "";
  console.log(
    `${map}: ${FIRE_SECONDS} s of fire: ${fire?.shots.all ?? 0} shots in the room (${fire?.shots.mine ?? 0} mine), effects ${fire?.samples.map((sample) => sample.calls).join("/") ?? "-"} draw calls, ` +
      `geometries ${fire?.before ?? "-"} -> ${fire?.samples.map((sample) => sample.geometries).join("/") ?? "-"}; most alive: ${busiest}`,
  );
  check(`${map}: ${FIRE_SECONDS} s of sustained fire in the full room`, !!fire && fire.shots.all >= 20 && fire.shots.mine > 0, fire ? `${fire.shots.all} shots, ${fire.shots.mine} mine` : "no effects group");
  check(`${map}: the effects take at most ${BUDGET.effects} draw calls throughout`, !!fire && fire.samples.length === FIRE_SECONDS && fireCalls <= BUDGET.effects && fireCalls > 0, `max ${fireCalls}`);
  check(`${map}: no geometry is added while firing`, !!fire && fireGeometries <= fire.before, fire ? `${fire.before} before, at most ${fireGeometries} during and after` : "");
  check(`${map}: no page errors`, errors.length === 0, errors.slice(0, 3).join(" | "));
}

const file = join(shots, "perf.json");
writeFileSync(file, JSON.stringify(report, null, 2));
console.log(`saved ${file}`);
const failed = checks.filter((ok) => !ok).length;
console.log(`\n${checks.length - failed}/${checks.length} checks passed`);
process.exit(failed ? 1 : 0);
