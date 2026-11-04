// Augment mode in the browser: mode choice, cards, refresh, wait, HP, models.
// Usage: node e2e/augment.mjs [url] [screenshot-dir]
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
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
await page.goto(url);
await page.fill(".login input", "aug");
await page.click(".login button");
await page.waitForSelector(".create-room");

const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  -> ${detail}` : ""}`);
};
const SHOTGUN = 5;
const WALLHACK = 9;

await page.selectOption(".mode-select", { label: "증강모드" });
await page.fill(".room-name-input", "증강 방");
await page.click(".create-room button");
await page.waitForSelector(".actions .primary");
const roomHead = await page.textContent(".panel-head");
check("the room shows its mode", roomHead.includes("증강모드"), roomHead);

await page.click(".actions .primary");
await page.waitForSelector(".augment-card");
const names = () => page.$$eval(".augment-name", (nodes) => nodes.map((n) => n.textContent));
const first = await names();
check("three cards are dealt", first.length === 3 && new Set(first).size === 3, first.join(", "));
await page.screenshot({ path: join(shots, "augment-cards.png") });

await page.locator(".augment-refresh").nth(1).click();
await page.waitForFunction(() => document.querySelectorAll(".augment-refresh")[1].disabled);
const second = await names();
check("a card's button swaps only that card", second[0] === first[0] && second[2] === first[2] && !first.includes(second[1]), second.join(", "));
check("that card can't be swapped again", await page.locator(".augment-refresh").nth(1).isDisabled());
check("the others still can", !(await page.locator(".augment-refresh").nth(0).isDisabled()));
await page.screenshot({ path: join(shots, "augment-card-swap.png") });
const timer = await page.textContent(".augment-timer");
check("choosing has its own 15 seconds", parseInt(timer) > 8, timer);

const before = await page.evaluate(() => ({ x: window.__game.pos.x, z: window.__game.pos.z }));
await page.click(".augment-card");
await page.waitForFunction(() => document.querySelector(".augment-offer").classList.contains("hidden"));
const picked = await page.evaluate(() => {
  const g = window.__game;
  const me = g.lastSnapshot.players.find((p) => p.id === g.myId);
  return {
    augment: me.augment,
    hp: me.hp,
    frozen: g.frozen,
    locked: g.locked,
    badge: document.querySelector(".augment-badge").textContent,
    botsPicked: g.lastSnapshot.players.filter((p) => p.id !== g.myId).every((p) => p.augment !== 0),
  };
});
check("the chosen card is applied", picked.augment !== 0 && second.some((name) => picked.badge.startsWith(name)), picked.badge);
check("HP is 150, or 200 with a vest", picked.hp === 150 || (picked.hp === 200 && /방탄조끼/.test(picked.badge)), String(picked.hp));
check("bots chose too", picked.botsPicked);
check("still waiting after choosing", picked.frozen);

await page.keyboard.down("KeyW");
await page.waitForTimeout(300);
const during = await page.evaluate(() => ({ x: window.__game.pos.x, z: window.__game.pos.z, left: window.__game.lastSnapshot.freezeLeft }));
check("no moving during the wait", during.left <= 0 || (during.x === before.x && during.z === before.z), `${during.left.toFixed(1)}s left`);
await page.screenshot({ path: join(shots, "augment-countdown.png") });
await page.waitForFunction(() => window.__game.lastSnapshot.freezeLeft <= 0, null, { timeout: 8000 });
if (await page.evaluate(() => window.__game.locked)) {
  await page.waitForTimeout(800);
  const after = await page.evaluate(() => ({ x: window.__game.pos.x, z: window.__game.pos.z, alive: window.__game.alive }));
  check("moves once the wait ends", !after.alive || Math.hypot(after.x - before.x, after.z - before.z) > 0.5);
}
await page.keyboard.up("KeyW");
const armed = await page.evaluate(() => {
  const g = window.__game;
  return { hudMax: document.querySelector(".hp-bar").style.width, clock: document.querySelector(".clock").textContent };
});
check("the clock starts after the wait", armed.clock < "00:03", armed.clock);

// Models and the Wallhack pass: forced locally, since the deal is random.
await page.evaluate((wallhack) => {
  const g = window.__game;
  const onSnapshot = g.onSnapshot.bind(g);
  g.onSnapshot = (snapshot) => {
    onSnapshot(snapshot);
    g.myActive = [wallhack];
  };
  const enemy = [...g.remotes.values()].find((r) => r.entry.team !== g.myTeam);
  const update = enemy.character.update.bind(enemy.character);
  enemy.character.update = (dt, x, z, yaw, pitch, moving, alive, shielded, time, crouching) =>
    update(dt, g.pos.x - Math.sin(g.yaw) * 4, g.pos.z - Math.cos(g.yaw) * 4, g.yaw + 2.6, pitch, moving, true, false, time, crouching);
  const setLook = enemy.character.setLook.bind(enemy.character);
  enemy.character.setLook = () => setLook(2, false, true);
  enemy.character.setWeapon(7);
  enemy.character.setWeapon = () => {};
}, WALLHACK);
await page.waitForTimeout(400);
await page.screenshot({ path: join(shots, "augment-xray.png") });

