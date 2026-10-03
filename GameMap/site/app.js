(function () {
"use strict";
const GM = window.GM;
const $ = id => document.getElementById(id);
const esc = s => String(s == null ? "" : s).replace(/[&<>"]/g, c => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;"}[c]));
const fmt = n => n.toLocaleString("en-US");
const kb = b => b >= 1048576 ? (b / 1048576).toFixed(2) + " MB" : b >= 1024 ? (b / 1024).toFixed(1) + " KB" : b + " B";
const pct = (a, b) => b ? 100 * a / b : 0;
const pf = (a, b, d) => pct(a, b).toFixed(d == null ? 1 : d) + "%";
const hex = a => a.toString(16).padStart(8, "0");
const css = n => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
const store = {get(k) { try { return localStorage.getItem("gm." + k); } catch (e) { return null; } },
               set(k, v) { try { localStorage.setItem("gm." + k, v); } catch (e) { /* private mode */ } }};

// ---- the two measurements ------------------------------------------------------------------------------------------
const DEC = [
  {k: "reviewed", label: "Reviewed", desc: "A person annotated it with evidence: verified against the original by emulation, or read and cross-checked."},
  {k: "proven", label: "Proven", desc: "Lifted pseudo-C proven equivalent to the machine code by randomized emulation."},
  {k: "partial", label: "Partial", desc: "The lifter produced code, but not every path could be exercised or verified."},
  {k: "decompiled", label: "Decompiled", desc: "Ghidra decompiles it cleanly. Readable, but not proven correct."},
  {k: "flagged", label: "Flagged", desc: "Ghidra output has bad instructions or warnings; needs manual work."},
  {k: "mapped", label: "Mapped", desc: "Only name, unit, class and call facts exist."},
  {k: "untracked", label: "Not tracked", desc: "Middleware (Wii SDK, Havok, nw4r, Lua, APT runtime, EA libraries): listed for size only."},
];
const RUN = [
  {k: "port", label: "Hand-ported Rust", desc: "Rewritten by hand in the remake; the Rust cites this function by address or name."},
  {k: "host", label: "Rust hook", desc: "A Rust host function runs in its place inside the PowerPC VM (physics, assets, sound, front end)."},
  {k: "run", label: "Ran original code", desc: "The original PowerPC executed in the VM during the recorded scenarios."},
  {k: "native", label: "Not reached yet", desc: "Allowed to run as original code, but no recorded scenario has entered it."},
  {k: "stub", label: "Stubbed", desc: "Rendering or audio detail the VM skips: returns 0 and does nothing."},
  {k: "trap", label: "Unimplemented service", desc: "Engine service with no Rust version yet; entering it stops the VM."},
];
const DI = Object.fromEntries(DEC.map((s, i) => [s.k, i])), RI = Object.fromEntries(RUN.map((s, i) => [s.k, i]));
const DONE_D = ["reviewed", "proven"], READ_D = ["reviewed", "proven", "partial", "decompiled"];
const LIVE_R = ["port", "host", "run"], RUST_R = ["port", "host"];
const KIND = {hook: "Rust hook", observe: "Rust observer", port: "Hand-written Rust", vm: "VM host code", drive: "Rust calls the original"};
const VMTXT = {native: "Runs as original code", host: "Rust hook replaces it", observe: "Rust observer, then original", stub: "Stub (returns 0)", trap: "Trap (unimplemented)"};
const varD = k => "var(--d-" + k + ")", varR = k => "var(--s-" + k + ")";

// ---- data ----------------------------------------------------------------------------------------------------------
const C = Object.fromEntries(GM.cols.map((c, i) => [c, i]));
const F = GM.funcs.map((r, i) => {
  const g = GM.groups[r[C.group]];
  const f = {i, a: r[C.a], s: r[C.s], unit: r[C.unit], grp: r[C.group], t: g.t, y: g.y,
    st: r[C.st] >= 0 ? GM.states[r[C.st]] : "untracked", why: r[C.why] >= 0 ? GM.whys[r[C.why]] : "",
    kind: r[C.kind] >= 0 ? GM.kinds[r[C.kind]] : "", fi: r[C.fi], fo: r[C.fo], gh: r[C.gh] || null, lf: r[C.lf] || null, ls: r[C.ls],
    rst: GM.rstates[r[C.rst]], vm: GM.vmkinds[r[C.vm]], scen: r[C.scen], entries: r[C.entries], refs: r[C.refs],
    n: r[C.n], g: r[C.g], m: r[C.m] || r[C.n], sum: r[C.sum], ev: r[C.ev] || null};
  f.l = (f.n + " " + f.m + " " + f.sum).toLowerCase();
  return f;
});
const byAddr = new Map(F.map(f => [f.a, f]));
const UNITS = GM.units.map((u, i) => ({i, name: u.u, t: u.t, y: u.y, a: u.a}));

// ---- view state ----------------------------------------------------------------------------------------------------
const view = {scope: store.get("scope") || "ge", axis: store.get("axis") || "d"};
const filter = {q: "", d: new Set(), r: new Set(), grp: -1, unit: -1, sort: "addr"};
let selected = -1, shown = 300;
const inScope = f => view.scope === "all" || (view.scope === "mw" ? f.t === "middleware" : f.t !== "middleware");
const colOf = f => view.axis === "d" ? varD(f.st) : varR(f.rst);
let SF = [];   // functions in scope
let SU = [];   // units in scope with stats

function rebuild() {
  SF = F.filter(inScope);
  const per = new Map();
  for (const f of SF) {
    let u = per.get(f.unit);
    if (!u) { u = Object.assign({}, UNITS[f.unit], {funcs: [], bytes: 0, bd: {}, br: {}}); per.set(f.unit, u); }
    u.funcs.push(f); u.bytes += f.s;
    u.bd[f.st] = (u.bd[f.st] || 0) + f.s; u.br[f.rst] = (u.br[f.rst] || 0) + f.s;
  }
  SU = [...per.values()];
  for (const u of SU) {
    const sum = (o, ks) => ks.reduce((t, k) => t + (o[k] || 0), 0);
    u.doneD = sum(u.bd, DONE_D) / u.bytes; u.doneR = sum(u.br, LIVE_R) / u.bytes;
  }
}

// ---- progress panels -----------------------------------------------------------------------------------------------
function tally(list, key, states) {
  const b = Object.fromEntries(states.map(s => [s.k, 0])), n = Object.fromEntries(states.map(s => [s.k, 0]));
  for (const f of list) { b[f[key]] += f.s; n[f[key]]++; }
  return {b, n, bytes: list.reduce((t, f) => t + f.s, 0), count: list.length};
}
const hl = (big, cap) => '<div class="hl"><span class="big num">' + big + '</span><span class="cap">' + cap + "</span></div>";
function bar(states, vals, total, colour) {
  return states.filter(s => vals[s.k]).map(s => '<span style="flex:' + vals[s.k] + ";background:" + colour(s.k) + '" title="' + esc(s.label) + ": " + pf(vals[s.k], total, 2) + '"></span>').join("");
}
function legend(states, T, set, colour, which) {
  return '<div class="legend">' + states.filter(s => T.n[s.k] || s.k !== "untracked").map(s =>
    '<button type="button" class="lg' + (set.has(s.k) ? " on" : "") + '" data-w="' + which + '" data-k="' + s.k + '"><span class="sw" style="background:' + colour(s.k) + '"></span><b>' + esc(s.label) +
    '</b><span class="v num">' + fmt(T.n[s.k]) + " fn · " + kb(T.b[s.k]) + " · " + pf(T.b[s.k], T.bytes) + '</span><span class="d">' + esc(s.desc) + "</span></button>").join("") + "</div>";
}
function bars(states, T, colour, label) {
  return '<div class="barrow"><span class="lab">By code size</span><div class="bar" role="img" aria-label="' + esc(label) + ' by bytes">' + bar(states, T.b, T.bytes, colour) + "</div></div>" +
    '<div class="barrow"><span class="lab">By function</span><div class="bar" role="img" aria-label="' + esc(label) + ' by function count">' + bar(states, T.n, T.count, colour) + "</div></div>";
}
function progress() {
  const sumB = (T, ks) => ks.reduce((t, k) => t + T.b[k], 0), sumN = (T, ks) => ks.reduce((t, k) => t + T.n[k], 0);
  const TD = tally(SF, "st", DEC), TR = tally(SF, "rst", RUN);
  const tracked = SF.filter(f => f.st !== "untracked"), TT = tally(tracked, "st", DEC);
  let d = '<h3>Decoding</h3><p class="what">How much of the original code has been turned into something readable, and how much of that is proven to behave identically.</p>';
  if (!tracked.length) d += '<p class="note">Middleware is not tracked for decoding: the Wii SDK, Havok, nw4r, Lua, the APT runtime and EA libraries are listed for size only. Switch to <b>Game + engine</b> to see decoding progress.</p>';
  else d += '<div class="headline">' + hl(pf(sumB(TT, DONE_D), TT.bytes), "of game + engine code proven or reviewed (" + fmt(sumN(TT, DONE_D)) + " functions)") +
    hl(pf(sumB(TT, READ_D), TT.bytes), "has readable decompiled code") + "</div>" + bars(DEC, TD, varD, "Decoding");
  if (tracked.length) d += legend(DEC, TD, filter.d, varD, "d");
  let r = '<h3>Runtime</h3><p class="what">What the Rust/Bevy remake does with each function: Rust that replaces it, or the original PowerPC run in the remake\'s VM.</p>';
  if (!GM.runtime) r += '<p class="note">No runtime data yet: run <code>tools/ingest_runtime.py</code>.</p>';
  r += '<div class="headline">' + hl(pf(sumB(TR, LIVE_R), TR.bytes), "of the code runs in the remake, as Rust or as original code the VM has executed") +
    hl(pf(sumB(TR, RUST_R), TR.bytes), "is replaced by Rust (" + fmt(sumN(TR, RUST_R)) + " functions)") + "</div>" + bars(RUN, TR, varR, "Runtime") + legend(RUN, TR, filter.r, varR, "r");
  $("axD").innerHTML = d; $("axR").innerHTML = r;
  document.querySelectorAll(".lg").forEach(b => b.onclick = () => { toggle(b.dataset.w, b.dataset.k); $("h-fn").scrollIntoView({behavior: "smooth", block: "start"}); });
}
function toggle(w, k) {
  const set = w === "d" ? filter.d : filter.r;
  set.has(k) ? set.delete(k) : set.add(k);
  if (w !== view.axis) setAxis(w);
  shown = 300; progress(); syncChips(); renderList(); drawMap();
}

// ---- reconstruction ------------------------------------------------------------------------------------------------
function recon() {
  const R = GM.reconstruction;
  if (!R) return;
  const p = R.proof;
  const el = $("recon"); el.hidden = false;
  el.innerHTML = '<h2 id="h-recon">Rust / Bevy reconstruction <span class="meta">· ' + esc(R.updated) + "</span></h2>" +
    '<div class="panel recon"><p class="sub" style="margin:0 0 12px">' + esc(R.summary) + "</p>" +
    '<div class="cards">' + R.metrics.map(m => '<div class="card"><div class="k">' + esc(m.label) + '</div><div class="v num">' + esc(m.value) + '</div><div class="d">' + esc(m.detail) + "</div></div>").join("") + "</div>" +
    (p ? '<p class="note">Latest proof report: <b>' + esc(p.passed_count) + " / " + esc(p.total) + "</b> checks passed · " + esc(p.generated) + (p.passed ? " · all checks passed" : " · full validation not yet complete") + ".</p>" : "") +
    "<details><summary>Details and evidence</summary><ul>" + R.details.map(x => "<li>" + esc(x) + "</li>").join("") + '</ul><p class="note">' + esc(R.scope) + "</p><p>" +
    R.links.map(l => '<a href="' + esc(l.url) + '">' + esc(l.label) + "</a>").join(" · ") + "</p></details></div>";
}

// ---- history -------------------------------------------------------------------------------------------------------
function drawHistory() {
  const H = GM.history || [];
  const dec = view.axis === "d";
  const pick = h => {
    if (dec) return view.scope === "mw" || !h.bc ? null : {b: h.bc, s: h.s};
    if (!h.rbc) return null;
    if (view.scope === "ge") return {b: h.rbc, s: h.s};
    if (view.scope === "all") return {b: h.all.rbc, s: h.all.s};
    return {b: Object.fromEntries(RUN.map(s => [s.k, h.all.rbc[s.k] - h.rbc[s.k]])), s: h.all.s - h.s};
  };
  const pts = H.map(h => ({h, v: pick(h)})).filter(p => p.v);
  const el = $("hist");
  if (pts.length < 2) {
    el.innerHTML = '<p class="note">' + (dec && view.scope === "mw" ? "Middleware is not tracked for decoding." :
      "History of this measurement appears once at least two snapshots include it (<code>python3 GameMap/tools/build_site.py --snapshot \"label\"</code>)." +
      (dec ? "" : " Runtime snapshots start with the merged site.")) + "</p>";
    return;
  }
  const states = dec ? DEC.slice(0, 6) : RUN, colour = dec ? varD : varR;
  const W = 900, Hh = 230, pl = 44, pr = 12, pt = 10, pb = 34;
  const x = i => pl + (W - pl - pr) * i / (pts.length - 1), y = v => pt + (Hh - pt - pb) * (1 - v);
  let g = "";
  for (let t = 0; t <= 4; t++) g += '<line x1="' + pl + '" x2="' + (W - pr) + '" y1="' + y(t / 4) + '" y2="' + y(t / 4) + '" stroke="var(--rule)"/><text x="' + (pl - 6) + '" y="' + (y(t / 4) + 4) + '" text-anchor="end">' + t * 25 + "%</text>";
  const acc = pts.map(() => 0), paths = [];
  states.forEach(s => {
    const lo = acc.slice();
    pts.forEach((p, i) => acc[i] += (p.v.b[s.k] || 0) / p.v.s);
    const top = pts.map((p, i) => x(i) + "," + y(acc[i])).join(" L"), bot = pts.map((p, i) => x(pts.length - 1 - i) + "," + y(lo[pts.length - 1 - i])).join(" L");
    paths.push('<path d="M' + top + " L" + bot + ' Z" fill="' + colour(s.k) + '" stroke="var(--panel)" stroke-width="1"><title>' + esc(s.label) + "</title></path>");
  });
  const hits = pts.map((p, i) => {
    const w = (W - pl - pr) / (pts.length - 1);
    return '<rect x="' + (x(i) - w / 2) + '" y="' + pt + '" width="' + w + '" height="' + (Hh - pt - pb) + '" fill="transparent"><title>' + esc(p.h.label) + " (" + esc(p.h.date.slice(0, 10)) + ")\n" +
      states.map(s => s.label + ": " + pf(p.v.b[s.k] || 0, p.v.s)).join("\n") + "</title></rect>" +
      '<text x="' + x(i) + '" y="' + (Hh - 12) + '" text-anchor="' + (i === 0 ? "start" : i === pts.length - 1 ? "end" : "middle") + '">' + esc(p.h.date.slice(5, 10) + " " + p.h.date.slice(11, 16)) + "</text>";
  }).join("");
  el.innerHTML = '<svg viewBox="0 0 ' + W + " " + Hh + '" role="img" aria-label="Share of code by ' + (dec ? "decoding" : "runtime") + ' state over time">' + g + paths.join("") + hits + "</svg>" +
    '<p class="note" style="margin:6px 0 0">' + (dec ? "Decoding of game + engine code" : "Runtime") + ", share of bytes per snapshot. Hover a column for its label.</p>";
}

// ---- units grid ----------------------------------------------------------------------------------------------------
function renderUnits() {
  const q = $("unitq").value.toLowerCase(), sort = $("unitsort").value, dec = view.axis === "d";
  const done = u => dec ? u.doneD : u.doneR;
  let us = SU.filter(u => !q || u.name.toLowerCase().includes(q) || u.y.includes(q));
  us.sort(sort === "size" ? (a, b) => b.bytes - a.bytes : sort === "done" ? (a, b) => done(b) - done(a) || b.bytes - a.bytes :
    sort === "todo" ? (a, b) => done(a) - done(b) || b.bytes - a.bytes : (a, b) => a.a - b.a);
  const el = $("units"); el.innerHTML = "";
  for (const u of us.slice(0, 400)) {
    const d = document.createElement("button"); d.className = "unit"; d.type = "button";
    const vals = dec ? u.bd : u.br, states = dec ? DEC : RUN;
    d.innerHTML = '<span class="t" title="' + esc(u.name) + '">' + esc(u.name) + '</span><span class="bar">' + bar(states, vals, u.bytes, dec ? varD : varR) +
      '</span><span class="p num">' + esc(u.y) + " · " + u.funcs.length + " fn · " + kb(u.bytes) + " · " + (done(u) * 100).toFixed(0) + (dec ? "% proven" : "% running") + "</span>";
    d.onclick = () => { filter.unit = u.i; $("unitsel").value = u.i; shown = 300; renderList(); drawMap(); $("h-fn").scrollIntoView({behavior: "smooth"}); };
    el.appendChild(d);
  }
  if (!us.length) el.innerHTML = '<p class="note">No source files match.</p>';
}
$("unitq").oninput = renderUnits;
$("unitsort").onchange = renderUnits;

// ---- treemap -------------------------------------------------------------------------------------------------------
let rects = [], mapW = 0, mapH = 0;
function squarify(items, x, y, w, h, out) {
  const total = items.reduce((a, b) => a + b.v, 0);
  if (!total || w <= 0 || h <= 0) return;
  let i = 0;
  while (i < items.length) {
    const remaining = items.slice(i).reduce((a, b) => a + b.v, 0);
    const horiz = w >= h, side = horiz ? h : w, scale = (w * h) / remaining;
    let row = [], best = Infinity, sum = 0;
    for (let j = i; j < items.length; j++) {
      const v = items[j].v * scale, ns = sum + v, rr = row.concat(v);
      const len = ns / side, worst = Math.max(...rr.map(a => Math.max(len * len / a, a / (len * len))));
      if (worst > best && row.length) break;
      row = rr; sum = ns; best = worst;
    }
    const len = sum / side;
    let off = 0;
    for (let k = 0; k < row.length; k++) {
      const it = items[i + k], l = row[k] / len;
      if (horiz) out.push({it, x, y: y + off, w: len, h: l}); else out.push({it, x: x + off, y, w: l, h: len});
      off += l;
    }
    i += row.length;
    if (horiz) { x += len; w -= len; } else { y += len; h -= len; }
  }
}
function layoutMap() {
  const cv = $("map"), r = cv.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
  mapW = r.width; mapH = r.height; cv.width = Math.round(mapW * dpr); cv.height = Math.round(mapH * dpr);
  const urs = [];
  squarify(SU.slice().sort((a, b) => b.bytes - a.bytes).map(u => ({v: u.bytes, u})), 0, 0, mapW, mapH, urs);
  rects = urs.map(ur => {
    const pad = ur.w > 12 && ur.h > 12 ? 1.5 : 0, fr = [];
    squarify(ur.it.u.funcs.slice().sort((a, b) => b.s - a.s).map(f => ({v: f.s, f})), ur.x + pad, ur.y + pad, ur.w - 2 * pad, ur.h - 2 * pad, fr);
    return {unit: ur.it.u, x: ur.x, y: ur.y, w: ur.w, h: ur.h, funcs: fr};
  });
  $("maphint").textContent = "Source files as blocks, largest first; each function is a cell sized by its code bytes and coloured by its " +
    (view.axis === "d" ? "decoding" : "runtime") + " state. Hover to identify a function; click to open it below.";
}
function matches(f) {
  if (filter.d.size && !filter.d.has(f.st)) return false;
  if (filter.r.size && !filter.r.has(f.rst)) return false;
  if (filter.grp >= 0 && f.grp !== filter.grp) return false;
  if (filter.unit >= 0 && f.unit !== filter.unit) return false;
  if (filter.q) { const q = filter.q; if (!f.l.includes(q) && !hex(f.a).includes(q.replace(/^0x/, ""))) return false; }
  return true;
}
const filtering = () => filter.d.size || filter.r.size || filter.grp >= 0 || filter.unit >= 0 || filter.q;
function drawMap() {
  const cv = $("map"), g = cv.getContext("2d"), dpr = window.devicePixelRatio || 1;
  g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, mapW, mapH);
  const dec = view.axis === "d";
  const col = Object.fromEntries((dec ? DEC : RUN).map(s => [s.k, css((dec ? "--d-" : "--s-") + s.k)]));
  const dim = filtering();
  g.fillStyle = css("--rule"); g.fillRect(0, 0, mapW, mapH);
  for (const ur of rects) for (const fr of ur.funcs) {
    const f = fr.it.f;
    g.globalAlpha = !dim || matches(f) ? 1 : 0.16;
    g.fillStyle = col[dec ? f.st : f.rst];
    g.fillRect(fr.x, fr.y, Math.max(fr.w - (fr.w > 3 ? 0.5 : 0), 0.3), Math.max(fr.h - (fr.h > 3 ? 0.5 : 0), 0.3));
  }
  g.globalAlpha = 1; g.strokeStyle = css("--panel"); g.lineWidth = 1.5;
  for (const ur of rects) g.strokeRect(ur.x + .75, ur.y + .75, ur.w - 1.5, ur.h - 1.5);
  if (selected >= 0) for (const ur of rects) for (const fr of ur.funcs) if (fr.it.f.i === selected) {
    g.strokeStyle = css("--ink"); g.lineWidth = 2; g.strokeRect(fr.x - 1, fr.y - 1, Math.max(fr.w + 2, 6), Math.max(fr.h + 2, 6));
  }
}
function hit(x, y) {
  for (const ur of rects) {
    if (x < ur.x || y < ur.y || x > ur.x + ur.w || y > ur.y + ur.h) continue;
    for (const fr of ur.funcs) if (x >= fr.x && y >= fr.y && x <= fr.x + fr.w && y <= fr.y + fr.h) return {ur, fr};
    return {ur, fr: null};
  }
  return null;
}
const cvs = $("map"), tip = $("tip");
cvs.addEventListener("mousemove", e => {
  const r = cvs.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top, h = hit(x, y);
  if (!h) { tip.hidden = true; return; }
  const u = h.ur.unit, f = h.fr && h.fr.it.f;
  tip.innerHTML = (f ? "<div><b>" + esc(f.n) + '</b></div><div class="m">0x' + hex(f.a) + " · " + f.s + " bytes · " + DEC[DI[f.st]].label + " · " + RUN[RI[f.rst]].label +
    (f.entries ? " · entered " + fmt(f.entries) + "×" : "") + "</div>" : "") +
    '<div class="m">' + esc(u.name) + " — " + (u.doneD * 100).toFixed(0) + "% proven · " + (u.doneR * 100).toFixed(0) + "% running · " + kb(u.bytes) + "</div>";
  tip.hidden = false;
  const tw = tip.offsetWidth, th = tip.offsetHeight;
  tip.style.left = Math.max(4, Math.min(x + 14, mapW - tw - 4)) + "px"; tip.style.top = (y + 18 + th > mapH ? y - th - 10 : y + 18) + "px";
});
cvs.addEventListener("mouseleave", () => tip.hidden = true);
cvs.addEventListener("click", e => { const r = cvs.getBoundingClientRect(), h = hit(e.clientX - r.left, e.clientY - r.top); if (h && h.fr) select(h.fr.it.f.i, true); });
let rt;
try { matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => drawMap()); } catch (e) { /* old browsers */ }
window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(() => { layoutMap(); drawMap(); }, 150); });

