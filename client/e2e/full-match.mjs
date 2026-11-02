// Plays a whole bot-filled match in a real browser: start -> 30 kills -> results
// -> back in the waiting room -> second match starts.
// Usage: node e2e/full-match.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const url = process.argv[2] ?? "http://localhost:8080/?debug";
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({
  channel: "msedge",
  headless: true,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const open = async (name) => {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(url);
  await page.fill(".login input", name);
  await page.click(".login button");
  await page.waitForSelector(".create-room");
  return page;
};
const errors = [];
let failed = 0;
const check = (name, ok, detail = "") => {
  if (!ok) failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  -> " + detail : ""}`);
};

const host = await open("host");
const watcher = await open("watcher");
// The second map, played with a weapon picked in the room.
await host.selectOption(".map-select", "2");
await host.click(".create-room button");
await host.waitForSelector(".weapon-card");
await host.screenshot({ path: join(shots, "room-weapons.png") });
await host.click(".weapon-card:nth-child(2)");
await host.waitForSelector(".weapon-card:nth-child(2).selected");
await host.waitForSelector(".actions .primary");
await host.click(".actions .primary");
await host.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });

const loadout = await host.evaluate(() => [window.__game.magazine, window.__game.reserve, window.__game.stats().name]);
check("sniper from the room is in hand at 5/20", loadout[0] === 5 && loadout[1] === 20, loadout.join(" "));

// Bolt action, scope and automatic reload through real input.
await host.click("#ui-root .pause");
await host.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });
await host.mouse.click(640, 360, { button: "right" });
// The camera zooms on the next rendered frame.
await host.waitForFunction(() => window.__game.scoped() && window.__game.camera.fov < 30, null, { timeout: 3000 });
check("right click scopes in", true);
await host.screenshot({ path: join(shots, "scope.png") });
await host.mouse.down();
await host.waitForTimeout(600);
await host.mouse.up();
const afterShot = await host.evaluate(() => ({ m: window.__game.magazine, scoped: window.__game.scoped(), alive: window.__game.alive }));
check("bolt action fires one round and drops the scope", !afterShot.alive || (afterShot.m === 4 && !afterShot.scoped), JSON.stringify(afterShot));
await host.mouse.down();
await host.waitForFunction(() => window.__game.reloadRemaining > 0 || !window.__game.alive, null, { timeout: 15000, polling: 50 });
await host.mouse.up();
const auto = await host.evaluate(() => ({ alive: window.__game.alive, reloading: window.__game.reloadRemaining, m: window.__game.magazine }));
check("emptying the magazine reloads automatically", !auto.alive || (auto.reloading > 0 && auto.m === 0), JSON.stringify(auto));
await host.evaluate(() => document.exitPointerLock());

await watcher.waitForFunction(() => document.querySelector(".status.live"));
check("running room shows as in game in the list", true);
check("running room can't be joined", await watcher.locator(".room-row button").isDisabled());

await host.waitForFunction(() => window.__game.matchOver, null, { timeout: 600000, polling: 500 });
await host.screenshot({ path: join(shots, "8-results.png") });
const result = await host.evaluate(() => {
  const s = window.__game.lastSnapshot;
  return { red: s.redScore, blue: s.blueScore, title: document.querySelector(".board-title").textContent, ticks: s.tick };
});
check("match ended at exactly 30 kills", Math.max(result.red, result.blue) === 30, `${result.red}:${result.blue} (${result.title}, ${(result.ticks / 30).toFixed(0)}s)`);
check("loser stayed under 30", Math.min(result.red, result.blue) < 30);

await host.waitForSelector(".room .teams", { timeout: 15000 }).catch(async (error) => {
  console.log("page errors:", errors);
  console.log("page state:", await host.evaluate(() => ({
    lobbyHidden: document.querySelector(".lobby")?.className,
    canvases: document.querySelectorAll("#game-root canvas").length,
    disposed: window.__game?.disposed,
    title: document.querySelector(".board-title")?.textContent,
  })));
  throw error;
});
check("returned to the waiting room", (await host.locator("#game-root canvas").count()) === 0);
check("HUD removed", (await host.locator(".hud").count()) === 0);
await host.click(".actions .primary");
await host.waitForFunction(() => window.__game?.hasFirstSnapshot && window.__game.lastSnapshot.redScore === 0, null, { timeout: 30000 });
check("a second match starts from the same room", true);
check("no page errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
process.exit(failed ? 1 : 0);
