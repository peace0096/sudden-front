// Regression test: sliding along walls must never jump the camera.
// Usage: node e2e/wall-rub.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { defaultUrl } from "./server.mjs";

const url = process.argv[2] ?? defaultUrl;
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({
  channel: "msedge",
  headless: true,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
await page.goto(url);
await page.fill(".login input", "rub");
await page.click(".login button");
await page.waitForSelector(".create-room");
// No bots: nothing may interrupt the slide.
await page.uncheck(".bots-check");
await page.click(".create-room button");
await page.waitForSelector(".actions .primary");

// Starting needs an opponent when bots are off.
const rival = await (await browser.newContext()).newPage();
await rival.goto(url);
await rival.fill(".login input", "rival");
await rival.click(".login button");
await rival.waitForSelector(".room-row button");
await rival.click(".room-row button");
await page.waitForFunction(() => document.querySelector(".team.blue")?.textContent.includes("rival"));
await page.click(".actions .primary");
await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
await page.click("#ui-root .pause");
await page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });

// Records the largest camera and reconciliation jumps between rendered frames.
await page.evaluate(() => {
  const g = window.__game;
  const rec = { maxJump: 0, frames: 0, last: null, maxServerGap: 0 };
  window.__rub = rec;
  const tick = () => {
    const p = g.camera.position;
    if (rec.last) rec.maxJump = Math.max(rec.maxJump, Math.hypot(p.x - rec.last[0], p.z - rec.last[1]));
    rec.last = [p.x, p.z];
    rec.frames++;
    const me = g.lastSnapshot?.players.find((q) => q.id === g.myId);
    if (me) rec.maxServerGap = Math.max(rec.maxServerGap, Math.hypot(g.pos.x - me.x, g.pos.z - me.z));
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});

// Face a wall at a shallow angle in several directions, so every axis is rubbed.
let failed = false;
const headings = [Math.PI / 2 + 0.5, Math.PI / 2 - 0.5, 0.4, -0.4, Math.PI - 0.4, Math.PI + 0.4, -Math.PI / 2 + 0.5];
for (const yaw of headings) {
  await page.evaluate((y) => {
    window.__game.yaw = y;
    window.__rub.maxJump = 0;
    window.__rub.maxServerGap = 0;
    window.__rub.last = null;
  }, yaw);
  await page.keyboard.down("KeyW");
  await page.waitForTimeout(2600);
  await page.keyboard.up("KeyW");
  const r = await page.evaluate(() => ({
    jump: window.__rub.maxJump,
    gap: window.__rub.maxServerGap,
    frames: window.__rub.frames,
    x: window.__game.pos.x,
    z: window.__game.pos.z,
  }));
  // Even at 10 fps a frame only covers ~0.6 m at this speed.
  const ok = r.jump < 1.5;
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  yaw ${yaw.toFixed(2)}: largest frame jump ${r.jump.toFixed(3)} m, largest server gap ${r.gap.toFixed(3)} m at (${r.x.toFixed(1)}, ${r.z.toFixed(1)})`);
}
await page.screenshot({ path: join(shots, "wall-rub.png") });
await browser.close();
process.exit(failed ? 1 : 0);