// ---- filters and list ----------------------------------------------------------------------------------------------
function buildChips() {
  const mk = (id, title, states, w, colour) => {
    const el = $(id);
    el.innerHTML = '<span class="t">' + title + "</span>" + states.map(s => '<button type="button" class="chip" data-w="' + w + '" data-k="' + s.k + '"><i style="background:' + colour(s.k) + '"></i>' + esc(s.label) + "</button>").join("");
  };
  mk("chipsD", "Decoding", DEC, "d", varD); mk("chipsR", "Runtime", RUN, "r", varR);
  document.querySelectorAll(".chip").forEach(b => b.onclick = () => toggle(b.dataset.w, b.dataset.k));
}
function syncChips() {
  document.querySelectorAll(".chip").forEach(b => b.classList.toggle("on", (b.dataset.w === "d" ? filter.d : filter.r).has(b.dataset.k)));
  document.querySelectorAll(".lg").forEach(b => b.classList.toggle("on", (b.dataset.w === "d" ? filter.d : filter.r).has(b.dataset.k)));
  $("chipsD").hidden = view.scope === "mw";
}
function buildSelects() {
  const grps = [...new Set(SF.map(f => f.grp))].map(i => ({i, label: GM.groups[i].t + " / " + GM.groups[i].y})).sort((a, b) => a.label.localeCompare(b.label));
  $("subsel").innerHTML = '<option value="-1">All subsystems</option>' + grps.map(g => '<option value="' + g.i + '">' + esc(g.label) + "</option>").join("");
  $("unitsel").innerHTML = '<option value="-1">All source files</option>' + SU.slice().sort((a, b) => a.name.localeCompare(b.name)).map(u => '<option value="' + u.i + '">' + esc(u.name) + " (" + u.funcs.length + ")</option>").join("");
  if (!grps.some(g => g.i === filter.grp)) filter.grp = -1;
  if (!SU.some(u => u.i === filter.unit)) filter.unit = -1;
  $("subsel").value = filter.grp; $("unitsel").value = filter.unit;
}
$("subsel").onchange = e => { filter.grp = +e.target.value; shown = 300; renderList(); drawMap(); };
$("unitsel").onchange = e => { filter.unit = +e.target.value; shown = 300; renderList(); drawMap(); };
$("q").oninput = e => { filter.q = e.target.value.trim().toLowerCase(); shown = 300; renderList(); drawMap(); };
$("sort").onchange = e => { filter.sort = e.target.value; shown = 300; renderList(); };
function renderList() {
  const fs = SF.filter(matches), s = filter.sort;
  if (s === "size") fs.sort((a, b) => b.s - a.s); else if (s === "fi") fs.sort((a, b) => b.fi - a.fi || b.s - a.s);
  else if (s === "entries") fs.sort((a, b) => b.entries - a.entries); else if (s === "refs") fs.sort((a, b) => b.refs - a.refs || b.s - a.s);
  $("count").textContent = fmt(fs.length) + " functions · " + kb(fs.reduce((a, f) => a + f.s, 0));
  const el = $("list"); el.innerHTML = "";
  const frag = document.createDocumentFragment();
  for (const f of fs.slice(0, shown)) {
    const d = document.createElement("div"); d.className = "row" + (f.i === selected ? " sel" : ""); d.tabIndex = 0; d.setAttribute("role", "option");
    d.innerHTML = '<span class="st" style="background:' + varD(f.st) + '" title="Decoding: ' + DEC[DI[f.st]].label + '"></span><span class="st" style="background:' + varR(f.rst) + '" title="Runtime: ' + RUN[RI[f.rst]].label +
      '"></span><span style="min-width:0"><div class="n">' + esc(f.n) + '</div><div class="u">' + esc(UNITS[f.unit].name) + " · " + DEC[DI[f.st]].label + " · " + RUN[RI[f.rst]].label +
      '</div></span><span class="z">' + hex(f.a) + "<br>" + f.s + " B</span>";
    d.onclick = () => select(f.i, false); d.onkeydown = e => { if (e.key === "Enter") select(f.i, false); };
    frag.appendChild(d);
  }
  el.appendChild(frag);
  if (fs.length > shown) {
    const b = document.createElement("button"); b.className = "more"; b.type = "button"; b.textContent = "Show " + Math.min(300, fs.length - shown) + " more";
    b.onclick = () => { shown += 300; renderList(); }; el.appendChild(b);
  }
  if (!fs.length) el.innerHTML = '<div class="empty">No functions match.</div>';
}

