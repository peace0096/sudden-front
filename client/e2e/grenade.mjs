// Slot 4: 4 takes the grenade out, the trigger throws it, the gun held before comes straight back,
// and the grenade flies, lands and goes off 3 seconds after the throw. Slot 4 is then spent.
// Usage: node e2e/grenade.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { defaultUrl } from "./server.mjs";

const url = process.argv[2] ?? defaultUrl;
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

// WeaponKind values (proto/game.proto).
const RIFLE = 1;
const GRENADE = 13;

const browser = await chromium.launch({
  channel: "msedge",
  headless: true,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
let failed = 0;
const check = (name, ok, detail = "") => {
  if (!ok) failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  -> ${detail}` : ""}`);
};
const errors = [];
const open = async (name) => {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
  page.on("pageerror", (e) => errors.push(`${name}: ${e.message}`));
  page.on("console", (m) => m.type() === "error" && errors.push(`${name}: ${m.text()}`));
  await page.goto(url);
  await page.fill(".login input", name);
  await page.click(".login button");
  return page;
};

const page = await open("thrower");
await page.waitForSelector(".create-room");
await page.uncheck(".bots-check");
await page.click(".create-room button");
await page.waitForSelector(".actions .primary");
const rival = await open("rival");
await rival.waitForSelector(".room-row button");
await rival.click(".room-row button");
await page.waitForFunction(() => document.querySelector(".team.blue")?.textContent.includes("rival"));
await rival.click(".actions .ready-btn");
await page.click(".actions .primary");
// The rival stays connected: with nobody left on blue, the match would end at once by forfeit.
await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
await page.click("#ui-root .pause");
await page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });

// Every explosion the game draws, and every grenade the server reports, with when.
await page.evaluate(() => {
  const g = window.__game;
  const rec = { explosions: [], last: null };
  window.__rec = rec;
  const explosion = g.effects.explosion.bind(g.effects);
  g.effects.explosion = (center) => {
    rec.explosions.push({ at: performance.now(), x: center.x, y: center.y, z: center.z });
    explosion(center);
  };
  const onSnapshot = g.onSnapshot.bind(g);
  g.onSnapshot = (snapshot) => {
    if (snapshot.grenades.length) rec.last = { ...snapshot.grenades[0] };
    onSnapshot(snapshot);
  };
});

const slotBar = () => page.textContent(".slot-bar");
const me = () =>
  page.evaluate(() => {
    const g = window.__game;
    const state = g.lastSnapshot.players.find((p) => p.id === g.myId);
    return { held: g.weaponKind, server: state.weapon, left: g.grenadesLeft, serverLeft: g.lastSnapshot.grenadesLeft, x: g.pos.x, z: g.pos.z };
  });

check("slot 4 holds one grenade", (await slotBar()).includes("4 수류탄 ×1") && (await me()).left === 1, await slotBar());

await page.keyboard.press("Digit4");
await page.waitForFunction(
  (kind) => {
    const g = window.__game;
    return g.weaponKind === kind && g.lastSnapshot.players.find((p) => p.id === g.myId).weapon === kind;
  },
  GRENADE,
  { timeout: 5000 },
);
check("4 takes the grenade out, here and on the server", true);
const ammo = await page.evaluate(() => [document.querySelector(".ammo-count .mag").textContent, document.querySelector(".ammo-count .reserve").textContent]);
check("the counter shows the grenade and no reserve", ammo[0] === "1" && ammo[1] === "—", ammo.join(" / "));
await page.screenshot({ path: join(shots, "grenade-in-hand.png") });

// Lobbed a little up along the open ground in front of the spawn points (x −46), away from the
// nearer end of the map, where no wall throws it back; the press is held long enough for a slow
// frame to see it.
await page.evaluate(() => {
  const g = window.__game;
  g.yaw = g.pos.z < 0 ? Math.PI : 0;
  g.pitch = 0.3;
});
await page.waitForTimeout(600);
const start = await me();
const thrownAt = await page.evaluate(() => performance.now());
await page.mouse.down();
await page.waitForTimeout(400);
await page.mouse.up();

await page.waitForFunction(() => window.__game.lastSnapshot.grenades.length === 1, null, { timeout: 5000 });
check("the server has the grenade", true);
await page.waitForFunction((kind) => window.__game.lastSnapshot.players.find((p) => p.id === window.__game.myId).weapon === kind, RIFLE, { timeout: 5000 });
const after = await me();
check("the rifle comes straight back, here and on the server", after.held === RIFLE && after.server === RIFLE, JSON.stringify(after));
check("slot 4 is spent", after.left === 0 && after.serverLeft === 0 && (await slotBar()).includes("4 수류탄 ×0"), await slotBar());
check(
  "the spent slot is dimmed",
  await page.evaluate(() => [...document.querySelectorAll(".slot-chip")].some((chip) => chip.textContent.startsWith("4") && chip.classList.contains("spent"))),
);
check("the grenade is drawn in the world", await page.evaluate(() => window.__game.grenades.size === 1));
await page.waitForTimeout(250);
await page.screenshot({ path: join(shots, "grenade-flight.png") });

await page.waitForFunction(() => window.__rec.explosions.length > 0, null, { timeout: 8000 });
const rec = await page.evaluate(() => window.__rec);
const fuse = (rec.explosions[0].at - thrownAt) / 1000;
check("it goes off about 3 s after the throw", fuse > 2.6 && fuse < 4, `${fuse.toFixed(2)} s`);
const blast = rec.explosions[0];
const flew = Math.hypot(blast.x - start.x, blast.z - start.z);
check("where it lay, thrown forward some way", rec.last && Math.hypot(blast.x - rec.last.x, blast.z - rec.last.z) < 0.05 && flew > 5, `${flew.toFixed(1)} m from the throw, at height ${blast.y.toFixed(2)}`);
check("then it is gone", await page.evaluate(() => window.__game.grenades.size === 0 && window.__game.lastSnapshot.grenades.length === 0));
await page.screenshot({ path: join(shots, "grenade-explosion.png") });

// None left: 4 is nothing to switch to.
await page.keyboard.press("Digit4");
await page.waitForTimeout(500);
check("4 does nothing with no grenade left", (await me()).held === RIFLE);

check("no page or console errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
process.exit(failed ? 1 : 0);
