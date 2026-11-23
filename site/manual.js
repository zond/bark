// The manual's small animations, in the front page's manner: grey nodes,
// a red message, and its hop confirmed by a red line back. Each runs
// only while it is on screen, and is a still picture for a reader who
// asked for less motion.
(() => {
// What the pictures' labels say, in the page's language.
const T = {
  en: { relay: 'relay', sleepy: 'sleepy node', paired: 'paired', open: 'public node', guests: 'guests', place: 'the place', dead: 'dead end', home: 'your home', you: 'you', friend: 'a friend', version: 'new version' },
  sv: { relay: 'relä', sleepy: 'sömnig nod', paired: 'parkopplad', open: 'publik nod', guests: 'gäster', place: 'platsen', dead: 'återvändsgränd', home: 'ditt hem', you: 'du', friend: 'en vän', version: 'ny version' },
  uk: { relay: 'ретранслятор', sleepy: 'сонний вузол', paired: 'спарений', open: 'публічний вузол', guests: 'гості', place: 'місце', dead: 'глухий кут', home: 'ваш дім', you: 'ви', friend: 'друг', version: 'нова версія' },
}[document.documentElement.lang];
const RED = '#c62828', GREY = '#777', PALE = '#c4c4c4', LINK = '#e2e2e2';
const HOP = .6, ACK = .3, STEP = HOP + ACK;
const calm = matchMedia('(prefers-reduced-motion: reduce)');
// A picture 400 by 200 that repeats every `period` seconds; `stillAt` is
// the moment shown without motion.
function scene(id, period, stillAt, draw) {
  const c = document.getElementById(id), g = c.getContext('2d');
  let seen = false, running = false;
  const size = () => {
    const r = devicePixelRatio || 1, w = c.clientWidth;
    c.width = w * r; c.height = w / 2 * r; g.setTransform(r * w / 400, 0, 0, r * w / 400, 0, 0);
  };
  const paint = t => { g.clearRect(0, 0, 400, 200); draw(g, t); };
  const frame = ms => {
    if (!seen || calm.matches) { running = false; return; }
    paint(ms / 1000 % period); requestAnimationFrame(frame);
  };
  const start = () => {
    size(); paint(stillAt);
    if (seen && !calm.matches && !running) { running = true; requestAnimationFrame(frame); }
  };
  new IntersectionObserver(e => { seen = e[0].isIntersecting; start(); }).observe(c);
  addEventListener('resize', start); calm.addEventListener('change', start);
  start();
}
const line = (g, a, b) => { g.beginPath(); g.moveTo(...a); g.lineTo(...b); g.stroke(); };
const links = (g, pairs, dashed) => {
  g.lineWidth = 1; g.strokeStyle = LINK; g.setLineDash(dashed ? [3, 3] : []);
  pairs.forEach(([a, b]) => line(g, a, b)); g.setLineDash([]);
};
const dot = (g, p, r, colour) => { g.fillStyle = colour; g.beginPath(); g.arc(...p, r, 0, 7); g.fill(); };
const relay = (g, p, glow) => dot(g, p, glow ? 7 : 5, glow ? RED : GREY);
const sleepy = (g, p, glow) => dot(g, p, glow ? 7 : 4, glow ? RED : PALE);
const phone = (g, p, colour = GREY) => {
  g.fillStyle = '#fff'; g.fillRect(p[0] - 4, p[1] - 7, 8, 14);
  g.lineWidth = 1.5; g.strokeStyle = colour; g.strokeRect(p[0] - 4, p[1] - 7, 8, 14);
};
// A node's announcement: a ring that widens and fades, every `every`
// seconds from `at`.
const ring = (g, p, t, every, at, strength = 1) => {
  const age = ((t - at) % every + every) % every;
  if (age < 1) {
    g.lineWidth = 1; g.strokeStyle = `rgba(136,136,136,${strength * (1 - age)})`;
    g.beginPath(); g.arc(...p, 6 + 30 * age, 0, 7); g.stroke();
  }
};
// A message that leaves at `t0` along `pts`, each hop confirmed; a
// request is drawn hollow. Returns when it has arrived.
function travel(g, t, t0, pts, hollow) {
  const n = pts.length - 1, f = t - t0;
  if (f >= 0 && f < n * STEP) {
    const i = Math.floor(f / STEP), q = f - i * STEP, u = pts[i], v = pts[i + 1];
    g.strokeStyle = g.fillStyle = RED;
    if (q < HOP) {
      const k = q / HOP;
      g.beginPath(); g.arc(u[0] + (v[0] - u[0]) * k, u[1] + (v[1] - u[1]) * k, 4, 0, 7);
      if (hollow) { g.lineWidth = 1.5; g.stroke(); } else g.fill();
    } else { g.globalAlpha = 1 - (q - HOP) / ACK; g.lineWidth = 2; line(g, v, u); g.globalAlpha = 1; }
  }
  return t0 + n * STEP;
}
// `n` messages kept at a node.
const held = (g, p, n) => { g.fillStyle = '#888'; for (let i = 0; i < n; i++) g.fillRect(p[0] + 9 + 7 * i, p[1] + 4, 5, 5); };
const label = (g, p, text) => {
  g.fillStyle = '#666'; g.font = '12px system-ui, sans-serif'; g.textAlign = 'center'; g.fillText(text, ...p);
};

// A relay passes a message on at once; a sleepy node's relay keeps it
// until the sleepy node's next beat.
scene('a-sleepy', 12, 5, (g, t) => {
  const A = [70, 120], B = [230, 120], S = [330, 60];
  links(g, [[A, B]]); links(g, [[B, S]], true);
  ring(g, A, t, 2, .3); ring(g, B, t, 2, 1.2); ring(g, S, t, 12, 8, .6);
  const at = travel(g, t, 1, [A, B]);
  if (t >= at && t < 8.3) held(g, B, 1);
  const got = travel(g, t, 8.3, [B, S]);
  relay(g, A); relay(g, B); sleepy(g, S, t >= got && t < got + 1.5);
  label(g, [A[0], A[1] + 24], T.relay); label(g, [B[0], B[1] + 24], T.relay);
  label(g, [S[0], S[1] - 14], T.sleepy);
});

// One guest's link at a time; the paired phone's stays.
scene('a-guests', 8, 0.6, (g, t) => {
  const N = [200, 105], own = [110, 55], G = [[295, 50], [320, 140], [200, 185], [85, 150]];
  const turn = Math.floor(t / 2), k = turn % 4, q = t - turn * 2, on = q < 1.5;
  g.lineWidth = 1.5; g.strokeStyle = GREY; line(g, own, N);
  if (on) { line(g, G[k], N); travel(g, t, turn * 2 + .3, k % 2 ? [N, G[k]] : [G[k], N]); }
  relay(g, N); phone(g, own);
  G.forEach((p, i) => phone(g, p, on && i == k ? GREY : PALE));
  label(g, [own[0], own[1] - 14], T.paired); label(g, [N[0], N[1] + 26], T.open);
  label(g, [G[0][0], G[0][1] - 14], T.guests);
});

// Toward the place: each hop to the neighbour nearest it, back from a
// dead end, round the other way.
scene('a-route', 10, 2.6, (g, t) => {
  const S = [40, 100], a = [120, 55], b = [205, 75], c = [115, 150], d = [210, 165], e = [300, 150], D = [360, 90];
  links(g, [[S, a], [S, c], [a, b], [c, d], [d, e], [e, D]]);
  g.strokeStyle = PALE; g.lineWidth = 1; g.beginPath(); g.arc(...D, 12, 0, 7); g.stroke();
  const end = travel(g, t, .5, [S, a, b, a, S, c, d, e, D]);
  [S, a, b, c, d, e].forEach(p => relay(g, p)); relay(g, D, t >= end && t < end + 1.5);
  label(g, [D[0] - 4, D[1] - 20], T.place); label(g, [b[0], b[1] - 12], T.dead);
});

// Mail waits at the home while its owner is away. They connect to a node
// and fetch, and the mail comes to where they are; their reply goes
// straight to the node the friend wrote from, and the friend's answer
// straight back. Then they leave, the clock runs on, and the next
// message waits at the home again.
scene('a-home', 23, 13.2, (g, t) => {
  const H = [60, 75], F = [105, 160], r1 = [150, 50], r2 = [240, 80], r3 = [330, 95], r4 = [225, 150], Y = [372, 45];
  const COMES = 2.5, GOES = 19.6, FADE = .6;
  const here = Math.min(1, Math.max(0, (t - COMES) / FADE), Math.max(0, (GOES + FADE - t) / FADE));
  links(g, [[H, r1], [r1, r2], [r2, r3], [F, H], [F, r4], [r4, r3]]);
  g.globalAlpha = here; links(g, [[r3, Y]], true); g.globalAlpha = 1;
  const kept = travel(g, t, 1, [F, H]);
  const asked = travel(g, t, 3.5, [Y, r3, r2, r1, H], true);
  const got = travel(g, t, asked + .3, [H, r1, r2, r3, Y]);
  const replied = travel(g, t, got + 1, [Y, r3, r4, F]);
  const answered = travel(g, t, replied + 1, [F, r4, r3, Y]);
  held(g, H, (t >= kept) - (t >= asked + .3));
  [H, r1, r2, r3, r4].forEach(p => relay(g, p)); relay(g, F, t >= replied && t < replied + 1);
  g.strokeStyle = GREY; g.lineWidth = 1.5;
  g.beginPath(); g.moveTo(H[0] - 11, H[1] - 7); g.lineTo(H[0], H[1] - 17); g.lineTo(H[0] + 11, H[1] - 7); g.stroke();
  g.globalAlpha = here;
  phone(g, Y, (t >= got && t < got + 1) || (t >= answered && t < answered + 1) ? RED : GREY);
  label(g, [Y[0] - 22, Y[1] + 4], T.you); g.globalAlpha = 1;
  label(g, [H[0], H[1] - 24], T.home); label(g, [F[0], F[1] + 22], T.friend);
  // A clock: a minute a turn while the phone is there, then three turns
  // less what it has gone, quickly, so it stands as it began.
  const C = [365, 165], AWAY = GOES + FADE, SLOW = 1 / 60, away = Math.max(0, (t - AWAY) / (23 - AWAY));
  const turns = Math.min(t, AWAY) * SLOW + (3 - AWAY * SLOW) * away * away * (3 - 2 * away);
  const hand = 2 * Math.PI * turns - Math.PI / 2;
  g.strokeStyle = PALE; g.lineWidth = 1.5; g.beginPath(); g.arc(...C, 12, 0, 7); g.stroke();
  for (let k = 0; k < 4; k++) {
    const x = Math.cos(k * Math.PI / 2), y = Math.sin(k * Math.PI / 2);
    line(g, [C[0] + 9 * x, C[1] + 9 * y], [C[0] + 12 * x, C[1] + 12 * y]);
  }
  g.strokeStyle = away ? RED : GREY; line(g, C, [C[0] + 8 * Math.cos(hand), C[1] + 8 * Math.sin(hand)]);
});

// A new version from a phone to one node and on from node to node, in
// pieces: a node passes on the pieces it has while it still collects
// the rest, so each is a little behind the one it takes from.
scene('a-update', 12, 4, (g, t) => {
  const P = [[60, 105], [135, 55], [135, 155], [215, 105], [295, 55], [295, 155], [370, 105]];
  const from = [null, 0, 0, 1, 3, 3, 4], ph = [40, 50], TAKES = 4, BEHIND = 1;
  const begin = [.5];
  for (let i = 1; i < P.length; i++) begin[i] = begin[from[i]] + BEHIND;
  const part = i => Math.min(1, Math.max(0, (t - begin[i]) / TAKES));
  links(g, [[0, 1], [0, 2], [1, 3], [2, 3], [3, 4], [3, 5], [4, 6], [5, 6]].map(([a, b]) => [P[a], P[b]]));
  links(g, [[ph, P[0]]], true);
  P.forEach((p, i) => {
    const got = part(i);
    if (got <= 0 || got >= 1) return;
    const src = i ? P[from[i]] : ph;
    // The pieces on their way, and how much of the version is here.
    for (let k = 0; k < 3; k++) {
      const q = (got * 6 + k / 3) % 1;
      dot(g, [src[0] + (p[0] - src[0]) * q, src[1] + (p[1] - src[1]) * q], 2, RED);
    }
    g.strokeStyle = RED; g.lineWidth = 2; g.beginPath();
    g.arc(...p, 9, -Math.PI / 2, -Math.PI / 2 + 2 * Math.PI * got); g.stroke();
  });
  // Red once it has all of it and runs it.
  P.forEach((p, i) => dot(g, p, 5, part(i) >= 1 ? RED : GREY));
  phone(g, ph);
  label(g, [ph[0], ph[1] - 14], T.version);
});
})();
