// SPDX-License-Identifier: MIT
//
// Generates flight-classify/tests/golden/fusion.csv by running Fleet's own engine
// (ref/fleet/src/state/engine.ts, MIT, (c) 2026 Nick Nisi) over an exhaustive input grid.
// Needs node >= 22.6 and the Fleet clone at ref/fleet. Run from the repo root:
//   node --experimental-strip-types tools/gen_fusion_golden.mjs
// The output is committed, so the Rust tests never need node or Fleet.

import { writeFileSync } from 'node:fs';
import { fuseDiscoveredState, fuseState } from '../ref/fleet/src/state/engine.ts';

const NOW = 1_000_000;
Date.now = () => NOW * 1000; // fuseState reads the clock

const HOOK = ['permit', 'done', 'working', 'waiting', 'idle', 'bogus'];
const HOOK_AGE = [0, 179, 180];
const STATUS = [null, 'PERMIT', 'QUESTION', 'DONE', 'BUSY', 'IDLE'];
const EVENT_AGE = [0, 179, 180];
const SCRAPE_RULE = ['permit.yn', 'busy.spinner-glyph'];

const lines = [];
for (const hookState of HOOK)
  for (const hookAge of HOOK_AGE)
    for (const eventStatus of STATUS)
      for (const eventAge of eventStatus === null ? [null] : EVENT_AGE)
        for (const scrapeStatus of STATUS)
          for (const scrapeRuleId of scrapeStatus === null ? [null] : SCRAPE_RULE) {
            const input = {
              hookState,
              hookTs: NOW - hookAge,
              eventStatus,
              eventTs: eventAge === null ? null : NOW - eventAge,
              scrapeStatus,
              scrapeRuleId,
            };
            const r = fuseState(input);
            const d = (v) => (v === null ? '-' : v);
            const rule = scrapeRuleId === null ? '-' : scrapeRuleId === 'busy.spinner-glyph' ? 'g' : 'p';
            // hookState,hookAge,eventStatus,eventAge,scrapeStatus,rule,status,winner,timeout
            lines.push(
              [hookState, hookAge, d(eventStatus), d(eventAge), d(scrapeStatus), rule, r.status, r.decision.winner, r.decision.workingTimeoutFired ? 1 : 0].join(','),
            );
          }

for (const working of [false, true])
  for (const scrapeStatus of STATUS)
    // D,working,scrapeStatus,status
    lines.push(['D', working ? 1 : 0, scrapeStatus ?? '-', fuseDiscoveredState(working, scrapeStatus, NOW)].join(','));

writeFileSync(new URL('../flight-classify/tests/golden/fusion.csv', import.meta.url), lines.join('\n') + '\n');
console.log(`wrote ${lines.length} cases`);
