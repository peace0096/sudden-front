// Photographs the ?art&candidates gallery section by section, with the shading, tone mapping,
// team colour and motion toggles that checkpoint A compares, and writes the PNGs and an index.html
// contact sheet with captions to client/e2e-shots/art-gallery/ (gitignored).
//
// With --final it photographs the plain ?art page instead (the built runtime art, phase 2 on) into
// client/e2e-shots/art-final/, with the character clips it compares.
//
// Usage: node gallery-shots.mjs [--final] [--swiftshader] [--only <section,...>] [url]
//   url            a dev server already running, such as http://localhost:5190/ (default: start
//                  Vite from client/ on a free port from 5190 and stop it afterwards)
//   --swiftshader  render in software as the e2e tests do, instead of on the GPU
//   --only         just these sections (characters, arms, guns, map, surfaces, effects), into a
//                  scratch folder beside the contact sheet, which is left as it was
// Needs Google Chrome: playwright-core drives it headless (channel "chrome"). The candidates must
// be staged first (stage-candidates.mjs); a production build drops them, so this uses the dev server.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import sharp from "sharp";

const here = fileURLToPath(new URL(".", import.meta.url));
const client = join(here, "..", "..", "client");
const shotsRoot = join(client, "e2e-shots");
const args = process.argv.slice(2);
const swiftshader = args.includes("--swiftshader");
const final = args.includes("--final");
const onlyAt = args.indexOf("--only");
const only = onlyAt >= 0 ? args[onlyAt + 1].split(",") : null;
const given = args.find((arg, i) => !arg.startsWith("--") && (onlyAt < 0 || i !== onlyAt + 1));

