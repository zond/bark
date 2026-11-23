(() => {
// Six relays on power round an empty middle, and eight sleepy nodes, each anchored to the relay nearest it.
const R = [[60,70],[190,45],[330,70],[350,215],[210,250],[65,205]];
const S = [[20,25],[110,120],[250,10],[390,115],[285,160],[390,280],[150,285],[20,270]];
const P = R.concat(S), N = R.length, RANGE = 150, RED = '#c62828';
const c = document.getElementById('mesh'), g = c.getContext('2d');
const d = (a, b) => Math.hypot(P[a][0] - P[b][0], P[a][1] - P[b][1]);
const relays = R.map((_, i) => i);
const nb = relays.map(i => relays.filter(j => j != i && d(i, j) < RANGE));
const anchor = i => i < N ? i : relays.reduce((a, b) => d(i, a) < d(i, b) ? a : b);
// A beat every 1 to 3 s for a relay, 6 to 9 s for a sleepy node: the spec's periods scaled.
const beat = P.map((_, i) => { const e = i < N ? 1 + 2 * Math.random() : 6 + 3 * Math.random(); return { e, at: e * Math.random() }; });
const nextBeat = (i, t) => { const b = beat[i]; return b.at + Math.ceil((t - b.at) / b.e) * b.e; };
// Relay to relay: each hop to the untried neighbour nearest the destination, back to the parent at a dead end.
function route(s, t) {
  const hops = [], seen = new Set([s]), st = [s];
  while (st.length) {
    const u = st[st.length - 1];
    if (u == t) return hops;
    const n = nb[u].filter(v => !seen.has(v)).sort((a, b) => d(a, t) - d(b, t))[0];
    if (n === undefined) { st.pop(); if (st.length) hops.push([u, st[st.length - 1]]); continue; }
    seen.add(n); st.push(n); hops.push([u, n]);
  }
  return [];
}
// A sleepy sender hands its message to its anchor; the recipient's anchor holds it until the recipient beats.
function message(a, b) {
  const steps = [], A = anchor(a), B = anchor(b);
  if (a != A) steps.push([a, A]);
  steps.push(...route(A, B));
  if (b != B) steps.push(B, [B, b]);
  return steps;
}
let W, msg = null, next = 1, sleepyNext = false;
function size() {
  const r = devicePixelRatio || 1; W = c.clientWidth;
  c.width = W * r; c.height = W * .75 * r; g.setTransform(r * W / 400, 0, 0, r * W / 400, 0, 0);
}
function pick(t) {
  const pool = sleepyNext ? S.map((_, i) => N + i) : relays;
  let a, b;
  do { a = pool[Math.floor(Math.random() * pool.length)]; b = pool[Math.floor(Math.random() * pool.length)]; }
  while (anchor(a) == anchor(b) || nb[anchor(a)].includes(anchor(b)) && !sleepyNext);
  sleepyNext = !sleepyNext;
  msg = { to: b, steps: message(a, b), i: 0, t0: t };
}
const HOP = .6, ACK = .3;
const line = (a, b) => { g.beginPath(); g.moveTo(...P[a]); g.lineTo(...P[b]); g.stroke(); };
function draw(t, still) {
  g.clearRect(0, 0, 400, 300);
  g.lineWidth = 1; g.strokeStyle = '#e2e2e2';
  nb.forEach((n, i) => n.forEach(j => j > i && line(i, j)));
  g.setLineDash([3, 3]); S.forEach((_, k) => line(N + k, anchor(N + k))); g.setLineDash([]);
  if (!still) beat.forEach((b, i) => {
    const age = (t - b.at) % b.e;
    if (t > b.at && age < 1) { g.strokeStyle = `rgba(136,136,136,${(i < N ? 1 : .6) * (1 - age)})`; g.beginPath(); g.arc(...P[i], 6 + 30 * age, 0, 7); g.stroke(); }
  });
  let glow = -1;
  while (msg) {
    const s = msg.steps[msg.i], f = t - msg.t0;
    if (s === undefined) { if (f < 1.5) glow = msg.to; else msg = null; break; }
    if (typeof s == 'number') {
      msg.until ??= nextBeat(msg.to, t);
      if (t < msg.until) { g.fillStyle = '#888'; g.fillRect(P[s][0] + 6, P[s][1] - 10, 5, 5); break; }
      msg.until = undefined; msg.i++; msg.t0 = t; continue;
    }
    if (f > HOP + ACK) { msg.i++; msg.t0 = t; continue; }
    const [u, v] = s; g.strokeStyle = g.fillStyle = RED;
    if (f < HOP) { const q = f / HOP; g.beginPath(); g.arc(P[u][0] + (P[v][0] - P[u][0]) * q, P[u][1] + (P[v][1] - P[u][1]) * q, 4, 0, 7); g.fill(); }
    else { g.globalAlpha = 1 - (f - HOP) / ACK; g.lineWidth = 2; line(v, u); g.globalAlpha = 1; }
    break;
  }
  P.forEach((p, i) => {
    g.fillStyle = i == glow ? RED : i < N ? '#777' : '#c4c4c4';
    g.beginPath(); g.arc(...p, i == glow ? 7 : i < N ? 5 : 4, 0, 7); g.fill();
  });
}
function still() {
  draw(0, true);
  g.strokeStyle = RED; g.lineWidth = 2; g.setLineDash([4, 4]);
  message(N + 1, N + 4).forEach(s => typeof s != 'number' && line(...s));
  g.setLineDash([]);
}
const calm = matchMedia('(prefers-reduced-motion: reduce)');
function frame(ms) {
  const t = ms / 1000;
  if (calm.matches) return still();
  if (!msg && t > next) { pick(t); next = t + 2 + 2 * Math.random(); }
  draw(t, false); requestAnimationFrame(frame);
}
addEventListener('resize', () => { size(); if (calm.matches) still(); });
size(); calm.matches ? still() : requestAnimationFrame(frame);
calm.addEventListener('change', () => calm.matches ? still() : requestAnimationFrame(frame));
})();
