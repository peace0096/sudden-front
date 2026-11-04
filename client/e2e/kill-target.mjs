// The host chooses the kill target and which empty seats hold bots; a guest sees
// both and can't change them.
// Usage: node e2e/kill-target.mjs [url] [screenshot-dir]
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
const errors = [];
let failed = 0;
const check = (name, ok, detail = "") => {
  if (!ok) failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  -> ${detail}` : ""}`);
};
const open = async (name) => {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(url);
  await page.fill(".login input", name);
  await page.click(".login button");
  await page.waitForSelector(".create-room");
  return page;
};

const seat = (page, team, n) => page.locator(`.team.${team} .slot`).nth(n);
const seatIs = (page, team, n, kind) => seat(page, team, n).evaluate((el, k) => el.classList.contains(k), kind);
const botCount = (page) => page.locator(".slot.bot").count();
const header = (page) => page.textContent(".panel-head");

const host = await open("host");
const guest = await open("guest");
await host.fill(".room-name-input", "목표 방");
await host.click(".create-room button");
await host.waitForSelector(".kill-target-select");
check("the target starts at 30", (await host.inputValue(".kill-target-select")) === "30");
// The create form's checkbox is on, so every seat but the host's starts as a bot.
check("every other seat holds a bot", (await botCount(host)) === 9 && (await header(host)).includes("봇 9"), await header(host));

// The guest waits in the lobby with a half-filled create form, which room changes must not redraw.
await guest.fill(".room-name-input", "손님의 방");
await guest.evaluate(() => {
  document.querySelector(".create-room").dataset.mark = "kept";
});

await host.selectOption(".kill-target-select", "10");
await guest.waitForFunction(() => document.querySelector(".room-list")?.textContent.includes("10킬"));
check("the room list shows the new target", true);
check("the room list shows the bots", (await guest.textContent(".room-list")).includes("봇 9"));
check(
  "a list update leaves the create form, its text and its focus alone",
  await guest.evaluate(() => {
    const input = document.querySelector(".room-name-input");
    return document.querySelector(".create-room").dataset.mark === "kept" && input.value === "손님의 방" && document.activeElement === input;
  }),
);

await guest.click(".room-row .btn");
// The lobby has a .panel-head too, so wait for the room itself.
await guest.waitForSelector(".room .teams");
check("a guest sees the target", (await guest.textContent(".panel-head")).includes("10킬 선취 승리"));
check("a guest has no selector", (await guest.$(".kill-target-select")) === null);

await host.selectOption(".kill-target-select", "20");
await guest.waitForFunction(() => document.querySelector(".panel-head").textContent.includes("20킬 선취 승리"));
check("a change reaches the guest", true);

// The guest joined on the team with fewer humans, in the first seat, which a bot gave up.
await host.waitForFunction(() => document.querySelector(".team.blue")?.textContent.includes("guest"));
check("the guest took blue's first seat", (await seatIs(guest, "blue", 0, "me")) && (await seat(guest, "blue", 0).textContent()).includes("guest"));
check("a bot gave way for the guest", (await botCount(guest)) === 8 && (await header(guest)).includes("봇 8"), await header(guest));
check("the host's open and bot seats are buttons", (await host.locator("button.slot").count()) === 8);
check("a guest has no seat buttons", (await guest.locator("button.slot").count()) === 0);

// Each change is one seat, and the guest sees it. A button's name holds its seat and the two
// texts it shows: the seat's state and what a click does.
const press = async (name, bots) => {
  await host.click(`[aria-label="${name}"]`);
  await guest.waitForFunction((n) => document.querySelectorAll(".slot.bot").length === n, bots);
};
await press("RED 2번 자리 BOT 봇 ✕", 7);
check("the guest sees the bot go and its seat open", (await seat(guest, "red", 1).textContent()) === "빈 자리" && (await header(guest)).includes("봇 7"), await header(guest));
await press("RED 2번 자리 빈 자리 + 봇", 8);
check("the guest sees the bot come back", (await seatIs(guest, "red", 1, "bot")) && (await header(guest)).includes("봇 8"), await header(guest));

// Down to one bot, on red's last seat.
let left = 8;
for (const seatName of ["RED 2번", "RED 3번", "RED 4번", "BLUE 2번", "BLUE 3번", "BLUE 4번", "BLUE 5번"]) {
  await press(`${seatName} 자리 BOT 봇 ✕`, --left);
}
check("one bot is left", (await botCount(guest)) === 1 && (await seatIs(guest, "red", 4, "bot")) && (await header(guest)).includes("봇 1"), await header(guest));
// The guest's screen is up to date; wait for the host's own before taking its picture.
await host.waitForFunction(() => document.querySelectorAll(".slot.bot").length === 1);
await host.screenshot({ path: join(shots, "kill-target-room.png") });

await host.click(".actions .primary");
await host.waitForFunction(() => window.__game?.config);
const started = await host.evaluate(() => ({
  config: window.__game.config.killsToWin,
  bar: document.querySelector(".score.target").textContent,
}));
check("the match uses it", started.config === 20 && started.bar === "020", JSON.stringify(started));
const roster = await host.evaluate(() => {
  const g = window.__game;
  const entries = [...g.roster.values()];
  return {
    humans: entries.filter((e) => !e.isBot).map((e) => e.name).sort(),
    botsOnMyTeam: entries.filter((e) => e.isBot).map((e) => e.team === g.myTeam),
  };
});
check("the match holds the two players and the one bot, on the host's team", JSON.stringify(roster) === JSON.stringify({ humans: ["guest", "host"], botsOnMyTeam: [true] }), JSON.stringify(roster));

// Blue is only the guest. When the guest leaves, blue has nobody, and red wins by forfeit.
await guest.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 60000 });
await guest.evaluate(() => window.__game.connection.send({ case: "leaveRoom", value: {} }));
await host.waitForFunction(() => window.__game.matchOver, null, { timeout: 60000 });
const verdict = await host.evaluate(() => ({
  title: document.querySelector(".board-title")?.textContent,
  sub: document.querySelector(".board-sub")?.textContent,
}));
check("red wins by forfeit when blue has nobody left", verdict.title === "VICTORY" && verdict.sub.includes("RED TEAM 승리"), JSON.stringify(verdict));
await host.waitForSelector(".room .teams", { timeout: 60000 });
check(
  "the room is back in the waiting room: blue's seat open, red's bot still there",
  (await seat(host, "blue", 0).textContent()).includes("빈 자리") && (await botCount(host)) === 1 && (await header(host)).includes("봇 1"),
  await header(host),
);

check("no page errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
process.exit(failed ? 1 : 0);
