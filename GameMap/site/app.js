(function () {
"use strict";
const GM = window.GM;
const $ = (s, r) => (r || document).querySelector(s);
const main = $("#main"), drawer = $("#drawer"), scrim = $("#scrim");
const ST = GM.states;
const LABEL = {reviewed: "Reviewed", proven: "Proven", partial: "Partial", decompiled: "Decompiled", flagged: "Flagged", mapped: "Mapped"};
const HELP = {
  reviewed: "Annotated by a person with evidence (verified against the original by emulation, or read and cross-checked).",
  proven: "Lifted pseudo-C proven equivalent to the machine code by randomized emulation.",
  partial: "Lifter produced code, but not every path could be exercised or verified.",
  decompiled: "Ghidra decompiles it cleanly. Readable, but not proven correct.",
  flagged: "Ghidra output has bad instructions/warnings; needs manual work.",
  mapped: "Only name / unit / class facts exist."
};
const esc = s => String(s == null ? "" : s).replace(/[&<>"]/g, c => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;"}[c]));
const fmt = n => n.toLocaleString("en-US");
const kb = n => n >= 1048576 ? (n / 1048576).toFixed(2) + " MB" : n >= 1024 ? (n / 1024).toFixed(1) + " KB" : n + " B";
const pct = (a, b) => b ? (100 * a / b) : 0;
const pf = (a, b, d) => pct(a, b).toFixed(d == null ? 1 : d) + "%";
const hex = a => a.toString(16).padStart(8, "0");
const col = s => "var(--" + s + ")";
const DONE = ["reviewed", "proven"];
const sum = (o, keys) => keys.reduce((t, k) => t + (o[k] || 0), 0);

const byAddr = new Map(GM.functions.map(f => [f.a, f]));
const subsystems = [...new Set(GM.functions.map(f => f.y))].sort();
const kinds = [...new Set(GM.functions.map(f => f.k))].sort();

function bar(counts, total, cls) {
  return '<div class="bar ' + (cls || "") + '">' + ST.map(s => counts[s] ? '<i class="s-' + s + '" style="width:' + pct(counts[s], total) + '%" title="' + LABEL[s] + ': ' + fmt(counts[s]) + '"></i>' : "").join("") + "</div>";
}
function legend(counts, total, unit) {
  return '<div class="legend">' + ST.map(s => '<span title="' + esc(HELP[s]) + '"><span class="dot s-' + s + '"></span> ' + LABEL[s] + " " + (unit === "b" ? kb(counts[s]) : fmt(counts[s])) + " (" + pf(counts[s], total) + ")</span>").join("") + "</div>";
}
const pill = s => '<span class="pill s-' + s + '">' + LABEL[s] + "</span>";

// ---------- routing ---------------------------------------------------------
function parse() {
  const h = location.hash.replace(/^#\/?/, "");
  const [path, qs] = h.split("?");
  const q = {};
  (qs || "").split("&").filter(Boolean).forEach(p => { const [k, v] = p.split("="); q[k] = decodeURIComponent(v || ""); });
  return {path: path || "overview", q};
}
function go(path, q) {
  const qs = Object.entries(q || {}).filter(([, v]) => v !== "" && v != null).map(([k, v]) => k + "=" + encodeURIComponent(v)).join("&");
  location.hash = "#/" + path + (qs ? "?" + qs : "");
}
function route() {
  const {path, q} = parse();
  const [p, arg] = path.split("/");
  document.querySelectorAll("#nav a").forEach(a => a.classList.toggle("on", a.dataset.t === (p === "fn" ? "functions" : p)));
  if (p === "fn") { if (!main.dataset.view) { main.dataset.view = "functions"; views.functions(q); } openFn(parseInt(arg, 16)); return; }
  closeDrawer(true);
  main.dataset.view = p;
  (views[p] || views.overview)(q);
  window.scrollTo(0, 0);
}
window.addEventListener("hashchange", route);

// ---------- overview --------------------------------------------------------
const views = {};
views.overview = function () {
  const T = GM.total;
  const doneB = sum(T.bc, DONE), doneF = sum(T.fc, DONE);
  const decB = doneB + sum(T.bc, ["partial", "decompiled"]);
  main.innerHTML = `
  <h1>EA Playground — decompilation progress</h1>
  <p class="sub">Game + engine code of the Wii executable (${fmt(T.n)} functions, ${kb(T.s)}). Middleware (${fmt(GM.middleware.n)} functions, ${kb(GM.middleware.s)}: Wii SDK, Havok, Lua, APT runtime…) is excluded from the percentages.</p>
  <div class="hero">
    <div><div class="big" style="color:var(--proven)">${pf(doneB, T.s)}</div><div class="lab">of code <b>proven</b> (bytes)</div></div>
    <div><div class="big" style="color:var(--decompiled)">${pf(decB, T.s)}</div><div class="lab">has readable decompiled code</div></div>
    <div style="flex:1;min-width:260px">
      <div class="lab" style="margin-bottom:4px">By code size · ${kb(T.s)}</div>${bar(T.bc, T.s)}
      <div class="lab" style="margin:12px 0 4px">By function count · ${fmt(T.n)}</div>${bar(T.fc, T.n)}
    </div>
  </div>
  ${legend(T.bc, T.s, "b")}
  <div class="grid" style="margin-top:8px">
    ${ST.map(s => `<a class="card" href="#/functions?state=${s}" style="color:inherit;text-decoration:none"><div class="k"><span class="dot s-${s}"></span> ${LABEL[s]}</div><div class="v">${fmt(T.fc[s])}</div><div class="d">${kb(T.bc[s])} · ${pf(T.bc[s], T.s)}</div></a>`).join("")}
  </div>
  <h2>History</h2>${historyChart()}
  <h2>By subsystem</h2>${subsystemBars(6)}
  <p><a href="#/subsystems">All subsystems →</a></p>
  <h2>Map of the code</h2>
  <p class="sub">Each rectangle is a compilation unit sized by code bytes; its colour bands show the state of its functions. Click to explore.</p>
  <div class="treemap" id="tm"></div>`;
  requestAnimationFrame(treemap);
};

function subsystemBars(limit) {
  const rows = GM.subsystems.filter(s => s.t !== "middleware").sort((a, b) => b.s - a.s).slice(0, limit || 999);
  return '<table><thead><tr><th>Subsystem</th><th class="num">Funcs</th><th class="num">Size</th><th>Progress (bytes)</th><th class="num">Proven</th></tr></thead><tbody>' +
    rows.map(s => `<tr class="click" onclick="location.hash='#/functions?sub=${s.y}&tier=${s.t}'"><td>${esc(s.y)} <span class="badge">${s.t}</span></td><td class="num">${fmt(s.n)}</td><td class="num">${kb(s.s)}</td><td class="bar-cell">${bar(s.bc, s.s, "sm")}</td><td class="num">${pf(sum(s.bc, DONE), s.s)}</td></tr>`).join("") + "</tbody></table>";
}
views.subsystems = function () {
  main.innerHTML = `<h1>Subsystems</h1><p class="sub">Grouped by what the code does. Click a row to list its functions.</p>` + subsystemBars();
};

function historyChart() {
  const H = GM.history;
  if (!H || H.length < 2) return '<div class="note">History appears here once at least two snapshots exist (<code>python3 GameMap/tools/build_site.py --snapshot "label"</code>).</div>';
  const W = 900, Hh = 220, pl = 40, pr = 10, pt = 10, pb = 30;
  const x = i => pl + (W - pl - pr) * i / (H.length - 1);
  const y = v => pt + (Hh - pt - pb) * (1 - v);
  let g = "";
  for (let t = 0; t <= 4; t++) g += `<line x1="${pl}" x2="${W - pr}" y1="${y(t / 4)}" y2="${y(t / 4)}" stroke="var(--line)"/><text x="${pl - 6}" y="${y(t / 4) + 4}" text-anchor="end" font-size="11" fill="var(--mute)">${t * 25}%</text>`;
  const acc = H.map(() => 0);
  const paths = [];
  ST.slice().reverse().forEach(s => {
    const lo = acc.slice();
    H.forEach((h, i) => acc[i] += (h.bc[s] || 0) / h.s);
    const top = H.map((h, i) => x(i) + "," + y(acc[i])).join(" L");
    const bot = H.map((h, i) => x(H.length - 1 - i) + "," + y(lo[H.length - 1 - i])).join(" L");
    paths.push(`<path d="M${top} L${bot} Z" fill="${col(s)}" opacity=".9"><title>${LABEL[s]}</title></path>`);
  });
  const lab = H.map((h, i) => `<text x="${x(i)}" y="${Hh - 8}" font-size="11" text-anchor="middle" fill="var(--mute)">${esc(h.label)}</text>`).join("");
  return `<div class="chart"><svg viewBox="0 0 ${W} ${Hh}">${g}${paths.join("")}${lab}</svg></div>`;
}

// ---------- treemap (squarified) ---------------------------------------------
function squarify(items, x, y, w, h) {
  const out = [];
  const total = items.reduce((t, i) => t + i.v, 0);
  if (!total) return out;
  const scale = w * h / total;
  items = items.map(i => Object.assign({}, i, {a: i.v * scale}));
  let rest = items.slice();
  while (rest.length) {
    const side = Math.min(w, h);
    let row = [], best = Infinity, sumA = 0;
    for (const it of rest) {
      const trial = row.concat(it), s = sumA + it.a;
      const mx = Math.max(...trial.map(t => t.a)), mn = Math.min(...trial.map(t => t.a));
      const worst = Math.max(side * side * mx / (s * s), s * s / (side * side * mn));
      if (worst > best && row.length) break;
      row = trial; sumA = s; best = worst;
    }
    rest = rest.slice(row.length);
    const thick = sumA / side;
    let off = 0;
    for (const it of row) {
      const len = it.a / thick;
      if (w >= h) out.push({it, x, y: y + off, w: thick, h: len}); else out.push({it, x: x + off, y, w: len, h: thick});
      off += len;
    }
    if (w >= h) { x += thick; w -= thick; } else { y += thick; h -= thick; }
  }
  return out;
}
function treemap() {
  const el = $("#tm"); if (!el) return;
  const W = el.clientWidth, H = el.clientHeight;
  const items = GM.units.filter(u => u.t !== "middleware").map(u => ({v: u.s, u})).sort((a, b) => b.v - a.v);
  el.innerHTML = squarify(items, 0, 0, W, H).map(r => {
    const u = r.it.u;
    let acc = 0;
    const stops = ST.filter(s => u.bc[s]).map(s => { const a = acc; acc += 100 * u.bc[s] / u.s; return `${col(s)} ${a}% ${acc}%`; });
    const label = r.w > 46 && r.h > 16 ? esc(u.u) : "";
    return `<div class="cell" style="left:${r.x}px;top:${r.y}px;width:${r.w}px;height:${r.h}px;background:linear-gradient(90deg,${stops.join(",")})" title="${esc(u.u)} · ${kb(u.s)} · ${u.n} functions · ${pf(sum(u.bc, DONE), u.s)} proven" onclick="location.hash='#/functions?unit=${encodeURIComponent(u.u)}'">${label}</div>`;
  }).join("");
}
window.addEventListener("resize", treemap);

// ---------- units -----------------------------------------------------------
views.units = function (q) {
  const tier = q.tier || "", text = (q.q || "").toLowerCase();
  const sort = q.sort || "a", dir = q.dir === "asc" ? 1 : -1;
  let rows = GM.units.filter(u => (!tier || u.t === tier) && (!text || u.u.toLowerCase().includes(text) || u.y.includes(text)));
  const val = {a: u => u.a, u: u => u.u, s: u => u.s, n: u => u.n, p: u => pct(sum(u.bc, DONE), u.s), d: u => pct(u.bc.decompiled + u.bc.partial + sum(u.bc, DONE), u.s)}[sort];
  rows.sort((a, b) => (val(a) > val(b) ? 1 : val(a) < val(b) ? -1 : 0) * (sort === "a" || sort === "u" ? (q.dir === "desc" ? -1 : 1) : dir));
  const th = (k, t, c) => `<th class="${c || ""}" data-sort="${k}">${t}${sort === k ? (q.dir === "asc" ? " ▲" : " ▼") : ""}</th>`;
  main.innerHTML = `<h1>Units</h1><p class="sub">One row per compilation unit (link order), like a decomp.dev "report" — but with proof state rather than match percentage.</p>
  <div class="tools"><input type="search" id="uq" placeholder="Filter units…" value="${esc(q.q || "")}">
  <select id="ut"><option value="">game + engine</option><option value="game">game</option><option value="engine">engine</option></select>
  <span class="count">${rows.length} units</span></div>
  <table><thead><tr>${th("u", "Unit")}<th>Subsystem</th>${th("n", "Funcs", "num")}${th("s", "Size", "num")}<th>Progress (bytes)</th>${th("p", "Proven", "num")}${th("d", "Decoded", "num")}</tr></thead><tbody>` +
    rows.map(u => `<tr class="click" data-u="${esc(u.u)}"><td class="mono">${esc(u.u)}</td><td>${esc(u.y)} <span class="badge">${u.t}</span></td><td class="num">${u.n}</td><td class="num">${kb(u.s)}</td><td class="bar-cell">${bar(u.bc, u.s, "sm")}</td><td class="num">${pf(sum(u.bc, DONE), u.s, 0)}</td><td class="num">${pf(u.bc.decompiled + u.bc.partial + sum(u.bc, DONE), u.s, 0)}</td></tr>`).join("") + "</tbody></table>";
  $("#ut").value = tier;
  let tmr; $("#uq").oninput = e => { clearTimeout(tmr); tmr = setTimeout(() => go("units", Object.assign({}, q, {q: e.target.value})), 250); };
  $("#ut").onchange = e => go("units", Object.assign({}, q, {tier: e.target.value}));
  main.querySelectorAll("th[data-sort]").forEach(t => t.onclick = () => go("units", Object.assign({}, q, {sort: t.dataset.sort, dir: sort === t.dataset.sort && q.dir !== "asc" ? "asc" : "desc"})));
  main.querySelectorAll("tr[data-u]").forEach(r => r.onclick = () => go("functions", {unit: r.dataset.u}));
  if (q.q) { const i = $("#uq"); i.focus(); i.setSelectionRange(i.value.length, i.value.length); }
};

// ---------- functions -------------------------------------------------------
let shown = 300;
views.functions = function (q) {
  const on = new Set((q.state || "").split(",").filter(Boolean));
  const opt = (arr, v, all) => `<option value="">${all}</option>` + arr.map(x => `<option ${x === v ? "selected" : ""}>${x}</option>`).join("");
  main.innerHTML = `<h1>Functions</h1><p class="sub">Every game/engine function. Click one for its facts and — if you have built the local code pack — compiled code next to the decoded version.</p>
  <div class="tools">
    <input type="search" id="fq" placeholder="Search name, class, address, summary…" value="${esc(q.q || "")}">
    ${ST.map(s => `<button class="chip ${on.has(s) ? "on" : ""}" data-s="${s}"><span class="dot s-${s}"></span> ${LABEL[s]}</button>`).join("")}
    <button class="chip ${q.dec ? "on" : ""}" id="decodedOnly" title="Only functions with decoded/decompiled code (everything except flagged and mapped)">Decoded only</button>
  </div>
  <div class="tools">
    <select id="ftier">${opt(["game", "engine"], q.tier, "all tiers")}</select>
    <select id="fsub">${opt(subsystems, q.sub, "all subsystems")}</select>
    <select id="fkind">${opt(kinds, q.kind, "all kinds")}</select>
    <select id="fsort">${[["a", "address"], ["s", "size ↓"], ["ss", "size ↑"], ["fi", "most called"]].map(([v, t]) => `<option value="${v}" ${q.sort === v ? "selected" : ""}>sort: ${t}</option>`).join("")}</select>
    ${q.unit ? `<span class="badge">unit: ${esc(q.unit)} <a href="#/functions">✕</a></span>` : ""}
    <span class="count" id="fcount"></span>
  </div>
  <div id="fnlist"></div>`;
  const apply = patch => { shown = 300; go("functions", Object.assign({}, q, patch)); };
  let tmr; $("#fq").oninput = e => { clearTimeout(tmr); tmr = setTimeout(() => apply({q: e.target.value}), 250); };
  main.querySelectorAll(".chip[data-s]").forEach(c => c.onclick = () => { const n = new Set(on); n.has(c.dataset.s) ? n.delete(c.dataset.s) : n.add(c.dataset.s); apply({state: [...n].join(",")}); });
  $("#decodedOnly").onclick = () => apply({dec: q.dec ? "" : "1"});
  $("#ftier").onchange = e => apply({tier: e.target.value});
  $("#fsub").onchange = e => apply({sub: e.target.value});
  $("#fkind").onchange = e => apply({kind: e.target.value});
  $("#fsort").onchange = e => apply({sort: e.target.value});
  if (q.q) { const i = $("#fq"); i.focus(); i.setSelectionRange(i.value.length, i.value.length); }
  renderFns(q, on);
};
function renderFns(q, on) {
  const text = (q.q || "").toLowerCase();
  let rows = GM.functions.filter(f =>
    (!on.size || on.has(f.st)) && (!q.dec || (f.st !== "flagged" && f.st !== "mapped")) &&
    (!q.tier || f.t === q.tier) && (!q.sub || f.y === q.sub) && (!q.kind || f.k === q.kind) && (!q.unit || f.u === q.unit) &&
    (!text || f.n.toLowerCase().includes(text) || hex(f.a).includes(text.replace(/^0x/, "")) || f.sum.toLowerCase().includes(text) || f.u.toLowerCase().includes(text)));
  const s = q.sort;
  if (s === "s") rows.sort((a, b) => b.s - a.s); else if (s === "ss") rows.sort((a, b) => a.s - b.s); else if (s === "fi") rows.sort((a, b) => b.fi - a.fi);
  $("#fcount").textContent = fmt(rows.length) + " functions · " + kb(rows.reduce((t, f) => t + f.s, 0));
  const part = rows.slice(0, shown);
  $("#fnlist").innerHTML = rows.length ? `<table id="fntable"><thead><tr><th>Address</th><th>Function</th><th>Unit</th><th class="num">Size</th><th>Kind</th><th>State</th><th>What it does</th></tr></thead><tbody>` +
    part.map(f => `<tr class="click" data-a="${hex(f.a)}"><td class="mono">${hex(f.a)}</td><td class="mono">${esc(f.n)}<span style="color:var(--mute)">(${esc(f.g)})</span></td><td>${esc(f.u)}</td><td class="num">${f.s}</td><td>${f.k}</td><td>${pill(f.st)}</td><td>${esc(f.sum)}</td></tr>`).join("") + "</tbody></table>" +
    (rows.length > shown ? `<p><button class="chip" id="more">Show ${Math.min(300, rows.length - shown)} more</button></p>` : "") : '<div class="empty">No functions match.</div>';
  main.querySelectorAll("tr[data-a]").forEach(r => r.onclick = () => { location.hash = "#/fn/" + r.dataset.a; });
  const m = $("#more"); if (m) m.onclick = () => { shown += 300; renderFns(q, on); };
}

// ---------- function drawer -------------------------------------------------
const codeCache = {};
const safe = u => u.replace(/[^A-Za-z0-9._-]/g, "_");
async function codeFor(f) {
  const key = safe(f.u);
  if (!(key in codeCache)) {
    codeCache[key] = fetch("code/" + key + ".json").then(r => r.ok ? r.json() : null).catch(() => null);
  }
  const shard = await codeCache[key];
  return shard ? shard[hex(f.a)] || null : null;
}
function closeDrawer(quiet) { drawer.hidden = true; scrim.hidden = true; document.body.style.overflow = ""; }
scrim.onclick = () => { closeDrawer(); if (location.hash.startsWith("#/fn/")) history.back(); };
document.addEventListener("keydown", e => { if (e.key === "Escape" && !drawer.hidden) scrim.onclick(); });

async function openFn(addr) {
  const f = byAddr.get(addr);
  if (!f) return;
  drawer.hidden = false; scrim.hidden = false; document.body.style.overflow = "hidden";
  const pack = await codeFor(f);
  let tab = "overview";
  const render = () => {
    const have = pack || {};
    const tabs = [["overview", "Overview"], ["compare", "Compare"], ["asm", "Compiled"], ["ghidra", "Ghidra C"], ["lifted", "Lifted C"], ["evidence", "Evidence"]];
    drawer.innerHTML = `<button class="x" title="Close (Esc)">×</button>
    <h3 class="mono">${esc(f.n)}</h3><div>${pill(f.st)} <span class="badge">${f.t}/${esc(f.y)}</span> <span class="badge">${esc(f.k)}</span> <span class="mono" style="color:var(--mute)">0x${hex(f.a)}</span></div>
    <div class="tabs">${tabs.map(([k, t]) => `<button data-t="${k}" class="${tab === k ? "on" : ""}">${t}</button>`).join("")}</div><div id="tabbody"></div>`;
    $(".x", drawer).onclick = scrim.onclick;
    drawer.querySelectorAll(".tabs button").forEach(b => b.onclick = () => { tab = b.dataset.t; render(); });
    const body = $("#tabbody");
    const nopack = '<div class="note">Code is not part of this site. Build the local <b>code pack</b> from your own copy of the executable to see it here:<br><code>python3 GameMap/tools/build_code_pack.py --elf path/to/playgroundz.elf</code>, then serve <code>GameMap/site</code> (e.g. <code>python3 -m http.server -d GameMap/site</code>). The pack is git-ignored because it contains EA code.</div>';
    const code = (t, label) => t ? `<pre class="code">${esc(t)}</pre>` : `<div class="note">${pack ? "No " + label + " for this function (" + esc(f.why || f.ls || "not produced") + ")." : ""}</div>` + (pack ? "" : nopack);
    if (tab === "overview") body.innerHTML = overview(f);
    else if (tab === "asm") body.innerHTML = code(have.asm, "compiled listing");
    else if (tab === "ghidra") body.innerHTML = code(have.ghidra, "Ghidra output");
    else if (tab === "lifted") body.innerHTML = code(have.lifted, "lifted code");
    else if (tab === "evidence") body.innerHTML = evidence(f);
    else if (tab === "compare") {
      if (!pack) { body.innerHTML = nopack; return; }
      const opts = [["lifted", "Lifted C (proven)"], ["ghidra", "Ghidra C"]].filter(([k]) => have[k]);
      const right = window.__cmp && have[window.__cmp] ? window.__cmp : (opts[0] || ["ghidra"])[0];
      body.innerHTML = `<div class="tools"><span>Compare compiled code with:</span><select id="cmp">${opts.map(([k, t]) => `<option value="${k}" ${k === right ? "selected" : ""}>${t}</option>`).join("")}</select>${f.st === "proven" || f.st === "reviewed" ? '<span class="diff-ok">✔ decoded version proven equivalent to the compiled one</span>' : '<span class="badge">not proven — treat the right side as a reading aid</span>'}</div>
      <div class="split"><div><h4>Compiled (PowerPC)</h4><pre class="code">${esc(have.asm || "")}</pre></div><div><h4>${right === "lifted" ? "Lifted C" : "Ghidra C"}</h4><pre class="code">${esc(have[right] || "(none)")}</pre></div></div>`;
      const sel = $("#cmp"); if (sel) sel.onchange = e => { window.__cmp = e.target.value; render(); };
    }
  };
  render();
}
function overview(f) {
  const g = f.gh, l = f.lf;
  const lk = (label, val) => val ? `<dt>${label}</dt><dd>${val}</dd>` : "";
  return `<dl class="kv">
    ${lk("Summary", esc(f.sum))}
    ${lk("Signature", '<span class="mono">' + esc(f.n) + "(" + esc(f.g) + ")</span>")}
    <dt>Address / size</dt><dd class="mono">0x${hex(f.a)} · ${f.s} bytes · ${f.ni} instructions</dd>
    <dt>Unit</dt><dd><a href="#/functions?unit=${encodeURIComponent(f.u)}">${esc(f.u)}</a></dd>
    <dt>Called by / calls</dt><dd>${f.fi} direct callers · ${f.fo} direct callees</dd>
    ${g ? `<dt>Ghidra</dt><dd>${g[0]} lines · ${g[1]} loops · ${g[2]} gotos · ${g[3]} switches · ${g[4]} warnings</dd>` : ""}
    ${l ? `<dt>Lift verification</dt><dd>${l[0]} randomized trials matched · ${l[2]}/${l[1]} basic blocks exercised · ${l[3]} statements</dd>` : ""}
    ${f.why ? `<dt>Blocker</dt><dd>${esc(f.why)}</dd>` : ""}
    <dt>State</dt><dd>${pill(f.st)} — ${esc(HELP[f.st])}</dd>
    <dt>Mangled</dt><dd class="mono">${esc(f.m)}</dd></dl>`;
}
function evidence(f) {
  if (!f.ev && f.st !== "proven") return '<div class="note">No human-written evidence yet. Machine-derived facts are on the Overview tab.</div>';
  const e = f.ev || {};
  return `<dl class="kv">${f.st === "proven" ? "<dt>Proof</dt><dd>Lifted to a guarded IR by symbolic execution; the IR was run against the original code under a PowerPC 750 emulator on randomized inputs (registers, memory, FP) and every trial matched, including memory writes, return values and call arguments. See <a href='#/method'>how it's measured</a>.</dd>" : ""}
  ${e.verified_by ? `<dt>Verified by</dt><dd>${esc(e.verified_by)}</dd>` : ""}${e.doc ? `<dt>Documented in</dt><dd class="mono">GameMap/${esc(e.doc)}</dd>` : ""}</dl>`;
}

// ---------- next up ---------------------------------------------------------
views.next = function () {
  const T = GM.total;
  const notDone = GM.functions.filter(f => !DONE.includes(f.st));
  const quick = notDone.filter(f => f.st === "decompiled" && f.k !== "loop" && f.s <= 160 && !f.why.startsWith("lift: loop")).sort((a, b) => b.fi - a.fi).slice(0, 25);
  const big = GM.functions.filter(f => f.st === "flagged").sort((a, b) => b.s - a.s).slice(0, 25);
  const hot = notDone.sort((a, b) => b.fi - a.fi).slice(0, 25);
  const tbl = rows => `<table><thead><tr><th>Function</th><th>Unit</th><th class="num">Size</th><th class="num">Callers</th><th>Blocker</th></tr></thead><tbody>` + rows.map(f => `<tr class="click" data-a="${hex(f.a)}"><td class="mono">${esc(f.n)}</td><td>${esc(f.u)}</td><td class="num">${f.s}</td><td class="num">${f.fi}</td><td>${esc(f.why || f.st)}</td></tr>`).join("") + "</tbody></table>";
  main.innerHTML = `<h1>Next up</h1><p class="sub">What stands between the current state and "everything proven". Blockers are counted from the last verification run.</p>
  <h2>Why functions are not proven yet</h2>
  <table><thead><tr><th>Blocker</th><th class="num">Functions</th><th class="num">Code</th><th>Share of remaining bytes</th></tr></thead><tbody>${GM.blockers.map(b => `<tr><td>${esc(b.why)}</td><td class="num">${fmt(b.n)}</td><td class="num">${kb(b.s)}</td><td class="bar-cell"><div class="bar sm"><i class="s-flagged" style="width:${pct(b.s, T.s - sum(T.bc, DONE))}%"></i></div></td></tr>`).join("")}</tbody></table>
  <h2>Quick wins</h2><p class="sub">Small, loop-free, already decompiled, most-called first — the cheapest to push to "proven".</p>${tbl(quick)}
  <h2>Most-called functions not yet proven</h2>${tbl(hot)}
  <h2>Biggest flagged functions</h2><p class="sub">Ghidra hit bad instructions (mostly paired-single float ops); these need a lifter extension or manual reading.</p>${tbl(big)}`;
  main.querySelectorAll("tr[data-a]").forEach(r => r.onclick = () => { location.hash = "#/fn/" + r.dataset.a; });
};

// ---------- method ----------------------------------------------------------
views.method = function () {
  main.innerHTML = `<div class="method"><h1>How progress is measured</h1>
  <p>Matching-decompilation sites like decomp.dev count bytes of C that recompile to identical machine code. This project has no compiler-matching goal: the executable is being <em>mapped and decoded</em> for a clean-room reconstruction in Rust/Bevy. So progress is measured by <b>how much of the code has been turned into something readable, and how much of that has been proven to behave identically</b>.</p>
  <table><thead><tr><th>State</th><th>Meaning</th></tr></thead><tbody>${ST.map(s => `<tr><td>${pill(s)}</td><td>${esc(HELP[s])}</td></tr>`).join("")}</tbody></table>
  <h2>Proof</h2><p>A function is <b>proven</b> when its lifted pseudo-C — produced by symbolic execution of the PowerPC into a guarded IR — behaves identically to the original machine code on randomized inputs run under a PowerPC 750 emulator (Unicorn). Compared: return registers, every memory write, every call target and argument, and FP results (NaN-canonicalised). Every block must be exercised; otherwise the function stays <b>partial</b>. Functions with loops, indirect branches or paired-single instructions are not yet supported by the lifter.</p>
  <p class="note">Verification coverage: the lifter/verifier run reached ${fmt(GM.lift_tested||0)} of ${fmt(GM.total.n)} functions when this snapshot was taken; the rest are shown by their Ghidra state only and are not yet attempted (not failed). The loop-support lifter was added after that run, so loop functions are under-counted.</p>
  <h2>Scope</h2><p>Only game and engine code count in the percentages. Compilation units are attributed to tiers from the original link order (<code>STT_FILE</code> symbols) and refined by dynamic programming over unanchored runs. Middleware — Wii SDK, Havok, nw4r, Lua, the APT UI runtime, EA libraries — is listed for size but not tracked.</p>
  <h2>Caveats</h2><ul><li>"Decompiled" comes from Ghidra 11.3 and may be wrong; only <b>proven</b> and <b>reviewed</b> mean checked.</li><li>Percentages are by function <em>size in bytes</em> unless stated.</li><li>Indirect/virtual calls are not in the call graph.</li></ul>
  <h2>Viewing code</h2><p>This site contains no game code. If you own the game, run <code>tools/build_code_pack.py</code> against your own ELF; a local <code>site/code/</code> folder then lights up the <b>Compare</b>, <b>Compiled</b>, <b>Ghidra C</b> and <b>Lifted C</b> tabs on every function.</p>
  <p class="sub">Data generated ${esc(GM.generated)} · ELF sha256 <span class="mono">${esc(GM.elf_sha256 || "")}</span></p></div>`;
};

$("#meta").textContent = fmt(GM.total.n) + " functions · " + kb(GM.total.s) + " · " + GM.generated.slice(0, 10);
route();
})();
