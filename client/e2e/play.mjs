// End-to-end smoke test: two real browsers against a running server.
// Usage: node e2e/play.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { defaultUrl } from "./server.mjs";

const url = process.argv[2] ?? defaultUrl;
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

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

async function newPlayer(name) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.goto(url);
  await page.fill(".login input", name);
  await page.click(".login button");
  await page.waitForSelector(".room-list");
  return { page, errors };
}

const state = (page) =>
  page.evaluate(() => {
    const g = window.__game;
    return {
      x: g.pos.x,
      z: g.pos.z,
      yaw: g.yaw,
      magazine: g.magazine,
      reserve: g.reserve,
      reloading: g.reloadRemaining,
      alive: g.alive,
      locked: g.locked,
      players: g.lastSnapshot?.players.length ?? 0,
      tick: g.lastSnapshot?.tick ?? 0,
      remotes: g.remotes.size,
    };
  });

const host = await newPlayer("방장");
const guest = await newPlayer("게스트");

await host.page.screenshot({ path: join(shots, "1-lobby.png") });
await host.page.fill(".room-name-input", "테스트 5대5");
await host.page.click(".create-room button");
await host.page.waitForSelector(".room .teams");

await guest.page.waitForSelector(".room-row");
const listed = await guest.page.textContent(".room-row");
check("created room appears in other user's list", listed.includes("테스트 5대5"), listed.trim());
await guest.page.screenshot({ path: join(shots, "2-room-list.png") });

await guest.page.click(".room-row button");
await guest.page.waitForSelector(".room .teams");
await host.page.waitForFunction(() => document.querySelector(".team.blue")?.textContent.includes("게스트"));
check("guest joined and is visible to host", true);
await host.page.screenshot({ path: join(shots, "3-room.png") });

check("guest has no start button", (await guest.page.locator(".actions .primary").count()) === 0);
check("host has a start button", (await host.page.locator(".actions .primary").count()) === 1);

await host.page.click(".actions .primary");
await host.page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
await guest.page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
check("both players entered the match", true);

let s = await state(host.page);
check("5 vs 5: ten players in the snapshot", s.players === 10, `${s.players} players`);
check("rifle starts at 30/120", s.magazine === 30 && s.reserve === 120, `${s.magazine}/${s.reserve}`);

await host.page.click("#ui-root .pause");
await host.page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });
check("pointer lock acquired", true);

const before = await state(host.page);
await host.page.keyboard.down("KeyW");
await host.page.waitForTimeout(1200);
await host.page.keyboard.up("KeyW");
const after = await state(host.page);
const moved = Math.hypot(after.x - before.x, after.z - before.z);
check("W moves the player forward", moved > 1, `${moved.toFixed(2)} m`);

await host.page.waitForTimeout(400);
const local = await state(host.page);
const server = await host.page.evaluate(() => {
  const g = window.__game;
  const me = g.lastSnapshot.players.find((p) => p.id === g.myId);
  return { x: me.x, z: me.z };
});
const drift = Math.hypot(local.x - server.x, local.z - server.z);
check("prediction agrees with the server at rest", drift < 0.02, `${drift.toFixed(4)} m`);

await host.page.screenshot({ path: join(shots, "4-ingame.png") });

const yaw0 = (await state(host.page)).yaw;
await host.page.mouse.move(700, 360);
await host.page.mouse.move(800, 360);
await host.page.waitForTimeout(100);
check("mouse moves the view", (await state(host.page)).yaw !== yaw0);

await host.page.waitForFunction(() => window.__game.alive, null, { timeout: 10000 });
const mag0 = (await state(host.page)).magazine;
await host.page.mouse.down();
await host.page.waitForTimeout(700);
await host.page.screenshot({ path: join(shots, "5-firing.png") });
await host.page.mouse.up();
await host.page.waitForTimeout(300);
const fired = await state(host.page);
check("left click fires and spends ammo", fired.magazine < mag0 || !fired.alive, `${mag0} -> ${fired.magazine}`);

if (fired.alive && fired.magazine < 30) {
  await host.page.keyboard.press("KeyR");
  await host.page.waitForTimeout(500);
  const reloading = await state(host.page);
  check("R starts a reload", reloading.reloading > 0 || !reloading.alive, `${reloading.reloading.toFixed(2)}s left`);
  await host.page.screenshot({ path: join(shots, "6-reload.png") });
  await host.page.waitForTimeout(2200);
  const done = await state(host.page);
  check(
    "reload refills the magazine from reserve",
    !done.alive || (done.magazine === 30 && done.reserve < 120),
    `${done.magazine}/${done.reserve}`,
  );
}

