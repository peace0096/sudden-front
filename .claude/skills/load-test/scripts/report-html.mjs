// HTML report of a load test run: stat tiles with verdicts, the agent's analysis, the tick budget,
// time-series charts (inline SVG drawn by a small script, with hover tooltips) and the detail
// tables behind <details>. One self-contained file, no network: it opens from disk.

const esc = (text) =>
  String(text ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

/** The few Markdown bits the analysis uses: **bold**, `code`, [text](url). */
function inline(text) {
  return esc(text)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<a href="$2">$1</a>');
}

/** Analysis Markdown (between the report.md markers) → HTML: bullets and paragraphs. */
function analysisHtml(md) {
  if (!md || md.includes("_(에이전트가 작성")) return '<p class="muted">분석이 아직 없습니다. report.md에 분석을 쓴 뒤 <code>render --dir</code>로 다시 만듭니다.</p>';
  const out = [];
  let list = false;
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^##\s/.test(line) || line.startsWith("<!--")) continue;
    const bullet = line.match(/^[-*]\s+(.*)$/);
    if (bullet) {
      if (!list) out.push("<ul>");
      list = true;
      out.push(`<li>${inline(bullet[1])}</li>`);
    } else {
      if (list) out.push("</ul>");
      list = false;
      out.push(`<p>${inline(line)}</p>`);
    }
  }
  if (list) out.push("</ul>");
  return out.join("\n");
}

const STATUS = {
  "✅ 정상": { cls: "good", icon: "✓", label: "정상" },
  "⚠️ 주의": { cls: "warning", icon: "!", label: "주의" },
  "❌ 문제": { cls: "critical", icon: "✕", label: "문제" },
  "-": { cls: "none", icon: "–", label: "데이터 없음" },
};
const status = (grade) => STATUS[grade] ?? STATUS["-"];
const chip = (grade) => {
  const s = status(grade);
  return `<span class="chip ${s.cls}"><span class="chip-icon" aria-hidden="true">${s.icon}</span>${s.label}</span>`;
};

const table = (head, rows) =>
  `<div class="table-wrap"><table><thead><tr>${head.map((h) => `<th>${esc(h)}</th>`).join("")}</tr></thead><tbody>${rows
    .map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`)
    .join("")}</tbody></table></div>`;

/**
 * @param {object} p
 * @param {object} p.run          run.json
 * @param {object} p.m            the collected metrics
 * @param {{rows: string[][], overall: string}} p.verdict  summary rows [label, value, criterion, grade]
 * @param {{label: string, ms: number|null}[]} p.tickBars  tick time figures against the budget
 * @param {number} p.budgetMs
 * @param {object[]} p.charts     [{ id, title, unit, note, series: [{ name, slot, points }] , ref? }]
 * @param {{title: string, head: string[], rows: string[][], note?: string}[]} p.tables
 * @param {{title: string, head: string[], rows: string[][], note: string}|null} p.comparison
 * @param {string[][]} p.conditions  [label, value]
 * @param {string[]} p.links      Markdown lines for 참고
 * @param {string} p.analysisMd
 */
export function htmlReport(p) {
  const { run, m, verdict } = p;
  const overall = status(verdict.overall);
  const w = m.window;
  const started = new Date(w.start * 1000).toLocaleString("sv-SE");
  const title = `부하 테스트 · 봇 ${run.bot.count}명 · ${run.bot.minutes}분`;

  const tiles = verdict.rows
    .map(
      ([label, value, criterion, grade]) => `
      <div class="tile">
        <div class="tile-label">${esc(label)}</div>
        <div class="tile-value">${inline(value)}</div>
        <div class="tile-foot"><span class="muted">기준 ${esc(criterion)}</span>${chip(grade)}</div>
      </div>`,
    )
    .join("");

  const budget = p.budgetMs;
  const bars = p.tickBars
    .map(({ label, ms }) => {
      const pct = ms === null ? 0 : Math.min(100, (ms / budget) * 100);
      const text = ms === null ? "-" : `${ms.toFixed(2)} ms · ${pct.toFixed(0)}%`;
      return `<div class="bar-row" title="${esc(label)}: ${esc(text)} (예산 ${budget.toFixed(1)} ms)">
        <div class="bar-label">${esc(label)}</div>
        <div class="bar-track"><div class="bar-fill" style="width:${pct.toFixed(2)}%"></div><span class="bar-value" style="left:${pct.toFixed(2)}%">${esc(text)}</span></div>
      </div>`;
    })
    .join("");

  const chartCards = p.charts.length
    ? p.charts
        .map(
          (c) => `
      <figure class="card chart" data-chart="${esc(c.id)}">
        <figcaption><span class="chart-title">${esc(c.title)}</span><span class="muted"> · ${esc(c.unit)}</span><div class="chart-note">${esc(c.note ?? "")}</div></figcaption>
        ${c.series.length > 1 ? `<div class="legend">${c.series.map((s) => `<span><i class="key s${s.slot}"></i>${esc(s.name)}</span>`).join("")}</div>` : ""}
        <div class="plot" role="img" aria-label="${esc(c.title)} 시계열"></div>
      </figure>`,
        )
        .join("")
    : '<p class="muted">이 실행의 metrics.json에 시계열이 없습니다. <code>report --dir</code>로 Prometheus에서 다시 수집하면 차트가 생깁니다.</p>';

  const chartData = {
    duration: Math.round(w.end - w.start),
    steady: [Math.round(w.steadyStart - w.start), Math.round(w.steadyEnd - w.start)],
    charts: Object.fromEntries(p.charts.map((c) => [c.id, { unit: c.unit, ref: c.ref ?? null, digits: c.digits ?? 1, series: c.series }])),
  };

  const comparison = p.comparison
    ? `<section class="card"><h2>${esc(p.comparison.title)}</h2><p class="muted">${inline(p.comparison.note)}</p>${table(p.comparison.head, p.comparison.rows)}</section>`
    : "";

  const details = p.tables
    .map(
      (t) => `<details><summary>${esc(t.title)}</summary>${table(t.head, t.rows)}${t.note ? `<p class="muted small">${inline(t.note)}</p>` : ""}</details>`,
    )
    .join("");

  return `<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>부하 테스트 리포트</title>
