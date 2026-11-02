// Crouch in the browser: fullscreen and keyboard lock (needed for Ctrl+W), the C
// key, and a screenshot of a crouching player seen by another human.
// Usage: node e2e/crouch-keys.mjs [url] [screenshot-dir]
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
  await page.goto(url);
  await page.fill(".login input", name);
  await page.click(".login button");
  return page;
};
const host = await open("host");
await host.waitForSelector(".create-room");
await host.uncheck(".bots-check");
await host.click(".create-room button");
await host.waitForSelector(".actions .primary");
const rival = await open("rival");
await rival.waitForSelector(".room-row button");
await rival.click(".room-row button");
await host.waitForFunction(() => document.querySelector(".team.blue")?.textContent.includes("rival"));
await host.click(".actions .primary");
for (const page of [host, rival]) {
  await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
  await page.click("#ui-root .pause");
  await page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });
}

const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  -> ${detail}` : ""}`);
};

const info = await host.evaluate(async () => ({
  fullscreen: !!document.fullscreenElement,
  keyboardApi: !!navigator.keyboard,
}));
check("clicking in enters fullscreen", info.fullscreen);
check("the Keyboard Lock API exists here", info.keyboardApi);

// A browser shortcut pressed with Ctrl must be swallowed while playing.
const swallowed = await host.evaluate(() => {
  const event = new KeyboardEvent("keydown", { code: "KeyS", key: "s", ctrlKey: true, cancelable: true, bubbles: true });
  window.dispatchEvent(event);
  window.dispatchEvent(new KeyboardEvent("keyup", { code: "KeyS", key: "s" }));
  return event.defaultPrevented;
});
check("Ctrl+S doesn't reach the browser", swallowed);

// Draw the rival's real model 4 m ahead of the camera, wherever they actually are.
await host.evaluate(() => {
  const g = window.__game;
  const character = [...g.remotes.values()][0].character;
  const update = character.update.bind(character);
  character.update = (dt, x, z, yaw, pitch, ...rest) =>
    update(dt, g.pos.x - Math.sin(g.yaw) * 4, g.pos.z - Math.cos(g.yaw) * 4, g.yaw + 2.6, pitch, ...rest);
});
await host.waitForFunction(() => !window.__game.lastSnapshot.players.some((p) => p.protected), null, { timeout: 15000 });
await host.waitForTimeout(500);
await host.screenshot({ path: join(shots, "stand-remote.png") });

await rival.keyboard.down("KeyC");
await host.waitForFunction(() => {
  const g = window.__game;
  return g.lastSnapshot.players.find((p) => p.id !== g.myId)?.crouching === true;
});
check("C crouches too", true);
await host.waitForTimeout(800);
await host.screenshot({ path: join(shots, "crouch-remote.png") });
await rival.keyboard.up("KeyC");

// Leaving the match restores the window.
await host.keyboard.press("Escape");
await host.evaluate(() => document.exitPointerLock());
await host.waitForFunction(() => !window.__game.locked);
await host.click("#ui-root .pause button");
await host.waitForFunction(() => !document.fullscreenElement, null, { timeout: 5000 });
check("leaving the match exits fullscreen", true);

await browser.close();
process.exit(results.every(Boolean) ? 0 : 1);