// Crouching: lowers the view, reaches the server, and stays in sync while walking.
if ((await state(host.page)).alive) {
  const stance = () => host.page.evaluate(() => {
    const g = window.__game;
    const me = g.lastSnapshot.players.find((p) => p.id === g.myId);
    return { y: g.camera.position.y, server: me.crouching, gap: Math.hypot(g.pos.x - me.x, g.pos.z - me.z), alive: g.alive, eye: g.config.eyeHeight, crouchEye: g.config.crouchEyeHeight };
  });
  await host.page.keyboard.down("Control");
  await host.page.keyboard.down("KeyW");
  await host.page.waitForTimeout(1000);
  await host.page.keyboard.up("KeyW");
  await host.page.waitForTimeout(500);
  const low = await stance();
  await host.page.screenshot({ path: join(shots, "crouch.png") });
  if (low.alive) {
    check("Ctrl lowers the view", Math.abs(low.y - low.crouchEye) < 0.02, `${low.y.toFixed(2)} m`);
    check("the server sees the crouch", low.server === true);
    check("prediction agrees after a crouched walk", low.gap < 0.05, `${low.gap.toFixed(4)} m`);
  }
  await host.page.keyboard.up("Control");
  await host.page.waitForTimeout(500);
  const high = await stance();
  if (high.alive) check("releasing Ctrl stands back up", Math.abs(high.y - high.eye) < 0.02 && high.server === false, `${high.y.toFixed(2)} m`);
}

// Weapon slots: each keeps its own ammo, and the knife attacks without any.
const inHand = () => host.page.evaluate(() => {
  const g = window.__game;
  return { kind: g.weaponKind, name: g.stats().name, magazine: g.magazine, reserve: g.reserve, synced: g.lastSnapshot.lastInputSeq >= g.pendingSlotSeq };
});
const settle = () => host.page.waitForFunction(() => {
  const g = window.__game;
  const me = g.lastSnapshot.players.find((p) => p.id === g.myId);
  return g.lastSnapshot.lastInputSeq >= g.pendingSlotSeq && me.weapon === g.weaponKind;
});
if ((await state(host.page)).alive) {
  const primaryBefore = await inHand();
  await host.page.keyboard.press("Digit2");
  await host.page.waitForFunction(() => window.__game.stats().name === "PT-9");
  await settle();
  const pistol = await inHand();
  check("2 draws the pistol at 12/36", pistol.magazine === 12 && pistol.reserve === 36, `${pistol.name} ${pistol.magazine}/${pistol.reserve}`);
  await host.page.screenshot({ path: join(shots, "slot-pistol.png") });
  await host.page.keyboard.press("Digit3");
  await host.page.waitForFunction(() => window.__game.stats().name === "KNIFE");
  await settle();
  await host.page.waitForTimeout(450);
  await host.page.screenshot({ path: join(shots, "slot-knife-idle.png") });
  await host.page.mouse.down();
  await host.page.waitForTimeout(150);
  await host.page.screenshot({ path: join(shots, "slot-knife.png") });
  await host.page.mouse.up();
  const knife = await inHand();
  check("3 draws the knife, which uses no ammo", knife.name === "KNIFE" && knife.magazine === 0, `${knife.name} ${knife.magazine}/${knife.reserve}`);
  await host.page.keyboard.press("Digit1");
  await host.page.waitForFunction(() => window.__game.stats().name !== "KNIFE");
  await settle();
  const primaryAfter = await inHand();
  if ((await state(host.page)).alive) {
    check("1 returns to the primary with its ammo kept", primaryAfter.reserve === primaryBefore.reserve, `${primaryAfter.name} ${primaryAfter.magazine}/${primaryAfter.reserve}`);
  }
}

// B opens the weapon menu; a pick outside spawn protection waits for the next spawn.
const before_menu = await state(host.page);
await host.page.keyboard.press("KeyB");
await host.page.waitForSelector(".weapon-menu:not(.hidden) .weapon-card");
check("B opens the weapon menu", true);
check("the cursor is freed for the menu", !(await state(host.page)).locked);
await host.page.screenshot({ path: join(shots, "6b-weapon-menu.png") });
await host.page.keyboard.press("Digit4");
await host.page.waitForFunction(() => document.querySelector(".weapon-menu").classList.contains("hidden"));
check("number key picks and closes the menu", before_menu !== null);
await host.page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });
check("pointer lock returns after picking", true);
await host.page.waitForFunction(() => window.__game.selectedWeapon === 4, null, { timeout: 3000 });
check("server acknowledged the SMG selection", true);

await host.page.keyboard.down("Tab");
await host.page.waitForTimeout(150);
await host.page.screenshot({ path: join(shots, "7-scoreboard.png") });
await host.page.keyboard.up("Tab");

// Guest leaving mid-match: nobody takes its seat, and the match keeps running a player short.
await guest.page.evaluate(() => window.__game.connection.send({ case: "leaveRoom", value: {} }));
await guest.page.waitForSelector(".room-list");
await host.page.waitForTimeout(500);
s = await state(host.page);
const bots = await host.page.evaluate(() => [...window.__game.roster.values()].filter((e) => e.isBot).length);
check("a leaver is not replaced: nine players, still eight bots", s.players === 9 && s.remotes === 8 && bots === 8, `${s.players} players, ${s.remotes} others, ${bots} bots`);

check("no page errors (host)", host.errors.length === 0, host.errors.slice(0, 3).join(" | "));
check("no page errors (guest)", guest.errors.length === 0, guest.errors.slice(0, 3).join(" | "));

await browser.close();
const failed = checks.filter((ok) => !ok).length;
console.log(`\n${checks.length - failed}/${checks.length} checks passed`);
process.exit(failed ? 1 : 0);