<style>
:root {
  color-scheme: light;
  --page: #f9f9f7; --surface: #fcfcfb; --ink: #0b0b0b; --ink-2: #52514e; --muted: #898781;
  --grid: #e1e0d9; --axis: #c3c2b7; --border: rgba(11,11,11,0.10); --band: rgba(11,11,11,0.045);
  --s1: #2a78d6; --s2: #eb6834; --s3: #1baf7a; --track: #cde2fb;
  --good: #0ca30c; --warning: #fab219; --critical: #d03b3b;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    color-scheme: dark;
    --page: #0d0d0d; --surface: #1a1a19; --ink: #f0efec; --ink-2: #c3c2b7; --muted: #898781;
    --grid: #2c2c2a; --axis: #383835; --border: rgba(255,255,255,0.10); --band: rgba(255,255,255,0.05);
    --s1: #3987e5; --s2: #d95926; --s3: #199e70; --track: #184f95;
  }
}
:root[data-theme="dark"] {
  color-scheme: dark;
  --page: #0d0d0d; --surface: #1a1a19; --ink: #f0efec; --ink-2: #c3c2b7; --muted: #898781;
  --grid: #2c2c2a; --axis: #383835; --border: rgba(255,255,255,0.10); --band: rgba(255,255,255,0.05);
  --s1: #3987e5; --s2: #d95926; --s3: #199e70; --track: #184f95;
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--page); color: var(--ink); font: 14px/1.5 system-ui, -apple-system, "Segoe UI", "Malgun Gothic", sans-serif; }
main { max-width: 1120px; margin: 0 auto; padding: 24px 16px 48px; }
h1 { font-size: 22px; margin: 0 0 4px; }
h2 { font-size: 15px; margin: 0 0 12px; }
.muted { color: var(--muted); }
.small { font-size: 12px; }
code { font: 12px ui-monospace, Consolas, monospace; background: var(--band); padding: 1px 4px; border-radius: 4px; }
a { color: var(--s1); }
header { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; justify-content: space-between; margin-bottom: 16px; }
.sub { color: var(--ink-2); }
.verdict { font-size: 15px; font-weight: 600; padding: 6px 14px; border-radius: 999px; background: var(--surface); border: 1px solid var(--border); display: inline-flex; gap: 8px; align-items: center; }
.dot { width: 10px; height: 10px; border-radius: 50%; display: inline-block; }
.card { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 16px; margin: 0 0 16px; min-width: 0; }
.tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 12px; margin-bottom: 16px; }
.tile { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 12px 14px; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.tile-label { color: var(--ink-2); font-size: 13px; }
.tile-value { font-size: 18px; font-weight: 600; overflow-wrap: anywhere; }
.tile-foot { display: flex; justify-content: space-between; align-items: center; gap: 8px; font-size: 12px; margin-top: auto; }
.chip { display: inline-flex; align-items: center; gap: 4px; font-size: 12px; font-weight: 600; color: var(--ink); white-space: nowrap; }
.chip-icon { width: 16px; height: 16px; border-radius: 50%; display: inline-grid; place-items: center; font-size: 10px; color: #fff; }
.chip.good .chip-icon, .dot.good { background: var(--good); }
.chip.warning .chip-icon, .dot.warning { background: var(--warning); color: #0b0b0b; }
.chip.critical .chip-icon, .dot.critical { background: var(--critical); }
.chip.none .chip-icon, .dot.none { background: var(--muted); }
.analysis ul { margin: 0; padding-left: 20px; }
.analysis li { margin: 4px 0; }
.analysis p { margin: 4px 0; }
.bar-row { display: grid; grid-template-columns: 56px 1fr; align-items: center; gap: 12px; margin: 10px 0; }
.bar-label { color: var(--ink-2); font-size: 13px; text-align: right; }
.bar-track { position: relative; height: 16px; background: var(--track); border-radius: 0 4px 4px 0; opacity: 1; }
.bar-fill { height: 100%; background: var(--s1); border-radius: 0 4px 4px 0; }
.bar-value { position: absolute; top: -1px; margin-left: 8px; font-size: 12px; white-space: nowrap; color: var(--ink); font-variant-numeric: tabular-nums; }
.budget-axis { display: grid; grid-template-columns: 56px 1fr; gap: 12px; font-size: 12px; color: var(--muted); }
.budget-axis div:last-child { display: flex; justify-content: space-between; }
.charts { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 480px), 1fr)); gap: 16px; }
.charts .card { margin: 0; }
figure { margin: 0; }
figcaption { margin-bottom: 6px; }
.chart-title { font-weight: 600; }
.chart-note { color: var(--ink-2); font-size: 12px; }
.legend { display: flex; flex-wrap: wrap; gap: 14px; font-size: 12px; color: var(--ink-2); margin-bottom: 4px; }
.legend span { display: inline-flex; align-items: center; gap: 6px; }
.key { width: 14px; height: 2px; border-radius: 1px; display: inline-block; }
.key.s1 { background: var(--s1); } .key.s2 { background: var(--s2); } .key.s3 { background: var(--s3); }
.plot { position: relative; height: 200px; touch-action: pan-y; }
.plot svg { display: block; overflow: visible; }
.plot text { font-size: 11px; fill: var(--muted); font-variant-numeric: tabular-nums; }
.tip { position: absolute; pointer-events: none; background: var(--surface); border: 1px solid var(--border); border-radius: 6px; padding: 6px 8px; font-size: 12px; box-shadow: 0 2px 8px rgba(0,0,0,0.12); white-space: nowrap; z-index: 2; }
.tip b { font-weight: 600; }
.tip .row { display: flex; align-items: center; gap: 6px; }
.band-note { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--ink-2); margin: 0 0 10px; }
.band-note i { width: 14px; height: 10px; background: var(--band); border: 1px solid var(--border); display: inline-block; }
.table-wrap { overflow-x: auto; }
table { border-collapse: collapse; width: 100%; font-size: 13px; }
th, td { text-align: left; padding: 6px 10px; border-bottom: 1px solid var(--grid); vertical-align: top; }
td { font-variant-numeric: tabular-nums; }
th { color: var(--ink-2); font-weight: 600; }
details { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 10px 16px; margin-bottom: 8px; }
summary { cursor: pointer; font-weight: 600; }
details[open] summary { margin-bottom: 8px; }
.links { margin: 0; padding-left: 20px; overflow-wrap: anywhere; }
</style>
</head>
<body>
<main>
  <header>
    <div>
      <h1>${esc(title)}</h1>
      <div class="sub">${esc(started)} 시작 · ${esc(run.bot.mode)} · ${esc(run.bot.map)} · ${esc(run.bot.matchType)} · 코드 <code>${esc(run.git.commit)}</code>${run.git.dirty ? " (커밋하지 않은 변경 있음)" : ""}</div>
    </div>
    <div class="verdict"><span class="dot ${overall.cls}" aria-hidden="true"></span>종합 판정: ${esc(overall.label)}</div>
  </header>

  <section class="tiles" aria-label="요약">${tiles}</section>
  <p class="muted small">판정 기준 구간: 봇 90% 이상이 경기에 들어간 시점부터 종료 신호까지(안정 구간).</p>

  <section class="card analysis"><h2>분석</h2>${analysisHtml(p.analysisMd)}</section>

  <section class="card">
    <h2>틱 시간과 예산(${budget.toFixed(1)} ms)</h2>
    ${bars}
    <div class="budget-axis"><div></div><div><span>0</span><span>예산 ${budget.toFixed(1)} ms</span></div></div>
  </section>

  <h2>시간에 따른 변화</h2>
  <div class="band-note"><i></i>회색 구간: 안정 구간 밖(봇 접속 중·종료 중)</div>
  <section class="charts">${chartCards}</section>

  <div style="height:16px"></div>
  ${comparison}

  <section class="card"><h2>실행 조건</h2>${table(["항목", "값"], p.conditions)}</section>

  <h2>상세 표</h2>
  ${details}

  <section class="card" style="margin-top:16px"><h2>참고</h2><ul class="links">${p.links.map((l) => `<li>${inline(l)}</li>`).join("")}</ul></section>
