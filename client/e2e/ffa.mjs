// A free-for-all room from the lobby to a second match. The room list and the waiting room call it
// 개인전, its ten seats are one block with no team to change to, and the host's bot seats reach the
// guest. A match on the warehouse map with two bots runs to a 10-kill target: nobody is on a team,
// every other player is an enemy, the score bar counts kills per player, the Tab board is one
// ranked list, and only the player who reached the target sees VICTORY. Back in the room, a second
// match with no bots ends when the guest leaves, and the host wins by forfeit.
// Usage: node e2e/ffa.mjs [url] [screenshot-dir]
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

// MatchType.FREE_FOR_ALL and MapId.WAREHOUSE.
const FREE_FOR_ALL = 2;
const WAREHOUSE = 2;
const TARGET = 10;
const ROOM = "FFA 방";

const seat = (page, n) => page.locator(".team.ffa .slot").nth(n);
const seatIs = (page, n, kind) => seat(page, n).evaluate((el, k) => el.classList.contains(k), kind);
const botCount = (page) => page.locator(".slot.bot").count();
const header = (page) => page.textContent(".panel-head");
const buttons = (page) => page.$$eval(".actions button", (nodes) => nodes.map((n) => n.textContent));
// Tab through the window, as the game hears it: a focused lobby button would keep the key to itself.
const tab = (page, type) => page.evaluate((t) => window.dispatchEvent(new KeyboardEvent(t, { code: "Tab", cancelable: true })), type);
// Keeps the MatchEnd the server sent, to compare with what the page shows.
const recordEnd = (page) =>
  page.evaluate(() => {
    const g = window.__game;
    const onMatchEnd = g.onMatchEnd.bind(g);
    window.__end = null;
    g.onMatchEnd = (end) => {
      window.__end = {
        winner: end.winner,
        winnerId: end.winnerId,
        red: end.redScore,
        blue: end.blueScore,
        scoreboard: end.scoreboard.map((e) => ({ id: e.id, name: e.name, team: e.team, kills: e.kills })),
      };
      onMatchEnd(end);
    };
  });
const ended = (page) =>
  page.evaluate(() => ({
    myId: window.__game.myId,
    end: window.__end,
    title: document.querySelector(".board-title")?.textContent,
    sub: document.querySelector(".board-sub")?.textContent,
    lists: document.querySelectorAll(".board-team").length,
  }));

const host = await open("host");
const guest = await open("guest");

// ------------------------------------------------------------ waiting room

// No bots to begin with: the host seats them by hand below.
await host.selectOption(".match-select", String(FREE_FOR_ALL));
await host.selectOption(".map-select", String(WAREHOUSE));
await host.uncheck(".bots-check");
await host.fill(".room-name-input", ROOM);
await host.click(".create-room button");
await host.waitForSelector(".room .teams");

const room = await host.evaluate(() => ({
  blocks: document.querySelectorAll(".room .teams .team").length,
  ffa: document.querySelectorAll(".room .teams .team.ffa").length,
  seats: document.querySelectorAll(".team.ffa .slot").length,
  labels: [...document.querySelectorAll(".team.ffa [aria-label]")].map((n) => n.getAttribute("aria-label")),
  addressed: document.querySelectorAll('.team.ffa [data-seat^="ffa:"]').length,
  text: document.querySelector(".team.ffa")?.textContent ?? "",
}));
check("the room is one free-for-all block of ten seats", room.blocks === 1 && room.ffa === 1 && room.seats === 10, JSON.stringify(room));
check("the header calls it 개인전", (await header(host)).includes("개인전 · "), await header(host));
check("the target starts at 20", (await host.inputValue(".kill-target-select")) === "20" && (await header(host)).includes("20킬 선취 승리"), await header(host));
check("there is no team to change to", !(await buttons(host)).includes("팀 변경"), (await buttons(host)).join("|"));
check("the host sits in the first seat", await seatIs(host, 0, "me"));
check(
  "the nine open seats are buttons named by seat number alone",
  room.labels.length === 9 &&
    room.addressed === 9 &&
    room.labels.every((label, i) => label === `${i + 2}번 자리 빈 자리 + 봇`) &&
    !room.text.includes("undefined"),
  room.labels.join(" | "),
);