for (const [kind, label] of [[7, "shotgun"], [8, "missile"], [10, "magnum"], [11, "akimbo"], [12, "taser"]]) {
  await page.evaluate((k) => {
    const g = window.__game;
    g.viewModel.setWeapon(k);
    g.effects.explosion(g.camera.position.clone().add({ x: -Math.sin(g.yaw) * 12, y: -0.5, z: -Math.cos(g.yaw) * 12 }));
    g.viewModel.setShield(k === 10);
    // A missile hanging in view, and what being paralysed looks like.
    const p = g.camera.position;
    g.syncMissiles([{ id: 999, x: p.x - Math.sin(g.yaw) * 6 + Math.cos(g.yaw) * 1.5, y: p.y + 0.3, z: p.z - Math.cos(g.yaw) * 6 - Math.sin(g.yaw) * 1.5, vx: Math.cos(g.yaw) * 0.01, vy: 0, vz: -Math.sin(g.yaw) * 0.01 }]);
    g.syncMissiles = () => {};
    g.showStun ??= g.hud.setStun.bind(g.hud);
    g.showStun(k === 12 ? 4 : 0);
    g.hud.setStun = () => {};
  }, kind);
  await page.waitForTimeout(80);
  await page.screenshot({ path: join(shots, `augment-${label}.png`) });
}

check("no page errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();

// A real shotgun: keep dealing until the card comes up, then fire it.
const browser2 = await chromium.launch({
  channel: "msedge",
  headless: true,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
let shotgunTried = false;
for (let attempt = 0; attempt < 12 && !shotgunTried; attempt++) {
  const p = await (await browser2.newContext({ viewport: { width: 960, height: 540 } })).newPage();
  await p.goto(url);
  await p.fill(".login input", `sg${attempt}`);
  await p.click(".login button");
  await p.waitForSelector(".create-room");
  await p.selectOption(".mode-select", { label: "증강모드" });
  await p.selectOption(".map-select", { index: 1 });
  await p.click(".create-room button");
  await p.waitForSelector(".actions .primary");
  await p.click(".actions .primary");
  await p.waitForSelector(".augment-card");
  const find = () => p.$$eval(".augment-card", (cards) => cards.findIndex((c) => c.querySelector(".augment-name").textContent === "샷건"));
  let index = await find();
  for (let slot = 0; slot < 3 && index < 0; slot++) {
    await p.locator(".augment-refresh").nth(slot).click();
    await p.waitForFunction((n) => document.querySelectorAll(".augment-refresh")[n].disabled, slot);
    index = await find();
  }
  if (index >= 0) {
    shotgunTried = true;
    await p.locator(".augment-card").nth(index).click();
    await p.waitForFunction(() => window.__game.lastSnapshot.freezeLeft <= 0, null, { timeout: 8000 });
    await p.evaluate(() => {
      const g = window.__game;
      window.__pellets = 0;
      const onShot = g.onShot.bind(g);
      g.onShot = (shot) => {
        if (shot.shooterId === g.myId) window.__pellets++;
        onShot(shot);
      };
    });
    if (!(await p.evaluate(() => window.__game.locked))) {
      await p.click("#ui-root .pause");
      await p.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });
    }
    await p.waitForFunction(() => window.__game.lastSnapshot.players.every((q) => q.id !== window.__game.myId || !q.protected || true));
    await p.keyboard.press("Digit2");
    await p.waitForFunction(() => window.__game.weaponKind === 7, null, { timeout: 5000 });
    await p.waitForTimeout(500);
    await p.mouse.down();
    await p.waitForFunction(() => window.__pellets >= 6, null, { timeout: 8000 });
    await p.mouse.up();
    const sg = await p.evaluate(() => ({ name: window.__game.stats().name, pellets: window.__pellets, mag: window.__game.magazine, shell: window.__game.stats().shellReload }));
    check("a real shotgun fires 6 pellets a shot", sg.name === "SG-12" && sg.pellets % 6 === 0, `${sg.name} ${sg.pellets} pellets, ${sg.mag} rounds left`);
    await p.waitForTimeout(150);
    await p.screenshot({ path: join(shots, "augment-real-shotgun.png") });
  }
  await p.context().close();
}
if (!shotgunTried) console.log("SKIP  shotgun never came up in 12 deals");
await browser2.close();
process.exit(results.every(Boolean) ? 0 : 1);