/** Each picture: the section, the toggles set before it and what it shows. */
const SHOTS = [
  { file: "01-characters.png", section: "characters", options: {}, caption: "캐릭터 기본: Lambert·ACES·원본 색·대기 클립. 머리 꼭대기를 1.76 m에 맞추고 서버 히트박스(빨강 머리 1.50–1.76 m, 주황 몸통, 파랑 다리)를 겹쳤습니다. 초록 선은 머리 관절 높이입니다." },
  { file: "02-characters-red.png", section: "characters", options: { team: "red" }, caption: "RED 팀 색: 각 캐릭터의 제복 재질에만 팀 색을 70% 섞었습니다." },
  { file: "03-characters-blue.png", section: "characters", options: { team: "blue" }, caption: "BLUE 팀 색." },
  { file: "04-characters-crouch.png", section: "characters", options: { motion: "crouch" }, caption: "앉기: 히트박스가 서버처럼 0.5 m 내려갑니다. 앉기 클립이 있는 것은 UAL1 마네킹뿐이고, 나머지는 대기 자세로 남아 있어 차이가 보입니다." },
  { file: "05-characters-run.png", section: "characters", options: { motion: "run" }, caption: "달리기 클립." },
  { file: "06-characters-toon-neutral.png", section: "characters", options: { shading: "toon", tone: "neutral" }, caption: "Toon 3단 셰이딩 + Neutral 톤 매핑 비교." },
  { file: "07-arms.png", section: "arms", options: {}, caption: "1인칭 팔(3인칭 메시 재사용 방식): 머리 뼈를 0배로 줄이고 게임 눈높이 1.42 m·시야각 72°에서 본 모습. 총은 아직 붙이지 않았습니다." },
  { file: "08-guns.png", section: "guns", options: {}, caption: "총기 측면, 모두 같은 축척(가로 1.32 m, 격자 10 cm). 역할별 목표 길이로 맞췄고, Animated Guns(FBX 변환)는 Reload 클립을 재생합니다." },
  { file: "09-guns-toon.png", section: "guns", options: { shading: "toon" }, caption: "총기 Toon 3단 셰이딩." },
  { file: "10-map.png", section: "map", options: {}, caption: "맵 키트: 주황 선은 서버 충돌 상자(컨테이너 6.0×2.6×2.4, 상자 1.2³, 드럼통 0.9×1.1×0.9 등). 모델은 균일 배율 하나로 맞추고 축마다 남는 차이를 적었습니다." },
  { file: "11-map-toon-neutral.png", section: "map", options: { shading: "toon", tone: "neutral" }, caption: "맵 키트 Toon + Neutral." },
  { file: "12-surfaces.png", section: "surfaces", options: {}, caption: "표면 텍스처: 앞 3개는 지금의 코드 텍스처, 뒤는 rubberduck 손그림 돌벽돌 후보입니다." },
  { file: "13-effects.png", section: "effects", options: {}, caption: "이펙트: 총구 화염·예광탄·연기·착탄 불꽃 반복 데모와 선택한 Kenney 스프라이트." },
];
const DEFAULTS = { shading: "lambert", tone: "aces", team: "none", motion: "idle" };
/** The plain ?art page: each picture's section, the character clip picked before it, and what it shows. */
const FINAL_SHOTS = [
  { file: "01-manifest.png", section: "manifest", caption: "매니페스트 요약: 12개 무기, 캐릭터 2벌, 1인칭 팔(캐릭터 재사용), 투사체, 맵 키트, 이펙트 아틀라스와 파일 목록." },
  { file: "02-budgets.png", section: "budgets", caption: "예산과 지금 파일 크기 합계, 예산을 넘는 파일." },
  { file: "03-characters-idle.png", section: "characters", clip: "idle", caption: "RED·BLUE 캐릭터(SWAT 감량본) 정면·측면·3/4, 서버 히트박스 와이어프레임, idle 클립. rest pose의 머리 뼈 높이와 삼각형 수." },
  { file: "04-characters-holdRifle.png", section: "characters", clip: "holdRifle", caption: "holdRifle 클립(Idle_Gun_Pointing): 3·4단계에서 총을 오른손 소켓에 붙일 자세." },
  { file: "05-characters-run.png", section: "characters", clip: "run", caption: "run 클립." },
  { file: "06-characters-death.png", section: "characters", clip: "death", caption: "death 클립." },
  { file: "07-weapons.png", section: "weapons", caption: "12개 무기와 미사일 탄: 측면, 같은 축척. 초록 십자는 손잡이(원점), 빨강 십자는 muzzle 노드. 총구는 −Z." },
  { file: "08-kits.png", section: "kits", caption: "맵 키트: 서버 충돌 상자(주황 선) 안에 맞춘 컨테이너 4색·상자·드럼통·가로등·기울어진 컨테이너." },
  { file: "09-effects.png", section: "effects", caption: "이펙트 아틀라스: 파티클 1024²(4×4)과 데칼 512²(2×2), 칸 이름과 경계." },
];
/** The effects loop is short and fast, so several moments of it are put side by side. */
const DEMO = { file: "14-effects-demo-frames.png", frames: 8, gap: 200, caption: "이펙트 데모를 0.2초 간격으로 8번 찍어 이어 붙였습니다(총구 화염은 발사마다 0.06초만 보임)." };

function freePort(from) {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", () => (from < 5199 ? freePort(from + 1).then(resolve, reject) : reject(new Error("no free port in 5190-5199"))));
    server.listen(from, "127.0.0.1", () => server.close(() => resolve(from)));
  });
}