// ---- detail --------------------------------------------------------------------------------------------------------
const cache = new Map();
function getJSON(url) {
  if (!cache.has(url)) cache.set(url, fetch(url).then(r => r.ok ? r.json() : null).catch(() => null));
  return cache.get(url);
}
const codeFor = f => getJSON("code/" + (f.a >>> 14).toString(16).padStart(5, "0") + ".json").then(p => p ? (p[hex(f.a)] || {}) : null);
const rustFor = f => f.refs ? getJSON("rust/" + (f.a >>> 16).toString(16).padStart(4, "0") + ".json").then(p => (p && p[hex(f.a)]) || []) : Promise.resolve([]);
const NOPACK = '<div class="empty">Code is not part of this site: it is EA\'s. If you own the game, build the local <b>code pack</b> from your copy of the executable, then serve the site folder:<br>' +
  "<code>python3 GameMap/tools/build_code_pack.py --elf path/to/playgroundz.elf --ghidra-c path/to/decomp.c</code><br><code>python3 -m http.server -d GameMap/site</code></div>";
let cMode = null;

async function select(i, scroll) {
  selected = i; drawMap();
  document.querySelectorAll(".row").forEach(r => r.classList.remove("sel"));
  const f = F[i], u = UNITS[f.unit], d = DEC[DI[f.st]], r = RUN[RI[f.rst]];
  try { history.replaceState(null, "", "#f" + hex(f.a)); } catch (e) { /* file:// */ }
  const scen = (GM.scenarios || []).map((n, k) => (f.scen >> k) & 1 ? "<span>" + esc(n) + "</span>" : "").join("");
  const fact = (k, v) => '<span class="fact">' + k + " <b>" + v + "</b></span>";
  const facts = [fact("Address", '<span class="mono">0x' + hex(f.a) + "</span>"), fact("Size", f.s + " B") + "", fact("File", esc(u.name)), fact("Subsystem", esc(f.t + " / " + f.y))];
  if (f.kind) facts.push(fact("Kind", esc(f.kind)));
  if (f.st !== "untracked") facts.push(fact("Callers / callees", f.fi + " / " + f.fo));
  facts.push(fact("VM", VMTXT[f.vm] || esc(f.vm)), fact("Entered", fmt(f.entries) + "×"));
  if (f.gh) facts.push(fact("Ghidra", f.gh[0] + " lines · " + f.gh[1] + " loops · " + f.gh[2] + " gotos · " + f.gh[4] + " warnings"));
  if (f.lf) facts.push(fact("Lift check", fmt(f.lf[0]) + " trials matched · " + f.lf[2] + "/" + f.lf[1] + " blocks"));
  if (f.why) facts.push(fact("Blocker", esc(f.why)));
  if (f.ev && f.ev.verified_by) facts.push(fact("Verified by", esc(f.ev.verified_by)));
  if (f.ev && f.ev.doc) facts.push(fact("Documented in", '<span class="mono">GameMap/' + esc(f.ev.doc) + "</span>"));
  const det = $("detail");
  det.innerHTML = '<div class="panel dh"><div class="name">' + esc(f.n) + "<span>(" + esc(f.g) + ")</span></div>" + (f.m !== f.n ? '<div class="mg">' + esc(f.m) + "</div>" : "") +
    (f.sum ? '<p class="sum">' + esc(f.sum) + "</p>" : "") +
    '<div class="facts"><span class="badge" title="' + esc(d.desc) + '"><i style="background:' + varD(f.st) + '"></i><small>Decoding</small> ' + d.label + "</span>" +
    '<span class="badge" title="' + esc(r.desc) + '"><i style="background:' + varR(f.rst) + '"></i><small>Runtime</small> ' + r.label + "</span></div>" +
    '<div class="facts">' + facts.join("") + "</div>" + (scen ? '<div class="scen" aria-label="Scenarios that entered it">' + scen + "</div>" : "") + "</div>" +
    '<div class="panes"><div class="pane"><h3>PowerPC (original)<span>' + f.s / 4 + ' instr</span></h3><div id="pAsm" class="empty">Loading…</div></div>' +
    '<div class="pane"><h3 id="cHead">Decompiled C</h3><div id="pC" class="empty">Loading…</div></div>' +
    '<div class="pane rust"><h3>Rust<span>' + f.refs + " reference" + (f.refs === 1 ? "" : "s") + '</span></h3><div id="pR" class="empty">Loading…</div></div></div>';
  if (scroll) det.scrollIntoView({behavior: "smooth", block: "start"});
  const [code, rust] = await Promise.all([codeFor(f), rustFor(f)]);
  if (selected !== i) return;
  // PowerPC
  const pa = $("pAsm");
  if (!code) { pa.outerHTML = NOPACK; } else if (code.asm) {
    const pre = document.createElement("pre"); pre.className = "asm";
    pre.innerHTML = code.asm.split("\n").map(l => {
      const m = l.match(/^([0-9a-f]{8}) {2}([0-9a-f]{8}) {2}(\S+)\s*(.*)$/); if (!m) return esc(l);
      const ops = esc(m[4]).replace(/(\.L_[0-9a-f]{8}|[A-Za-z_][A-Za-z0-9_]*__[A-Za-z0-9_]+)/g, '<span class="lb">$1</span>');
      return '<span class="ad">' + m[1] + '</span>  <span class="wd">' + m[2] + '</span>  <span class="mn">' + m[3].padEnd(8) + "</span> " + ops;
    }).join("\n");
    pa.replaceWith(pre);
  }
  // C: Ghidra or lifted
  const showC = () => {
    const have = code || {}, opts = [["lifted", "Lifted C"], ["ghidra", "Ghidra C"]].filter(([k]) => have[k]);
    const prefer = ["reviewed", "proven", "partial"].includes(f.st) ? "lifted" : "ghidra";
    const mode = opts.some(o => o[0] === cMode) ? cMode : opts.some(o => o[0] === prefer) ? prefer : (opts[0] || [null])[0];
    $("cHead").innerHTML = (opts.length > 1 ? '<span class="seg">' + opts.map(([k, t]) => '<button type="button" data-c="' + k + '" class="' + (k === mode ? "on" : "") + '">' + t + "</button>").join("") + "</span>" : (mode === "lifted" ? "Lifted C" : "Ghidra C")) +
      (mode === "lifted" && (f.st === "proven" || f.st === "reviewed") ? '<span class="proofok">✓ proven equivalent</span>' : mode ? "<span>" + (mode === "ghidra" ? esc(have.gsrc || "decompiled") + " · reading aid" : "not proven") + "</span>" : "");
    $("cHead").querySelectorAll("button").forEach(b => b.onclick = () => { cMode = b.dataset.c; showC(); });
    const box = $("pC");
    if (!code) { box.outerHTML = '<div id="pC">' + NOPACK + "</div>"; return; }
    if (!mode) { box.className = "empty"; box.innerHTML = "No decompiled C for this function in the code pack."; return; }
    const pre = document.createElement("pre"), c = document.createElement("code"); c.className = "language-c"; c.textContent = have[mode];
    pre.appendChild(c); box.className = ""; box.innerHTML = ""; box.appendChild(pre);
    if (window.hljs && have[mode].length < 80000) window.hljs.highlightElement(c);
  };
  showC();
  // Rust
  const pr = $("pR");
  if (!rust.length) {
    pr.className = "empty";
    pr.textContent = {run: "No Rust is written for this function: the remake runs the original PowerPC in its VM.",
      native: "No Rust yet, and no recorded scenario has reached it. It would run as original code in the VM.",
      stub: "No Rust needed: the VM skips it (rendering or audio detail) and returns 0.",
      trap: "No Rust yet: an engine service the VM cannot run. A run that needs it stops here."}[f.rst] || "Bound to Rust by a general rule rather than by name.";
    return;
  }
  pr.className = ""; pr.innerHTML = "";
  for (const ref of rust) {
    const box = document.createElement("div"); box.className = "ref";
    box.innerHTML = '<div class="refh"><span class="kind">' + esc(KIND[ref.kind] || ref.kind) + '</span><a href="' + GM.repo + esc(ref.file) + "#L" + ref.start + '" target="_blank" rel="noopener">' + esc(ref.file) + ":" + ref.line + "</a>" +
      (ref.fn ? '<span class="mono" style="font-size:12px;color:var(--muted)">fn ' + esc(ref.fn) + "</span>" : "") + "</div>";
    const pre = document.createElement("pre"), c = document.createElement("code"); c.className = "language-rust"; c.textContent = ref.code;
    pre.appendChild(c); box.appendChild(pre); pr.appendChild(box);
    if (window.hljs) window.hljs.highlightElement(c);
  }
}

