// Big Head mode in the browser: the mode is chosen when the room is made and shows in the room's
// header; the match gets its levels from GameConfig; everyone starts at level 0 with a normal head;
// the level panel above the HP shows mine, and moves when bots kill me or I kill; the dot and cute
// shots get their own banner and kill-feed tags (fed through Game.onKill, as a snapshot's events
// are); every player's head follows their level; and, when the bots get there in time, a King
// appears with the hammer, the crown and the notice, and a hammer kill comes in as a cute shot.
// What the bots do not reach in time is reported as SKIP.
// Usage: node e2e/big-head.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { defaultUrl } from "./server.mjs";

const url = process.argv[2] ?? defaultUrl;
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

// proto/game.proto: GameMode.BIG_HEAD, WeaponKind.RIFLE and HAMMER, SpecialKill.DOT_SHOT and CUTE_SHOT.
const BIG_HEAD = 3;
const RIFLE = 1;
const HAMMER = 14;
const DOT_SHOT = 1;
const CUTE_SHOT = 2;
// server/src/game/big_head.rs HEAD_SCALES, from level -5 to 3.
const HEAD_SCALES = [0.4, 0.5, 0.6, 0.7, 0.85, 1.0, 1.25, 1.55, 2.6];
const headFor = (level) => HEAD_SCALES[level + 5];

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
const skip = (name, detail = "") => console.log(`SKIP  ${name}${detail ? `  -> ${detail}` : ""}`);
const errors = [];
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(url);
await page.fill(".login input", "king");
await page.click(".login button");

// A room of bots in Big Head mode, played to 50 kills so it lasts while the bots climb.
await page.waitForSelector(".create-room");
await page.selectOption(".mode-select", { label: "왕대두모드" });
await page.click(".create-room button");
await page.waitForSelector(".actions .primary");
check("the room's header names the mode", (await page.textContent(".panel-head")).includes("왕대두모드"));
check("the host's mode selector starts at it", (await page.inputValue(".room-mode-select")) === String(BIG_HEAD));
await page.selectOption(".kill-target-select", "50");
await page.waitForFunction(() => document.querySelector(".panel-head")?.textContent.includes("50킬"));
await page.click(".actions .primary");
await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
await page.click("#ui-root .pause");
await page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });

const start = await page.evaluate(() => {
  const g = window.__game;
  const panel = document.querySelector(".level-panel");
  return {
    config: { mode: g.config.mode, min: g.config.minLevel, max: g.config.maxLevel, king: g.config.kingTime },
    players: g.lastSnapshot.players.map((p) => ({ level: p.level, head: p.headScale, king: p.kingLeft })),
    panel: !!panel && !panel.classList.contains("hidden"),
    value: document.querySelector(".level-value")?.textContent,
    pips: document.querySelectorAll(".level-pips .pip").length,
    lit: [...document.querySelectorAll(".level-pips .pip.on")].map((pip) => pip.dataset.level),
  };
});
check(
  "the match is Big Head mode with levels -5 to 3 and a 20 s reign",
  start.config.mode === BIG_HEAD && start.config.min === -5 && start.config.max === 3 && start.config.king === 20,
  JSON.stringify(start.config),
);
check(
  "everyone starts at level 0 with a normal head",
  start.players.length === 10 && start.players.every((p) => p.level === 0 && p.head === 1 && p.king === 0),
  `${start.players.length} players`,
);
check("the level panel shows level 0 on a gauge of 9 levels", start.panel && start.value === "0" && start.pips === 9 && start.lit.join() === "0", JSON.stringify(start));

// The special kills, fed through the game's kill-event handler as a snapshot's events are.
const ids = await page.evaluate(() => {
  const g = window.__game;
  const others = [...g.roster.values()].filter((entry) => entry.id !== g.myId);
  return { me: g.myId, a: others[0].id, b: others[1].id, c: others[2].id };
});
const kill = (killer, victim, headshot, weapon, special) =>
  page.evaluate(
    ({ killer, victim, headshot, weapon, special }) => {
      window.__game.onKill(killer, victim, headshot, weapon, special);
      const banner = document.querySelector(".kill-banner");
      const row = document.querySelector(".killfeed .kill-row:last-child");
      return {
        title: banner.querySelector(".kb-title")?.textContent ?? "",
        sub: banner.querySelector(".kb-sub")?.textContent ?? "",
        className: banner.className,
        emblem: banner.querySelectorAll(".kb-special").length,
        tag: row?.querySelector(".special, .headshot")?.textContent ?? "",
        tagClass: row?.querySelector(".special, .headshot")?.className ?? "",
      };
    },
    { killer, victim, headshot, weapon, special },
  );