async function startVite() {
  if (!existsSync(join(client, "src", "gen", "game_pb.ts"))) throw new Error("client/src/gen is missing; run npm run gen in client/ first");
  const port = await freePort(5190);
  const vite = spawn(process.execPath, [join(client, "node_modules", "vite", "bin", "vite.js"), "--port", String(port), "--strictPort", "--host", "127.0.0.1"], {
    cwd: client,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const url = `http://127.0.0.1:${port}/`;
  for (let i = 0; i < 100; i++) {
    if (vite.exitCode !== null) throw new Error(`vite exited with ${vite.exitCode}`);
    try {
      if ((await fetch(url)).ok) return { url, stop: () => vite.kill() };
    } catch {
      // not listening yet
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  vite.kill();
  throw new Error("vite did not start");
}

const escape = (text) => String(text).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

function contactSheet(shots, index, url, messages) {
  const rows = (category, columns) =>
    index.candidates
      .filter((item) => item.category === category)
      .map((item) => `<tr>${columns.map(([, value]) => `<td>${escape(value(item))}</td>`).join("")}</tr>`)
      .join("\n");
  const table = (title, category, columns) =>
    `<h2>${escape(title)}</h2><table><thead><tr>${columns.map(([name]) => `<th>${escape(name)}</th>`).join("")}</tr></thead><tbody>${rows(category, columns)}</tbody></table>`;
  const size = (item) => (item.fit ? item.fit.size.map((v) => v.toFixed(2)).join(" × ") : "");
  return `<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Art Candidates</title>
<style>
:root { color-scheme: dark; --bg: #12130f; --panel: #1c1e19; --line: #565a4a; --text: #e6e3d2; --muted: #9b9a88; --accent: #e7a42c; }
body { margin: 0; padding: 16px; background: var(--bg); color: var(--text); font: 14px/1.5 "Apple SD Gothic Neo", "Malgun Gothic", sans-serif; }
main { max-width: 1520px; margin: 0 auto; }
h1 { margin: 8px 0 4px; } h2 { margin: 28px 0 8px; color: var(--accent); }
p.meta { color: var(--muted); margin: 0 0 16px; }
figure { margin: 0 0 22px; padding: 12px; background: var(--panel); border: 1px solid var(--line); }
figure img { display: block; max-width: 100%; height: auto; }
figcaption { margin-top: 8px; }
figcaption b { color: var(--accent); }
table { border-collapse: collapse; width: 100%; font-size: 13px; background: var(--panel); }
th, td { border: 1px solid var(--line); padding: 4px 8px; text-align: left; vertical-align: top; }
th { color: var(--muted); font-weight: 600; }
</style></head>
<body><main>
<h1>Sudden Front 아트 후보 — 체크포인트 A</h1>
<p class="meta">${escape(new Date().toISOString())} · ${escape(url)}?art&amp;candidates · 후보 ${index.candidates.length}개 (CC0만) · 콘솔 오류 ${messages.errors.length}건, 경고 ${messages.warnings.length}건</p>
${shots.map((shot) => `<figure><img src="${escape(shot.file)}" alt="${escape(shot.file)}" loading="lazy"><figcaption><b>${escape(shot.file)}</b> — ${escape(shot.caption)}</figcaption></figure>`).join("\n")}
${table("캐릭터", "character", [["id", (i) => i.id], ["삼각형", (i) => i.triangles], ["draw call", (i) => i.drawCalls], ["뼈", (i) => i.joints], ["클립", (i) => i.clips.length], ["등신", (i) => i.character?.headsTall ?? "-"], ["머리 관절 m", (i) => i.character?.headJoint ?? "-"], ["앉기 클립", (i) => i.character?.crouchClips.join(", ") || "없음"], ["KB", (i) => (i.bytes / 1024).toFixed(0)]])}
${table("총기", "gun", [["id", (i) => i.id], ["역할", (i) => i.role], ["목표 길이 m", (i) => i.length], ["삼각형", (i) => i.triangles], ["draw call", (i) => i.drawCalls], ["클립", (i) => i.clips.map((c) => c.name.replace(/^.*\|/, "")).join(", ")], ["원본 크기", size], ["KB", (i) => (i.bytes / 1024).toFixed(0)]])}
${table("맵 키트", "map", [["id", (i) => i.id], ["용도", (i) => i.role], ["기준 상자 m", (i) => i.target?.join(" × ") ?? "-"], ["원본 크기", size], ["삼각형", (i) => i.triangles], ["KB", (i) => (i.bytes / 1024).toFixed(0)]])}
${messages.errors.length || messages.warnings.length ? `<h2>콘솔</h2><pre>${escape([...messages.errors, ...messages.warnings].join("\n"))}</pre>` : ""}
</main></body></html>
`;
}

/** The contact sheet for the plain ?art page, with what the page measured. */
function finalSheet(shots, url, messages, report) {
  const row = (cells) => `<tr>${cells.map((cell) => `<td>${escape(cell)}</td>`).join("")}</tr>`;
  const files = report.files.map((file) => row([file.file, file.bytes === null ? "-" : (file.bytes / 1024).toFixed(1), file.triangles ?? "-", file.limitKB ?? "-", file.limitTriangles ?? "-", file.ok ? "OK" : "초과"]));
  const weapons = Object.entries(report.weapons).map(([kind, weapon]) => row([kind, weapon.file ?? "-", weapon.loaded ? "예" : "아니오", weapon.muzzle ? "예" : "아니오", weapon.triangles]));
  const characters = Object.entries(report.characters).map(([team, character]) => row([team, character.file, character.headJoint?.toFixed(3) ?? "-", character.triangles, character.bones.length, character.clips.join(", ")]));
  return `<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Runtime Art</title>
<style>
:root { color-scheme: dark; --bg: #12130f; --panel: #1c1e19; --line: #565a4a; --text: #e6e3d2; --muted: #9b9a88; --accent: #e7a42c; }
body { margin: 0; padding: 16px; background: var(--bg); color: var(--text); font: 14px/1.5 "Apple SD Gothic Neo", "Malgun Gothic", sans-serif; }
main { max-width: 1520px; margin: 0 auto; }
h1 { margin: 8px 0 4px; } h2 { margin: 28px 0 8px; color: var(--accent); }
p.meta { color: var(--muted); margin: 0 0 16px; }
figure { margin: 0 0 22px; padding: 12px; background: var(--panel); border: 1px solid var(--line); }
figure img { display: block; max-width: 100%; height: auto; }
figcaption { margin-top: 8px; }
figcaption b { color: var(--accent); }
table { border-collapse: collapse; width: 100%; font-size: 13px; background: var(--panel); }
th, td { border: 1px solid var(--line); padding: 4px 8px; text-align: left; vertical-align: top; }
th { color: var(--muted); font-weight: 600; }
</style></head>
<body><main>
<h1>Sudden Front 런타임 에셋 — 2단계</h1>
<p class="meta">${escape(new Date().toISOString())} · ${escape(url)}?art · 파일 ${report.files.length}개, ${(report.totalBytes / 1024).toFixed(1)} KB / ${(report.downloadLimitBytes / 1048576).toFixed(0)} MB · 불러오기 실패 ${report.failures.length}건 · 콘솔 오류 ${messages.errors.length}건, 경고 ${messages.warnings.length}건</p>
${shots.map((shot) => `<figure><img src="${escape(shot.file)}" alt="${escape(shot.file)}" loading="lazy"><figcaption><b>${escape(shot.file)}</b> — ${escape(shot.caption)}</figcaption></figure>`).join("\n")}
<h2>파일과 예산</h2><table><thead><tr><th>파일</th><th>KB</th><th>삼각형</th><th>예산 KB</th><th>예산 삼각형</th><th>결과</th></tr></thead><tbody>${files.join("")}</tbody></table>
<h2>무기</h2><table><thead><tr><th>WeaponKind</th><th>파일</th><th>불러옴</th><th>muzzle</th><th>삼각형</th></tr></thead><tbody>${weapons.join("")}</tbody></table>
<h2>캐릭터</h2><table><thead><tr><th>팀</th><th>파일</th><th>머리 뼈 m</th><th>삼각형</th><th>뼈</th><th>클립</th></tr></thead><tbody>${characters.join("")}</tbody></table>
${messages.errors.length || messages.warnings.length ? `<h2>콘솔</h2><pre>${escape([...messages.errors, ...messages.warnings].join("\n"))}</pre>` : ""}
</main></body></html>
`;
}

const out = join(shotsRoot, final ? "art-final" : only ? "art-gallery-only" : "art-gallery");
const index = final ? null : JSON.parse(readFileSync(join(client, "public", "art", "_candidates", "index.json"), "utf8"));
const server = given ? { url: given, stop: () => {} } : await startVite();
const messages = { errors: [], warnings: [] };
let browser;
try {
  browser = await chromium.launch({
    channel: "chrome",
    headless: true,
    args: swiftshader ? ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] : ["--ignore-gpu-blocklist", "--enable-unsafe-swiftshader"],
  });
  const page = await (await browser.newContext({ viewport: { width: 1520, height: 2400 }, deviceScaleFactor: 1 })).newPage();
  page.on("pageerror", (error) => messages.errors.push(String(error)));
  page.on("console", (message) => {
    const where = message.location()?.url ?? "";
    // The dev server has no favicon; the browser asks for one on its own and logs the 404.
    if (where.endsWith("/favicon.ico")) return;
    const text = where ? `${message.text()} (${where})` : message.text();
    if (message.type() === "error") messages.errors.push(text);
    if (message.type() === "warning") messages.warnings.push(text);
  });
  const target = new URL(server.url);
  target.search = final ? "?art" : "?art&candidates";
  await page.goto(target.href);
  await page.waitForSelector(final ? ".art-preview[data-state='ready']" : ".gallery[data-gallery='ready']", { timeout: 300_000 });
  // The page scrolls inside .lobby; let the window scroll instead so a whole section can be captured.
  await page.addStyleTag({ content: "html, body { height: auto !important; overflow: visible !important; } .lobby { position: static !important; overflow: visible !important; } .gallery-toolbar { position: static !important; }" });
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  for (const shot of (final ? FINAL_SHOTS : SHOTS).filter((item) => !only || only.includes(item.section))) {
    if (final && shot.clip) await page.selectOption('section[data-section="characters"] select', shot.clip);
    const options = final ? {} : { ...DEFAULTS, ...shot.options };
    for (const [key, value] of Object.entries(options)) await page.click(`button[data-option="${key}"][data-value="${value}"]`);
    const section = page.locator(`section[data-section="${shot.section}"]`);
    await section.evaluate((node) => node.scrollIntoView({ block: "start" }));
    // A few frames so animated cells move on from the pose they were first drawn in.
    const start = await page.evaluate(() => window.__artGallery.frames());
    await page.waitForFunction((count) => window.__artGallery.frames() >= count, start + 8, { timeout: 120_000 });
    await page.waitForTimeout(400);
    await section.screenshot({ path: join(out, shot.file) });
    console.log(`wrote ${shot.file}`);
  }
  if (final) {
    writeFileSync(join(out, "index.html"), finalSheet(FINAL_SHOTS.filter((item) => !only || only.includes(item.section)), server.url, messages, await page.evaluate(() => window.__artReport)));
    console.log(`contact sheet: ${join(out, "index.html")}`);
  } else if (!only) {
    for (const [key, value] of Object.entries(DEFAULTS)) await page.click(`button[data-option="${key}"][data-value="${value}"]`);
    const demo = page.locator(".gallery-fx-demo canvas");
    await demo.evaluate((node) => node.scrollIntoView({ block: "center" }));
    const frames = [];
    for (let i = 0; i < DEMO.frames; i++) {
      frames.push(await demo.screenshot());
      await page.waitForTimeout(DEMO.gap);
    }
    const { width, height } = await sharp(frames[0]).metadata();
    const columns = 4;
    await sharp({ create: { width: width * columns, height: height * Math.ceil(frames.length / columns), channels: 3, background: "#12130f" } })
      .composite(frames.map((input, i) => ({ input, left: (i % columns) * width, top: Math.floor(i / columns) * height })))
      .png()
      .toFile(join(out, DEMO.file));
    console.log(`wrote ${DEMO.file}`);
    writeFileSync(join(out, "index.html"), contactSheet([...SHOTS, DEMO], index, server.url, messages));
    console.log(`contact sheet: ${join(out, "index.html")}`);
  }
  console.log(`console errors: ${messages.errors.length}, warnings: ${messages.warnings.length}`);
  for (const line of [...messages.errors, ...messages.warnings].slice(0, 10)) console.log(`  ${line}`);
} finally {
  await browser?.close();
  server.stop();
}
if (messages.errors.length) process.exitCode = 1;