</main>
<script type="application/json" id="chart-data">${JSON.stringify(chartData).replace(/</g, "\\u003c")}</script>
<script>
(() => {
  const DATA = JSON.parse(document.getElementById("chart-data").textContent);
  const NS = "http://www.w3.org/2000/svg";
  const clock = (s) => Math.floor(s / 60) + ":" + String(Math.round(s % 60)).padStart(2, "0");
  const num = (v, d) => v.toLocaleString("en-US", { maximumFractionDigits: d, minimumFractionDigits: d });
  function niceStep(range, count) {
    const raw = range / count || 1;
    const p = Math.pow(10, Math.floor(Math.log10(raw)));
    const f = raw / p;
    return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10) * p;
  }
  function el(name, attrs, parent) {
    const node = document.createElementNS(NS, name);
    for (const k in attrs) node.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(node);
    return node;
  }
  function nearest(points, t) {
    let lo = 0, hi = points.length - 1;
    while (hi - lo > 1) { const mid = (lo + hi) >> 1; if (points[mid][0] < t) lo = mid; else hi = mid; }
    return Math.abs(points[lo][0] - t) <= Math.abs(points[hi][0] - t) ? points[lo] : points[hi];
  }
  function draw(card) {
    const spec = DATA.charts[card.dataset.chart];
    const box = card.querySelector(".plot");
    box.textContent = "";
    const W = box.clientWidth, H = box.clientHeight;
    const M = { l: 48, r: 12, t: 8, b: 22 };
    const iw = W - M.l - M.r, ih = H - M.t - M.b;
    if (iw < 40) return;
    const series = spec.series.filter((s) => s.points.length);
    let max = 0;
    for (const s of series) for (const p of s.points) max = Math.max(max, p[1]);
    if (spec.ref) max = Math.max(max, spec.ref.y);
    const step = niceStep(max * 1.05 || 1, 4);
    const yMax = Math.max(step, Math.ceil((max * 1.05) / step) * step);
    const xMax = DATA.duration;
    const X = (t) => M.l + (t / xMax) * iw;
    const Y = (v) => M.t + ih - (v / yMax) * ih;
    const svg = el("svg", { width: W, height: H, viewBox: "0 0 " + W + " " + H }, box);
    const [s0, s1] = DATA.steady;
    if (s0 > 0) el("rect", { x: X(0), y: M.t, width: X(s0) - X(0), height: ih, fill: "var(--band)" }, svg);
    if (s1 < xMax) el("rect", { x: X(s1), y: M.t, width: X(xMax) - X(s1), height: ih, fill: "var(--band)" }, svg);
    const yd = step < 1 ? (step < 0.1 ? 2 : 1) : 0;
    for (let v = 0; v <= yMax + 1e-9; v += step) {
      el("line", { x1: M.l, x2: M.l + iw, y1: Y(v), y2: Y(v), stroke: v === 0 ? "var(--axis)" : "var(--grid)", "stroke-width": 1, "shape-rendering": "crispEdges" }, svg);
      const t = el("text", { x: M.l - 6, y: Y(v) + 4, "text-anchor": "end" }, svg);
      t.textContent = v >= 10000 ? num(v / 1000, 0) + "K" : num(v, yd);
    }
    const xStep = [30, 60, 120, 300, 600, 900, 1800, 3600].find((s) => xMax / s <= Math.max(2, Math.floor(iw / 70))) || 3600;
    for (let t = 0; t <= xMax; t += xStep) {
      const tx = el("text", { x: X(t), y: H - 6, "text-anchor": t === 0 ? "start" : "middle" }, svg);
      tx.textContent = clock(t);
    }
    if (spec.ref) {
      el("line", { x1: M.l, x2: M.l + iw, y1: Y(spec.ref.y), y2: Y(spec.ref.y), stroke: "var(--ink-2)", "stroke-width": 1, "shape-rendering": "crispEdges" }, svg);
      const t = el("text", { x: M.l + iw, y: Y(spec.ref.y) - 4, "text-anchor": "end", style: "fill: var(--ink-2)" }, svg);
      t.textContent = spec.ref.label;
    }
    const gap = 3 * (series[0]?.points[1] ? series[0].points[1][0] - series[0].points[0][0] : 5);
    for (const s of series) {
      let d = "";
      s.points.forEach((p, i) => {
        const jump = i === 0 || p[0] - s.points[i - 1][0] > gap;
        d += (jump ? "M" : "L") + X(p[0]).toFixed(1) + " " + Y(p[1]).toFixed(1);
      });
      el("path", { d, fill: "none", stroke: "var(--s" + s.slot + ")", "stroke-width": 2, "stroke-linejoin": "round", "stroke-linecap": "round" }, svg);
    }
    // Hover layer: crosshair, dots with a surface ring, tooltip.
    const cross = el("line", { y1: M.t, y2: M.t + ih, stroke: "var(--axis)", "stroke-width": 1, visibility: "hidden" }, svg);
    const dots = series.map((s) => el("circle", { r: 4, fill: "var(--s" + s.slot + ")", stroke: "var(--surface)", "stroke-width": 2, visibility: "hidden" }, svg));
    const hit = el("rect", { x: M.l, y: 0, width: iw, height: H, fill: "transparent" }, svg);
    const tip = document.createElement("div");
    tip.className = "tip";
    tip.hidden = true;
    box.appendChild(tip);
    const move = (ev) => {
      const r = svg.getBoundingClientRect();
      const t = Math.max(0, Math.min(xMax, ((ev.clientX - r.left - M.l) / iw) * xMax));
      let html = "<b>" + clock(t) + "</b>";
      let at = t;
      series.forEach((s, i) => {
        const p = nearest(s.points, t);
        at = p[0];
        dots[i].setAttribute("cx", X(p[0]));
        dots[i].setAttribute("cy", Y(p[1]));
        dots[i].setAttribute("visibility", "visible");
        html += '<div class="row"><i class="key s' + s.slot + '"></i>' + s.name + " <b>" + num(p[1], spec.digits) + "</b> " + spec.unit + "</div>";
      });
      cross.setAttribute("x1", X(at));
      cross.setAttribute("x2", X(at));
      cross.setAttribute("visibility", "visible");
      tip.innerHTML = html;
      tip.hidden = false;
      const x = X(at);
      tip.style.left = (x + 12 + tip.offsetWidth > W ? x - 12 - tip.offsetWidth : x + 12) + "px";
      tip.style.top = M.t + "px";
    };
    const leave = () => { tip.hidden = true; cross.setAttribute("visibility", "hidden"); dots.forEach((d) => d.setAttribute("visibility", "hidden")); };
    hit.addEventListener("pointermove", move);
    hit.addEventListener("pointerdown", move);
    hit.addEventListener("pointerleave", leave);
  }
  const cards = [...document.querySelectorAll(".chart")];
  const redraw = () => cards.forEach(draw);
  redraw();
  let pending = 0;
  new ResizeObserver(() => { cancelAnimationFrame(pending); pending = requestAnimationFrame(redraw); }).observe(document.querySelector("main"));
})();
</script>
</body>
</html>
`;
}
