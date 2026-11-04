// Regression test: no name tag may show for a player behind a wall.
// Usage: node e2e/name-tags.mjs [url] [screenshot-dir]
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
await page.fill(".login input", "tags");
await page.click(".login button");
await page.waitForSelector(".create-room");
await page.click(".create-room button");
await page.waitForSelector(".actions .primary");
await page.click(".actions .primary");
await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });

// Samples tags after each rendered frame. The line-of-sight test is written out
// here rather than calling the game's, so it can't share a bug with it.
await page.evaluate(() => {
  const g = window.__game;
  const rec = { blockedTagged: 0, blockedHidden: 0, clearTagged: 0, frames: 0, example: "" };
  window.__tags = rec;
  const blocked = (from, to) => {
    for (const b of g.boxes) {
      let t0 = 0;
      let t1 = 1;
      let miss = false;
      for (const [o, e, min, max] of [
        [from[0], to[0], b.minX, b.maxX],
        [from[1], to[1], b.minY, b.maxY],
        [from[2], to[2], b.minZ, b.maxZ],
      ]) {
        const d = e - o;
        if (Math.abs(d) < 1e-7) {
          if (o < min || o > max) miss = true;
          continue;
        }
        const a = (min - o) / d;
        const c = (max - o) / d;
        t0 = Math.max(t0, Math.min(a, c));
        t1 = Math.min(t1, Math.max(a, c));
      }
      if (!miss && t0 <= t1) return true;
    }
    return false;
  };
  const tick = () => {
    if (g.alive) {
      rec.frames++;
      const y = g.config.eyeHeight;
      for (const remote of g.remotes.values()) {
        if (!remote.state?.alive) continue;
        const shown = remote.character.name.visible;
        if (blocked([g.pos.x, y, g.pos.z], [remote.shownX, y, remote.shownZ])) {
          if (shown) {
            rec.blockedTagged++;
            rec.example ||= `${remote.entry.name}, ally=${remote.entry.team === g.myTeam}`;
          } else rec.blockedHidden++;
        } else if (shown) rec.clearTagged++;
      }
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});

await page.waitForTimeout(25000);
const r = await page.evaluate(() => window.__tags);
await page.screenshot({ path: join(shots, "name-tags.png") });
await browser.close();

console.log(JSON.stringify(r));
const checks = [
  ["no tag shows behind a wall", r.blockedTagged === 0],
  ["the run saw players behind walls", r.blockedHidden + r.blockedTagged > 0],
  ["tags still show in clear view", r.clearTagged > 0],
];
for (const [name, ok] of checks) console.log(`${ok ? "PASS" : "FAIL"}  ${name}`);
process.exit(checks.every(([, ok]) => ok) ? 0 : 1);
