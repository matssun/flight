// SPDX-License-Identifier: MIT
//
// Generates flight-classify/tests/golden/resolve.csv and glyph.csv by running Fleet's own
// stateful discovery logic (ref/fleet/src/agents/discovery.ts, MIT, (c) 2026 Nick Nisi):
//   resolve.csv  resolveDiscoveredStatus over signal sequences (the DONE state machine)
//   glyph.csv    discoverAgents' glyph debounce over timed glyph sequences
// Needs node >= 22.6 and the Fleet clone at ref/fleet. Run from the repo root:
//   node --experimental-strip-types tools/gen_resolve_golden.mjs

import { writeFileSync } from 'node:fs';
import { discoverAgents, resolveDiscoveredStatus } from '../ref/fleet/src/agents/discovery.ts';

// fuseDiscoveredState reads the clock; pin it to the `now` the sequences pass (100 s).
Date.now = () => 100 * 1000;

const OUT = (name) => new URL(`../flight-classify/tests/golden/${name}`, import.meta.url);
const STATUS = { '-': null, P: 'PERMIT', Q: 'QUESTION', B: 'BUSY', I: 'IDLE' };
const LETTER = { PERMIT: 'P', QUESTION: 'Q', BUSY: 'B', IDLE: 'I', DONE: 'D' };

// --- resolve.csv: token = glyph(0/1) title(-PB) scrape(-PQBI) focused(0/1), e.g. "1-P0" ---
const tokens = [];
for (const g of '01') for (const t of '-PB') for (const s of '-PQBI') for (const f of '01') tokens.push(g + t + s + f);

function run(seq) {
  const tracking = { wasBusy: new Set(), done: new Set() };
  return seq
    .map((tok) =>
      LETTER[
        resolveDiscoveredStatus(
          '%1',
          { glyphWorking: tok[0] === '1', title: STATUS[tok[1]], scrape: STATUS[tok[2]], focused: tok[3] === '1' },
          tracking,
          100,
        )
      ],
    )
    .join('');
}

let seed = 12345; // mulberry32: fixed seed so the golden is reproducible
const rand = () => {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

const seqs = [];
for (const a of tokens) seqs.push([a]);
for (const a of tokens) for (const b of tokens) seqs.push([a, b]);
for (let i = 0; i < 1500; i++) {
  const len = 3 + Math.floor(rand() * 6);
  seqs.push(Array.from({ length: len }, () => tokens[Math.floor(rand() * tokens.length)]));
}
writeFileSync(OUT('resolve.csv'), seqs.map((s) => `${s.join(' ')};${run(s)}`).join('\n') + '\n');

// --- glyph.csv: step = dt(1|3)+glyph(0/1) e.g. "31"; agent aider|codex; idleSecs 3 ---
const steps = ['10', '11', '30', '31'];
const glyphRows = [];
const PS = ['  100    50 agent'];
for (const agent of ['aider', 'codex']) {
  const walk = (prefix) => {
    if (prefix.length > 0) {
      let lastWorking = new Map();
      let now = 1000;
      let out = '';
      for (const st of prefix) {
        now += Number(st[0]);
        const caps = new Map([['%1', st[1] === '1' ? '⠹ working' : 'idle']]);
        const scan = discoverAgents(PS.map((l) => l.replace('agent', agent)), new Map([[50, '%1']]), caps, {
          allowlist: new Set([agent]),
          lastWorking,
          idleSecs: 3,
          now,
        });
        lastWorking = scan.lastWorking;
        out += scan.agents[0].working ? 'W' : 'i';
      }
      glyphRows.push(`${agent};${prefix.join(' ')};${out}`);
    }
    if (prefix.length < 5) for (const st of steps) walk([...prefix, st]);
  };
  walk([]);
}
writeFileSync(OUT('glyph.csv'), glyphRows.join('\n') + '\n');
console.log(`resolve: ${seqs.length} sequences, glyph: ${glyphRows.length} sequences`);