// ---- next up -------------------------------------------------------------------------------------------------------
function nextUp() {
  const tracked = SF.filter(f => f.st !== "untracked");
  const notDone = tracked.filter(f => !DONE_D.includes(f.st));
  const tbl = (title, sub, rows, cols) => '<div class="panel"><h3>' + title + "</h3>" + (sub ? '<p class="note" style="margin:4px 0 8px">' + sub + "</p>" : "") +
    (rows.length ? '<div class="tblwrap"><table><thead><tr><th>Function</th>' + cols.map(c => '<th class="' + (c[2] || "") + '">' + c[0] + "</th>").join("") + "</tr></thead><tbody>" +
      rows.map(f => '<tr class="click" data-i="' + f.i + '"><td class="mono">' + esc(f.n) + "</td>" + cols.map(c => '<td class="' + (c[2] || "") + '">' + c[1](f) + "</td>").join("") + "</tr>").join("") + "</tbody></table></div>"
      : '<p class="note">Nothing in this scope.</p>') + "</div>";
  const blk = new Map();
  for (const f of tracked) if (["decompiled", "flagged", "mapped"].includes(f.st) && f.why) { const b = blk.get(f.why) || {n: 0, s: 0}; b.n++; b.s += f.s; blk.set(f.why, b); }
  const rest = notDone.reduce((t, f) => t + f.s, 0);
  const blockers = '<div class="panel"><h3>Why functions are not proven yet</h3><p class="note" style="margin:4px 0 8px">Blockers from the last lifter and Ghidra run, game + engine code.</p>' +
    (blk.size ? '<div class="tblwrap"><table><thead><tr><th>Blocker</th><th class="num">Functions</th><th class="num">Code</th><th class="num">Share of the rest</th></tr></thead><tbody>' +
      [...blk].sort((a, b) => b[1].s - a[1].s).map(([w, b]) => "<tr><td>" + esc(w) + '</td><td class="num">' + fmt(b.n) + '</td><td class="num">' + kb(b.s) + '</td><td class="num">' + pf(b.s, rest) + "</td></tr>").join("") + "</tbody></table></div>"
      : '<p class="note">Nothing in this scope.</p>') + "</div>";
  const quick = notDone.filter(f => f.st === "decompiled" && f.kind !== "loop" && f.s <= 160 && !f.why.startsWith("lift: loop")).sort((a, b) => b.fi - a.fi).slice(0, 25);
  const hot = notDone.slice().sort((a, b) => b.fi - a.fi).slice(0, 25);
  const gaps = SF.filter(f => f.rst === "trap").sort((a, b) => b.entries - a.entries || b.s - a.s).slice(0, 25);
  $("next").innerHTML = blockers +
    tbl("Quick wins", "Small, loop-free, already decompiled, most-called first: the cheapest to push to proven.", quick, [["Callers", f => f.fi, "num"], ["Size", f => f.s, "num"]]) +
    tbl("Most-called functions not proven yet", "", hot, [["Callers", f => f.fi, "num"], ["State", f => DEC[DI[f.st]].label], ["Blocker", f => esc(f.why)]]) +
    tbl("Runtime gaps: unimplemented services", "Functions the remake's VM traps on. A scenario that needs one stops there; each needs a Rust hook or a port.", gaps,
      [["Size", f => f.s, "num"], ["File", f => esc(UNITS[f.unit].name)]]);
  $("next").querySelectorAll("tr[data-i]").forEach(r => r.onclick = () => select(+r.dataset.i, true));
}