// One player is no match.
await host.click(".actions .primary");
const refused = await host
  .waitForSelector(".toast", { timeout: 5000 })
  .then((node) => node.textContent())
  .catch(() => "");
check("a free-for-all match needs two players", refused.includes("2명 이상") && !(await host.evaluate(() => !!window.__game)), refused);

const row = guest.locator(".room-row", { hasText: ROOM });
await row.waitFor();
const listed = await row.locator(".room-info .muted").textContent();
check("the room list calls it 개인전, at 20 kills", listed.startsWith("개인전 · ") && listed.includes("20킬"), listed);

await row.locator(".btn").click();
await guest.waitForSelector(".room .teams");
await host.waitForFunction(() => document.querySelector(".team.ffa")?.textContent.includes("guest"));
check("the guest took the first open seat", (await seatIs(guest, 1, "me")) && (await seat(guest, 1).textContent()).includes("guest"));
check("the guest's header calls it 개인전", (await header(guest)).includes("개인전 · "), await header(guest));
check(
  "a guest has no seat buttons and no team to change to",
  (await guest.locator("button.slot").count()) === 0 && !(await buttons(guest)).includes("팀 변경"),
  (await buttons(guest)).join("|"),
);

await host.selectOption(".kill-target-select", String(TARGET));
await guest.waitForFunction((kills) => document.querySelector(".panel-head").textContent.includes(`${kills}킬 선취 승리`), TARGET);
check("the guest sees the 10-kill target", true);

// Each click is one seat, and the guest sees it.
const press = async (name, bots) => {
  await host.click(`[aria-label="${name}"]`);
  await guest.waitForFunction((n) => document.querySelectorAll(".slot.bot").length === n, bots);
};
await press("3번 자리 빈 자리 + 봇", 1);
await press("4번 자리 빈 자리 + 봇", 2);
await press("5번 자리 빈 자리 + 봇", 3);
check("the guest sees the host's bots come", (await seatIs(guest, 4, "bot")) && (await header(guest)).includes("봇 3"), await header(guest));
await press("5번 자리 BOT 봇 ✕", 2);
check(
  "and one go again",
  (await seatIs(guest, 4, "open")) && (await seatIs(guest, 2, "bot")) && (await seatIs(guest, 3, "bot")) && (await header(guest)).includes("봇 2"),
  await header(guest),
);

await host.waitForFunction(() => document.querySelectorAll(".slot.bot").length === 2);
const actions = await host.locator(".room .actions").boundingBox();
check("the seats leave the room's buttons in view at 1280x720", !!actions && actions.y + actions.height <= 720, JSON.stringify(actions));
await host.screenshot({ path: join(shots, "ffa-room.png") });

// ------------------------------------------------------------ first match: two bots, to the target

// The guest readies; the host's start waits for it.
await guest.click(".actions .ready-btn");
await host.click(".actions .primary");
for (const page of [host, guest]) {
  await page.waitForFunction(() => window.__game?.hasFirstSnapshot, null, { timeout: 60000 });
  await recordEnd(page);
}

