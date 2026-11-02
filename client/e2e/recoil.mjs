// Sustained fire: the crosshair climbs and opens, and the bullets follow it.
// Usage: node e2e/recoil.mjs [url] [screenshot-dir]
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
const page = await open("shooter");
await page.waitForSelector(".create-room");
await page.uncheck(".bots-check");
await page.click(".create-room button");
await page.waitForSelector(".actions .primary");
const rival = await open("rival");
await rival.waitForSelector(".room-row button");
await rival.click(".room-row button");
await page.waitForFunction(() => document.querySelector(".team.blue")?.textContent.includes("rival"));
await page.click(".actions .primary");
await rival.close();
await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
await page.click("#ui-root .pause");
await page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });

// Records the server's actual shots next to the crosshair at the same moment.
await page.evaluate(() => {
  const g = window.__game;
  g.pitch = 0.15; // Over the walls, so shots always fly their full length.
  const rec = { shots: [], gaps: [] };
  window.__rec = rec;
  const gap = () => parseFloat(g.hud.crosshair.style.getPropertyValue("--gap"));
  rec.aims = [];
  const fire = g.tryLocalFire.bind(g);
  g.tryLocalFire = () => {
    const ready = g.fireCooldown <= 0;
    const aim = [g.aimPitch(), g.aimYaw()];
    fire();
    if (ready && g.fireCooldown > 0) rec.aims.push(aim);
  };
  const onShot = g.onShot.bind(g);
  g.onShot = (shot) => {
    if (shot.shooterId === g.myId) {
      const d = [shot.toX - shot.fromX, shot.toY - shot.fromY, shot.toZ - shot.fromZ];
      const len = Math.hypot(...d);
      rec.shots.push({ pitch: Math.asin(d[1] / len), yaw: Math.atan2(-d[0], -d[2]), camera: g.camera.rotation.x });
      rec.gaps.push(gap());
    }
    onShot(shot);
  };
  rec.gapBefore = gap();
});

await page.mouse.down();
await page.waitForFunction(() => window.__game.magazine <= 6, null, { timeout: 30000 });
await page.screenshot({ path: join(shots, "recoil-firing.png") });
await page.mouse.up();
await page.waitForTimeout(700);
const r = await page.evaluate(() => {
  const g = window.__game;
  return { ...window.__rec, recoil: g.recoil, bloom: g.bloom, gapAfter: parseFloat(g.hud.crosshair.style.getPropertyValue("--gap")) };
});
await browser.close();

const mean = (a) => a.reduce((sum, v) => sum + v, 0) / a.length;
const early = r.shots.slice(0, 3);
const late = r.shots.slice(-8);
const scatter = (group) => {
  const yawMean = mean(group.map((s) => s.yaw));
  return mean(group.map((s) => Math.abs(s.yaw - yawMean)));
};
const results = [];
const check = (name, ok, detail) => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}  -> ${detail}`);
};
check("fired enough rounds", r.shots.length >= 15, `${r.shots.length}`);
check("the first shot goes where the mouse aimed", Math.abs(r.shots[0].pitch - 0.15) < 0.01, `${r.shots[0].pitch.toFixed(4)} rad`);
const climb = mean(late.map((s) => s.pitch)) - mean(early.map((s) => s.pitch));
check("real shots climb", climb > 0.02, `${climb.toFixed(4)} rad`);
const offset = Math.abs(mean(late.map((s) => s.pitch - s.camera)));
check("shots stay centred on the crosshair", offset < 0.015, `${offset.toFixed(4)} rad`);
check("real shots scatter late in the burst", scatter(late) > 0.004, `yaw scatter ${scatter(early).toFixed(4)} -> ${scatter(late).toFixed(4)} rad`);
const lateGap = mean(r.gaps.slice(-8));
const jumps = r.aims.slice(1).map((a, i) => Math.hypot(a[0] - r.aims[i][0], a[1] - r.aims[i][1]));
const jumpPx = (rad) => rad * 495;
check("the aim visibly jumps on the first shots", mean(jumps.slice(0, 5)) * 495 > 4, `${jumpPx(mean(jumps.slice(0, 5))).toFixed(1)} px per shot`);
check("the aim keeps jumping late in the magazine", mean(jumps.slice(-8)) * 495 > 4, `${jumpPx(mean(jumps.slice(-8))).toFixed(1)} px per shot`);
check("the crosshair opens", lateGap > r.gapBefore + 8, `${r.gapBefore.toFixed(1)} -> ${lateGap.toFixed(1)} px`);
check("everything settles after release", r.recoil < 0.01 && r.gapAfter < r.gapBefore + 3, `recoil ${r.recoil.toFixed(4)}, gap ${r.gapAfter.toFixed(1)} px`);
process.exit(results.every(Boolean) ? 0 : 1);
