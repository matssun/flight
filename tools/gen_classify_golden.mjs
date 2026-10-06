// SPDX-License-Identifier: MIT
//
// Generates flight-classify/tests/golden/classify.csv by running Fleet's own detector
// (ref/fleet/src/state/{detection,scraper}.ts, MIT, (c) 2026 Nick Nisi) over every window of 1-6
// consecutive lines of every fixture capture, under every built-in manifest, plus every fixture title. This
// exercises the JS-to-Rust regex translation on varied real input. Needs node >= 22.6 and the
// Fleet clone at ref/fleet. Run from the repo root:
//   node --experimental-strip-types tools/gen_classify_golden.mjs

import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { CLAUDE_MANIFEST, CODEX_MANIFEST, OPENCODE_MANIFEST, PI_MANIFEST } from '../ref/fleet/src/state/detection.ts';
import { detectFromPaneContent, detectFromTitle } from '../ref/fleet/src/state/scraper.ts';

const MANIFESTS = { claude: CLAUDE_MANIFEST, codex: CODEX_MANIFEST, opencode: OPENCODE_MANIFEST, pi: PI_MANIFEST };
const DIR = new URL('../flight-classify/tests/fixtures/', import.meta.url);
const label = (s) => (s === null ? '-' : s);

const rows = [];
for (const file of readdirSync(DIR).filter((f) => f.endsWith('.txt')).sort()) {
  const lines = readFileSync(new URL(file, DIR), 'utf-8').split('\n');
  for (const [agent, manifest] of Object.entries(MANIFESTS)) {
    for (let start = 0; start < lines.length; start++) {
      for (let len = 1; len <= 6 && start + len <= lines.length; len++) {
        const r = detectFromPaneContent(lines.slice(start, start + len), manifest);
        // S,agent,fixture,start,len,status,ruleId
        rows.push(['S', agent, file, start, len, label(r.status), label(r.ruleId)].join(','));
      }
    }
  }
}
for (const file of readdirSync(DIR).filter((f) => f.endsWith('.title')).sort()) {
  const title = readFileSync(new URL(file, DIR), 'utf-8').trim();
  for (const [agent, manifest] of Object.entries(MANIFESTS)) {
    const r = detectFromTitle(title, manifest);
    // T,agent,fixture,-,-,status,ruleId
    rows.push(['T', agent, file, '-', '-', label(r.status), label(r.ruleId)].join(','));
  }
}
writeFileSync(new URL('../flight-classify/tests/golden/classify.csv', import.meta.url), rows.join('\n') + '\n');
console.log(`wrote ${rows.length} cases`);