const match = await host.evaluate(() => {
  const g = window.__game;
  const roster = [...g.roster.values()];
  return {
    ffa: g.ffa,
    matchType: g.config.matchType,
    target: g.config.killsToWin,
    myTeam: g.myTeam,
    teams: roster.map((e) => e.team),
    humans: roster.filter((e) => !e.isBot).map((e) => e.name).sort(),
    bots: roster.filter((e) => e.isBot).length,
    remotes: g.remotes.size,
    allies: [...g.remotes.values()].filter((r) => g.isAlly(r.entry)).length,
    bar: document.querySelector(".score.target")?.textContent,
    sides: [".score.me", ".score.rival", ".score.red, .score.blue"].map((s) => document.querySelectorAll(s).length),
  };
});
check("the match is a free-for-all", match.ffa === true && match.matchType === FREE_FOR_ALL, JSON.stringify(match));
check("nobody is on a team", match.myTeam === 0 && match.teams.length === 4 && match.teams.every((team) => team === 0), JSON.stringify(match.teams));
check("the match holds the two players and the two bots", JSON.stringify(match.humans) === JSON.stringify(["guest", "host"]) && match.bots === 2, JSON.stringify(match));
check("every other player is an enemy", match.remotes === 3 && match.allies === 0, JSON.stringify(match));
// Nobody wears a team's colour: the first-person arms and every soldier are the neutral uniform.
const NEUTRAL = "characters/swat-neutral.glb";
const looks = await host.evaluate(() => {
  const g = window.__game;
  return { arms: g.viewModel.arms?.file ?? null, soldiers: [...g.remotes.values()].map((r) => r.character.made?.file ?? null) };
});
check(
  "everyone wears the neutral uniform: the arms in view and every other soldier",
  looks.arms === NEUTRAL && looks.soldiers.length === 3 && looks.soldiers.every((file) => file === NEUTRAL),
  JSON.stringify(looks),
);
check(
  "the score bar shows my kills and a rival's around the 10-kill target",
  match.target === TARGET && match.bar === "010" && JSON.stringify(match.sides) === "[1,1,0]",
  JSON.stringify(match),
);

await tab(host, "keydown");
const board = await host.evaluate(() => ({
  shown: !document.querySelector(".board").classList.contains("hidden"),
  title: document.querySelector(".board-title")?.textContent,
  lists: document.querySelectorAll(".board-team").length,
  ffa: document.querySelectorAll(".board-team.ffa").length,
  rows: document.querySelectorAll(".board-team.ffa .board-row:not(.head)").length,
  ranks: [...document.querySelectorAll(".board-team.ffa .board-row:not(.head) .r")].map((n) => n.textContent),
  me: [...document.querySelectorAll(".board-row.me .n")].map((n) => n.textContent),
}));
check(
  "Tab shows one ranked list of all four",
  board.shown && board.title === "FREE FOR ALL" && board.lists === 1 && board.ffa === 1 && board.rows === 4 && board.ranks[0] === "1" && JSON.stringify(board.me) === '["host"]',
  JSON.stringify(board),
);
await host.screenshot({ path: join(shots, "ffa-board.png") });
await tab(host, "keyup");

// With a few kills in, the bar's sides follow the snapshot: mine, and the most anyone else has.
await host.waitForFunction(() => window.__game.matchOver || window.__game.lastSnapshot.players.some((p) => p.kills >= 2), null, {
  timeout: 300000,
  polling: 250,
});
const bar = await host.evaluate(() => {
  const g = window.__game;
  const s = g.lastSnapshot;
  const pad = (n) => String(n).padStart(3, "0");
  return {
    shown: [document.querySelector(".score.me").textContent, document.querySelector(".score.rival").textContent],
    expected: [pad(s.players.find((p) => p.id === g.myId)?.kills ?? 0), pad(Math.max(0, ...s.players.filter((p) => p.id !== g.myId).map((p) => p.kills)))],
    teamScores: [s.redScore, s.blueScore],
  };
});
check(
  "the bar shows my kills and the best of the rest, and the team scores stay 0",
  JSON.stringify(bar.shown) === JSON.stringify(bar.expected) && bar.teamScores.every((score) => score === 0),
  JSON.stringify(bar),
);

// The feed has one colour for everyone; my name stands out once a kill involves me.
const feed = await host
  .waitForFunction(
    () => {
      const mine = document.querySelector(".killfeed .name.me");
      if (!mine && !window.__game.matchOver) return null;
      return {
        mine: mine?.textContent ?? null,
        neutral: document.querySelectorAll(".killfeed .name.ffa").length,
        team: document.querySelectorAll(".killfeed .name.red, .killfeed .name.blue").length,
      };
    },
    null,
    { timeout: 600000, polling: 100 },
  )
  .then((handle) => handle.jsonValue());