// ---- method --------------------------------------------------------------------------------------------------------
function method() {
  const rows = (states, colour) => "<table><tbody>" + states.map(s => '<tr><td style="white-space:nowrap"><span class="badge"><i style="background:' + colour(s.k) + '"></i>' + esc(s.label) + "</span></td><td>" + esc(s.desc) + "</td></tr>").join("") + "</tbody></table>";
  $("method").innerHTML = "<p>Matching-decompilation sites like decomp.dev count bytes of C that recompile to identical machine code. This project has no compiler-matching goal: the executable is " +
    "<em>mapped and decoded</em> for a clean-room reconstruction in Rust/Bevy. So each function gets two independent measurements.</p>" +
    '<div class="grid2"><div><h3>Decoding</h3><p class="note">Highest applicable state wins. Game + engine code only.</p>' + rows(DEC, varD) +
    '</div><div><h3>Runtime</h3><p class="note">One state per function, first match in this order.</p>' + rows(RUN, varR) + "</div></div>" +
    "<h3>Proof</h3><p>A function is <b>proven</b> when its lifted pseudo-C, produced by symbolic execution of the PowerPC into a guarded IR, behaves identically to the original machine code on randomized inputs run under a PowerPC 750 emulator (Unicorn). " +
    "Compared: return registers, every memory write, every call target and argument, and FP results. Every block must be exercised; otherwise the function stays <b>partial</b>. " +
    "The lifter/verifier run reached " + fmt(GM.lift_tested || 0) + " functions in the last snapshot; the rest are shown by their Ghidra state only (not attempted, not failed).</p>" +
    "<h3>Runtime</h3><p>The remake runs the original executable in its own PowerPC VM (<span class=\"mono\">_bevy/src/gekko</span>, <span class=\"mono\">mgvm</span>) and replaces engine services with Rust. " +
    "<span class=\"mono\">mglab classify</span> lists every hook the VM installs; runs with <span class=\"mono\">EAGL_PPC_COVER</span> record which functions were entered. Scenarios recorded: " +
    ((GM.scenarios || []).map(esc).join(", ") || "none") + ". Rust references are found in <span class=\"mono\">_bevy/src</span> by hook bindings, cited addresses and names.</p>" +
    "<h3>Scope</h3><p>Compilation units come from the original link order (<span class=\"mono\">STT_FILE</span> symbols). Decoding percentages cover game and engine code; middleware (Wii SDK, Havok, nw4r, Lua, the APT UI runtime, EA libraries) is listed for size. " +
    "The runtime measurement covers every function. Percentages are by function size in bytes unless stated.</p>" +
    "<h3>Caveats</h3><ul><li>\"Decompiled\" comes from Ghidra and may be wrong; only <b>proven</b> and <b>reviewed</b> mean checked.</li>" +
    "<li>\"Ran original code\" means entered at least once in a recorded scenario, not that every path ran.</li><li>Indirect and virtual calls are not in the call graph.</li></ul>" +
    "<h3>Viewing code</h3><p>This site contains no game code. If you own the game, build the local code pack (<span class=\"mono\">tools/build_code_pack.py</span>) against your own ELF; a local <span class=\"mono\">site/code/</span> folder then fills the PowerPC and C panes. The Rust panes always work: that code is the remake's own.</p>" +
    '<p class="note">Data generated ' + esc(GM.generated) + ' · ELF sha256 <span class="mono">' + esc(GM.elf_sha256 || "") + "</span></p>";
}