const dot = await kill(ids.me, ids.a, true, RIFLE, DOT_SHOT);
check(
  "my dot shot: DOT SHOT on the banner with its emblem, [DOT SHOT] in the feed",
  dot.title === "DOT SHOT" && dot.className.includes("dot-shot") && dot.emblem === 1 && dot.tag === "[DOT SHOT]" && dot.tagClass.includes("dot-shot"),
  JSON.stringify(dot),
);
// The banner pops in from nothing: the picture waits for it.
await page.waitForTimeout(400);
await page.screenshot({ path: join(shots, "big-head-dot-shot.png") });
const cute = await kill(ids.me, ids.b, false, HAMMER, CUTE_SHOT);
check(
  "a cute shot next: CUTE SHOT, the run's DOUBLE KILL under it, [CUTE SHOT] in the feed",
  cute.title === "CUTE SHOT" && cute.sub === "DOUBLE KILL" && cute.className.includes("cute-shot") && cute.tag === "[CUTE SHOT]",
  JSON.stringify(cute),
);
await page.waitForTimeout(400);
await page.screenshot({ path: join(shots, "big-head-cute-shot.png") });
const others = await kill(ids.a, ids.c, true, RIFLE, CUTE_SHOT);
check("someone else's special kill is tagged in the feed only", others.tag === "[CUTE SHOT]" && others.title === "CUTE SHOT", JSON.stringify(others));
const plain = await kill(ids.a, ids.b, true, RIFLE, 0);
check("a plain headshot keeps its [HEAD SHOT] tag", plain.tag === "[HEAD SHOT]", JSON.stringify(plain));

// From here on, the special kills the server reports, read from every snapshot as it comes in.
await page.evaluate(() => {
  const g = window.__game;
  window.__specials = [];
  const original = g.onSnapshot.bind(g);
  g.onSnapshot = (snapshot) => {
    for (const { event } of snapshot.events) if (event.case === "kill" && event.value.special) window.__specials.push(event.value.special);
    original(snapshot);
  };
});

// Real play: standing still, I get killed; the bots kill each other.
const levels = () =>
  page.evaluate(() => {
    const g = window.__game;
    const me = g.lastSnapshot.players.find((p) => p.id === g.myId);
    const remotes = [...g.remotes.values()].filter((r) => r.state).map((r) => ({ id: r.entry.id, level: r.state.level, head: r.state.headScale, shown: r.character.headScale }));
    return { me: { level: me.level, head: me.headScale, king: me.kingLeft }, panel: document.querySelector(".level-value")?.textContent, remotes };
  });
await page
  .waitForFunction(() => window.__game.lastSnapshot.players.some((p) => p.level !== 0), null, { timeout: 60000, polling: 250 })
  .catch(() => undefined);
const moved = await levels();
const changed = moved.remotes.filter((r) => r.level !== 0).length + (moved.me.level !== 0 ? 1 : 0);
check("kills and deaths move levels", changed > 0, `${changed} players off level 0`);
check(
  "every head is its level's size, on the server and on the model",
  [moved.me, ...moved.remotes].every((p) => Math.abs(p.head - headFor(p.level)) < 1e-6) && moved.remotes.every((r) => r.shown === r.head),
  moved.remotes.map((r) => `${r.level}:${r.head}`).join(" "),
);
check("the level panel shows my level", moved.panel === String(moved.me.level), `panel ${moved.panel}, level ${moved.me.level}`);
await page.screenshot({ path: join(shots, "big-head-levels.png") });

// A King, when a bot (or I) gets to level 3 in time. Read a moment after the crowning, once the
// frames since have drawn it.
const kingId = await page
  .waitForFunction(() => window.__game.lastSnapshot.players.find((p) => p.kingLeft > 0)?.id ?? null, null, { timeout: 150000, polling: 100 })
  .then((handle) => handle.jsonValue())
  .catch(() => null);
await page.waitForTimeout(kingId === null ? 0 : 600);
const reign =
  kingId === null
    ? null
    : await page.evaluate((id) => {
        const g = window.__game;
        const king = g.lastSnapshot.players.find((p) => p.id === id);
        const remote = g.remotes.get(id);
        return {
          id,
          mine: id === g.myId,
          level: king.level,
          head: king.headScale,
          weapon: king.weapon,
          left: king.kingLeft,
          crown: remote ? remote.character.crown.visible : null,
          slot: document.querySelector(".slot-chip.king")?.textContent ?? "",
          notice: document.querySelector(".big-head-notice")?.textContent ?? "",
        };
      }, kingId);
if (!reign) {
  skip("a King with the hammer, the crown and the notice", "nobody reached level 3 within 150 s");
} else {
  check(
    "the King is at level 3 with the biggest head and the hammer, under 20 s left",
    reign.level === 3 && Math.abs(reign.head - headFor(3)) < 1e-6 && reign.weapon === HAMMER && reign.left > 0 && reign.left <= 20,
    JSON.stringify(reign),
  );
  check("everyone is told who was crowned", reign.notice.includes("왕대두"), reign.notice);
  if (reign.mine) check("my slot strip shows only the hammer", reign.slot.includes("뿅망치"), reign.slot);
  else check("the King's model wears the crown", reign.crown === true);
  // Turn to face the King for the picture.
  await page.evaluate((id) => {
    const g = window.__game;
    const remote = g.remotes.get(id);
    if (!remote) return;
    g.yaw = Math.atan2(-(remote.shownX - g.pos.x), -(remote.shownZ - g.pos.z));
    g.pitch = 0;
  }, reign.id);
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(shots, "big-head-king.png") });
  const cuteShot = await page
    .waitForFunction((special) => window.__specials.includes(special), CUTE_SHOT, { timeout: 30000, polling: 200 })
    .then(() => true)
    .catch(() => false);
  if (cuteShot) check("a King's hammer kill comes in from the server as a cute shot", true);
  else skip("a King's hammer kill comes in from the server as a cute shot", "no hammer kill within 30 s of the crowning");
}

check("no console errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
console.log(failed ? `${failed} check(s) failed` : "all checks passed");
process.exit(failed ? 1 : 0);
