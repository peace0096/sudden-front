// The kill banner at the top of the screen: KILL for a kill of mine, DOUBLE KILL, TRIPLE KILL...
// for each one within 4 seconds of the last, with a cartridge for each; HEADSHOT with a holed skull
// for a first kill to the head, and a HEADSHOT tag on a later one; under it, the name of who fell in
// their team's colour; nothing for kills of others; the
// run ends with my death or a 4-second gap, and the banner goes away by itself. The kills are fed
// through the game's own kill-event handler (Game.onKill), as a snapshot's events are.
// Usage: node e2e/kill-banner.mjs [url] [screenshot-dir]
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { defaultUrl } from "./server.mjs";

const url = process.argv[2] ?? defaultUrl;
const shots = process.argv[3] ?? "e2e-shots";
mkdirSync(shots, { recursive: true });

// WeaponKind.RIFLE (proto/game.proto).
const RIFLE = 1;

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

const page = await open("ace");
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
await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 30000 });
await page.click("#ui-root .pause");
await page.waitForFunction(() => window.__game.locked, null, { timeout: 5000 });

const ids = await page.evaluate(() => {
  const g = window.__game;
  return { me: g.myId, rival: [...g.roster.values()].find((entry) => entry.name === "rival").id };
});
/** Feeds one kill event through the game, and reads the banner it leaves. */
const kill = (killer, victim, headshot) =>
  page.evaluate(
    ({ killer, victim, headshot, weapon }) => {
      window.__game.onKill(killer, victim, headshot, weapon);
      const banner = document.querySelector(".kill-banner");
      return {
        shown: !banner.classList.contains("hidden"),
        title: banner.querySelector(".kb-title")?.textContent ?? "",
        sub: banner.querySelector(".kb-sub")?.textContent ?? "",
        bullets: banner.querySelectorAll(".kb-bullet").length,
        skull: banner.querySelectorAll(".kb-skull").length,
        className: banner.className,
        victim: banner.querySelector(".kb-victim-name")?.textContent ?? "",
        victimClass: banner.querySelector(".kb-victim-name")?.className ?? "",
        label: banner.querySelector(".kb-victim-label")?.textContent ?? "",
      };
    },
    { killer, victim, headshot, weapon: RIFLE },
  );
const describe = (b) => `${b.title} · ${b.bullets} cartridges${b.skull ? " + skull" : ""}${b.sub ? ` · ${b.sub}` : ""} · ${b.victim} ${b.label}`;

let banner = await kill(ids.me, ids.rival, false);
check("a kill of mine shows KILL with one cartridge", banner.shown && banner.title === "KILL" && banner.bullets === 1 && !banner.skull && !banner.sub, describe(banner));
// The rival sits on blue, the host on red.
check("under it, the name of who fell in their team's colour", banner.victim === "rival" && banner.victimClass.includes("blue") && banner.label === "처치", `${banner.victim} (${banner.victimClass}) ${banner.label}`);
await page.waitForTimeout(700);
await page.screenshot({ path: join(shots, "kill-banner-1-kill.png") });

banner = await kill(ids.me, ids.rival, false);
check("the next within 4 s is DOUBLE KILL with two", banner.title === "DOUBLE KILL" && banner.bullets === 2, describe(banner));
await page.waitForTimeout(700);
await page.screenshot({ path: join(shots, "kill-banner-2-double.png") });

banner = await kill(ids.me, ids.rival, true);
check("a headshot in the run keeps its count, with a skull and a HEADSHOT tag", banner.title === "TRIPLE KILL" && banner.bullets === 3 && banner.skull === 1 && banner.sub === "HEADSHOT", describe(banner));
await page.waitForTimeout(700);
await page.screenshot({ path: join(shots, "kill-banner-3-triple-headshot.png") });

const titles = [];
for (let i = 0; i < 3; i++) titles.push((await kill(ids.me, ids.rival, false)).title);
check("then QUADRA, PENTA and HEXA KILL", JSON.stringify(titles) === JSON.stringify(["QUADRA KILL", "PENTA KILL", "HEXA KILL"]), titles.join(", "));
await page.waitForTimeout(700);
await page.screenshot({ path: join(shots, "kill-banner-6-hexa.png") });

// Nobody else's kill shows: not the rival's on a third party, nor one of me.
banner = await kill(ids.rival, ids.me, false);
check("my death shows no banner of its own", banner.title === "HEXA KILL", describe(banner));
banner = await kill(ids.me, ids.rival, true);
check("after my death a headshot starts over as HEADSHOT, a skull alone", banner.title === "HEADSHOT" && banner.skull === 1 && banner.bullets === 0 && banner.className.includes("headshot-kill"), describe(banner));
check("the HEADSHOT banner names who fell too", banner.victim === "rival", describe(banner));
await page.waitForTimeout(700);
await page.screenshot({ path: join(shots, "kill-banner-headshot.png") });
banner = await kill(ids.me, ids.rival, false);
check("and the next is DOUBLE KILL", banner.title === "DOUBLE KILL" && banner.bullets === 2 && !banner.sub, describe(banner));

// More than 4 s after the last, a kill starts a new run.
await page.evaluate(() => {
  window.__game.lastKillAt -= 4.5;
});
banner = await kill(ids.me, ids.rival, false);
check("a kill more than 4 s after the last is KILL again", banner.title === "KILL" && banner.bullets === 1, describe(banner));

// It goes by itself, once its animation (2.4 s) has played.
await page.waitForTimeout(3500);
check("the banner goes away by itself", await page.evaluate(() => document.querySelector(".kill-banner").classList.contains("hidden")));

check("no page or console errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
process.exit(failed ? 1 : 0);
