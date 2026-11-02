// The host chooses the kill target; a guest sees it and can't change it.
// Usage: node e2e/kill-target.mjs [url] [screenshot-dir]
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

const host = await open("host");
const guest = await open("guest");
await host.fill(".room-name-input", "목표 방");
await host.click(".create-room button");
await host.waitForSelector(".kill-target-select");
check("the target starts at 30", (await host.inputValue(".kill-target-select")) === "30");

await host.selectOption(".kill-target-select", "10");
await guest.waitForFunction(() => document.querySelector(".room-list")?.textContent.includes("10킬"));
check("the room list shows the new target", true);

await guest.click(".room-row .btn");
await guest.waitForSelector(".panel-head");
check("a guest sees the target", (await guest.textContent(".panel-head")).includes("10킬 선취 승리"));
check("a guest has no selector", (await guest.$(".kill-target-select")) === null);

await host.selectOption(".kill-target-select", "20");
await guest.waitForFunction(() => document.querySelector(".panel-head").textContent.includes("20킬 선취 승리"));
check("a change reaches the guest", true);
await host.screenshot({ path: join(shots, "kill-target-room.png") });

await host.click(".actions .primary");
await host.waitForFunction(() => window.__game?.config);
const started = await host.evaluate(() => ({
  config: window.__game.config.killsToWin,
  bar: document.querySelector(".score.target").textContent,
}));
check("the match uses it", started.config === 20 && started.bar === "020", JSON.stringify(started));

check("no page errors", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
process.exit(failed ? 1 : 0);
