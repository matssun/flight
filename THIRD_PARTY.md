<!-- SPDX-License-Identifier: MIT -->

# Third-Party Notices

Flight is MIT-licensed, as are the projects below. They informed Flight's design and are kept under `ref/` (gitignored) for study. Where code is copied or closely ported, its copyright and permission notice must be reproduced here and in the file that carries it.

| Project | Language | Copyright | License | Use |
|---|---|---|---|---|
| [sesh](https://github.com/joshmedeski/sesh) | Go | 2023 Josh Medeski | MIT | Design reference: session sources, namer, connector |
| [tmux-sessionizer](https://github.com/jrmoulton/tmux-sessionizer) | Rust | 2022 Jared Moulton | MIT | Design reference: tmux wrapper, picker |
| [fleet](https://github.com/nicknisi/fleet) | TypeScript | 2026 Nick Nisi | MIT | Design reference: agent state engine, dashboard, `fleet.observe/v1` contract |

## Copied or ported code

Source: [fleet](https://github.com/nicknisi/fleet) at commit `fc5b2f0`. Copyright (c) 2026 Nick Nisi, MIT (full text below).

| Flight file | Fleet file | Nature |
|---|---|---|
| `flight-classify/src/builtin/*.rs` | `src/state/detection.ts` | Ported: rule ids, patterns, states and ordering (priorities replace array order; JS regex syntax translated to Rust) |
| `flight-classify/src/screen.rs`, `title.rs`, `refine.rs` | `src/state/scraper.ts` | Ported: windowed first-match detection, prompt-marker fallback, codex title refinement |
| `flight-classify/src/ansi.rs` | `src/terminal/ansi.ts` | Ported: `stripAnsi` pattern |
| `flight-classify/tests/fixtures/*` | `src/state/fixtures/*` | Copied verbatim: real pane captures (Fleet notes their origin as herdr/tmux-agents-mon test fixtures) |
| `flight-classify/tests/claude.rs`, `other_agents.rs` | `src/state/detection.test.ts`, `scraper.test.ts` | Test cases ported |

### MIT License (Fleet)

```
MIT License

Copyright (c) 2026 Nick Nisi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Crate dependencies

`regex` (MIT OR Apache-2.0), used by `flight-classify`. A machine-checked allow-list will be added.