if (feed.mine === null) console.log("SKIP  the kill feed picks out my name: the match ended before a kill involved the host");
else check("the kill feed has one colour for all and picks out my name", feed.mine === "host" && feed.neutral >= 2 && feed.team === 0, JSON.stringify(feed));

for (const page of [host, guest]) {
  await page.waitForFunction(() => window.__game.matchOver && window.__end, null, { timeout: 600000, polling: 500 });
}
await host.screenshot({ path: join(shots, "ffa-results.png") });
const results = { host: await ended(host), guest: await ended(guest) };
const end = results.host.end;
const winner = end.scoreboard.find((e) => e.id === end.winnerId);
const atTarget = end.scoreboard.filter((e) => e.kills >= TARGET);
check(
  "one player won, and only the winner has 10 kills",
  !!winner && winner.kills === TARGET && atTarget.length === 1 && atTarget[0].id === end.winnerId && results.guest.end.winnerId === end.winnerId,
  JSON.stringify(end.scoreboard),
);
check("the end names no team and no team score", end.winner === 0 && end.red === 0 && end.blue === 0 && end.scoreboard.every((e) => e.team === 0), JSON.stringify(end));
for (const [name, seen] of Object.entries(results)) {
  const expected = end.winnerId === seen.myId ? "VICTORY" : "DEFEAT";
  check(
    `${name} sees ${expected}, the winner's name and one list`,
    seen.title === expected && !!winner && seen.sub.includes(`${winner.name} 승리 · ${TARGET}킬 달성`) && seen.lists === 1,
    JSON.stringify({ title: seen.title, sub: seen.sub, myId: seen.myId, winnerId: end.winnerId, lists: seen.lists }),
  );
}

// ------------------------------------------------------------ second match: two players, a forfeit

for (const page of [host, guest]) await page.waitForSelector(".room .teams", { timeout: 60000 });
check(
  "both are back in the free-for-all room, in their seats, with the bots",
  (await host.locator(".team.ffa").count()) === 1 && (await seatIs(host, 0, "me")) && (await seatIs(guest, 1, "me")) && (await botCount(host)) === 2,
  await header(host),
);

await press("3번 자리 BOT 봇 ✕", 1);
await press("4번 자리 BOT 봇 ✕", 0);
// window.__game keeps pointing at the finished Game until the next match start replaces it.
const finished = { host: await host.evaluateHandle(() => window.__game), guest: await guest.evaluateHandle(() => window.__game) };
// Readiness was spent on the first match: the guest readies again.
await guest.click(".actions .ready-btn");
await host.click(".actions .primary");
for (const [name, page] of [["host", host], ["guest", guest]]) {
  await page.waitForFunction((old) => window.__game !== old && window.__game?.hasFirstSnapshot, finished[name], { timeout: 60000 });
}
check("a second match starts from the same room", true);
const duel = await host.evaluate(() => ({
  ffa: window.__game.ffa,
  roster: [...window.__game.roster.values()].map((e) => `${e.name}:${e.team}:${e.isBot}`).sort(),
}));
check("it is the two players alone", duel.ffa === true && JSON.stringify(duel.roster) === JSON.stringify(["guest:0:false", "host:0:false"]), JSON.stringify(duel));

await recordEnd(host);
await guest.evaluate(() => window.__game.connection.send({ case: "leaveRoom", value: {} }));
await host.waitForFunction(() => window.__game.matchOver && window.__end, null, { timeout: 60000 });
const forfeit = await ended(host);
check(
  "the host wins by forfeit when the guest leaves, short of the target",
  forfeit.end.winnerId === forfeit.myId && forfeit.end.winner === 0 && forfeit.title === "VICTORY" && forfeit.sub.includes("host 승리") && !forfeit.sub.includes("킬 달성"),
  JSON.stringify({ title: forfeit.title, sub: forfeit.sub, myId: forfeit.myId, winnerId: forfeit.end.winnerId }),
);
await host.waitForSelector(".room .teams", { timeout: 60000 });
check("the room is back in the waiting room, the guest's seat open", (await seatIs(host, 1, "open")) && (await botCount(host)) === 0, await header(host));

check("no page errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
process.exit(failed ? 1 : 0);