// ---- switches ------------------------------------------------------------------------------------------------------
function syncSeg() {
  document.querySelectorAll("#scope button").forEach(b => b.classList.toggle("on", b.dataset.v === view.scope));
  document.querySelectorAll("#axis button").forEach(b => b.classList.toggle("on", b.dataset.v === view.axis));
}
function setAxis(v) { view.axis = v; store.set("axis", v); syncSeg(); drawHistory(); renderUnits(); layoutMap(); drawMap(); }
function setScope(v) {
  view.scope = v; store.set("scope", v);
  if (v === "mw") { filter.d.clear(); if (view.axis === "d") view.axis = "r"; }
  rebuild(); syncSeg(); progress(); syncChips(); buildSelects(); drawHistory(); renderUnits(); layoutMap(); drawMap(); shown = 300; renderList(); nextUp();
  $("meta").textContent = fmt(SF.length) + " functions · " + kb(SF.reduce((t, f) => t + f.s, 0)) + " · data " + GM.generated.slice(0, 10);
}
document.querySelectorAll("#scope button").forEach(b => b.onclick = () => setScope(b.dataset.v));
document.querySelectorAll("#axis button").forEach(b => b.onclick = () => setAxis(b.dataset.v));

// ---- boot ----------------------------------------------------------------------------------------------------------
buildChips(); recon(); method();
setScope(view.scope);
$("foot").innerHTML = "Reference map of the Wii executable <span class=\"mono\">playgroundz.elf</span> (" + fmt(F.length) + " functions in " + fmt(UNITS.length) + " source files). " +
  "Progress is measured, never estimated: decoding from GameMap's Ghidra, lifter and annotation data; runtime from the remake's VM hook table and coverage of " + (GM.scenarios || []).length + " recorded scenarios.";
function fromHash() {
  const h = location.hash.match(/^#(?:f|\/fn\/)([0-9a-f]{8})$/i), f = h && byAddr.get(parseInt(h[1], 16));
  if (f && f.i !== selected) { if (!inScope(f)) setScope("all"); select(f.i, true); }
}
window.addEventListener("hashchange", fromHash);
fromHash();
})();
