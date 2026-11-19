# Porting map

Every Node file of `core/lib` and `core/studio/*.mjs` belongs to exactly one Rust module of `native/src`.
A file that is split by job lists its other modules under "Also". The owner group is the one that ports
it and edits only its own files; `Cargo.toml`, `src/main.rs` and `src/cli/mod.rs` belong to the integrator.
Set the status to `done` in the same commit that registers the commands as native.

| Node file | Rust module | Also | Owner group | Status |
|---|---|---|---|---|
| `core/lib/a11y.mjs` | `browser/a11y` |  | browser | done (callers: `check`, `approve G2`, doctor, install, `setup --a11y`) |
| `core/lib/binary.mjs` | `tools/binary` |  | tools | done |
| `core/lib/cdp.mjs` | `browser/cdp` | findBrowser, launch, guard: browser/launch; installBrowser, checkedDownloadUrl: browser/download | browser | done |
| `core/lib/check.mjs` | `tools/check` | starting a tool: tools/check/run; the report: tools/check/report | tools | done (the engine lookup is review/locate, the project env filter core/config) |
| `core/lib/cli.mjs` | `cli (mod.rs + one file per group)` | decision log cases: workflow/decisions | every group, its own `cli/<group>.rs` | done (every command native) |
| `core/lib/color.mjs` | `design/color` |  | design | done |
| `core/lib/comments.mjs` | `review/comments` |  | review | done |
| `core/lib/compose.mjs` | `design/compose` | parsing moves to design/html | design | done |
| `core/lib/config.mjs` | `core/config` |  | core | done |
| `core/lib/dsaudit.mjs` | `design/dsaudit` |  | design | done |
| `core/lib/edit.mjs` | `design/edit` | split by job: edit/locate, edit/style, edit/moves, edit/layout | design | done (callers: the Studio server; `__debug edit\|undo`) |
| `core/lib/extract.mjs` | `browser/extract` |  | browser | done |
| `core/lib/fonts.mjs` | `design/fonts` |  | design | done |
| `core/lib/freeze.mjs` | `workflow/freeze` |  | workflow | done |
| `core/lib/gate.mjs` | `workflow/gates` |  | workflow | done |
| `core/lib/hooks.mjs` | `hooks (one file per handler)` | session_start, pre_tool_use, post_tool_use, user_prompt_submit, stop | hooks | done |
| `core/lib/host.mjs` | `core/host` |  | core | done |
| `core/lib/icons.mjs` | `design/icons` |  | design | done |
| `core/lib/inbox.mjs` | `review/inbox` |  | review | done |
| `core/lib/inspector.mjs` | `tools/inspector` |  | tools | done |
| `core/lib/install.mjs` | `tools/install` |  | tools | done |
| `core/lib/inventory.mjs` | `design/inventory` |  | design | done |
| `core/lib/lang.mjs` | `workflow/lang` |  | workflow | done |
| `core/lib/layout.mjs` | `browser/layout_check` | changedScreens, markChecked: design/layout_cache | browser | done |
| `core/lib/lighthouse.mjs` | `tools/lighthouse` |  | tools | done |
| `core/lib/lint.mjs` | `design/lint` | JavaScript regular expressions for the regex crate: core/js/regex | design | done |
| `core/lib/live.mjs` | `studio/live_proxy` | the server half: studio/live_proxy/proxy; the wire: studio/http | studio | done |
| `core/lib/locate.mjs` | `review/locate` |  | review | done, with the engine lookup of 0.5.2 (`resolveEngine`, `verifyEngine`, `engineAssets`, `launcherCommand`); callers: doctor and the check runner |
| `core/lib/mcp.mjs` | `tools/mcp` | install and uninstall: tools/mcp_install | tools | done |
| `core/lib/penpot-tokens.mjs` | `penpot/tokens` |  | penpot | done |
| `core/lib/penpot.mjs` | `penpot/export` | split by job: penpot/diff, penpot/import, penpot/live; `snapshotPage` stays JavaScript in `penpot/snapshot_page.js` | penpot | done (the Studio's Penpot routes call it from studio/penpot_routes) |
| `core/lib/pixeldiff.mjs` | `browser/pixeldiff` |  | browser | done (pixelmatch ported: no browser needed) |
| `core/lib/runtime.mjs` | `browser/launch` | installBrowser: browser/download; screenshot: browser/screenshot | browser | done |
| `core/lib/stack.mjs` | `workflow/stack` |  | workflow | done |
| `core/lib/state.mjs` | `workflow/state` | decision log entry: workflow/decisions | workflow | done |
| `core/lib/style.mjs` | `design/style` |  | design | done |
| `core/lib/system-page.mjs` | `design/system_page` | the CSS literal: design/system_page.css | design | done (callers: Studio server and layout check) |
| `core/lib/system.mjs` | `design/system` |  | design | done |
| `core/lib/tokens.mjs` | `design/tokens` |  | design | done |
| `core/lib/util.mjs` | `core/util` | paths: core/paths; DfwError: core/error; JavaScript value rules: core/js; child processes: core/process | core | done |
| `core/studio/pages.mjs` | `design/pages` |  | studio | done |
| `core/studio/server.mjs` | `studio/server` | split by job: studio/api, studio/watch (polling only: no OS watcher without a crate), studio/sse, studio/penpot_routes, studio/http (the HTTP/1.1 layer shared with the proxy); browser files embedded by `assets` | studio | done |

Rust modules with no Node file of their own: `design/html` (the tokenizer, design decision 2, owner design, done),
`assets` (embedded files, design decision 6: core/studio and core/templates; kits/ and assets/ are still read from the
plugin folder), `core/js` (the JavaScript value and regular expression rules every module follows), `core/process`
(spawn plans, pids, Node.js on PATH), `tools/handoff` (`cmdHandoff` of cli.mjs, owner tools, done) and `telemetry`
(the `telemetry` change: events, otlp, spool, flush; with `build.rs` and `cli/telemetry.rs`; done).

## Commands

| Group file | Commands | State |
|---|---|---|
| `cli/mod.rs` | version, help | native; `DFW_NATIVE=off` still hands the whole command line to Node (escape hatch until the Node core is deleted) |
| `cli/debug.rs` | __debug (hidden helpers for the conformance tests) | native |
| `cli/workflow.rs` | init, status, mode, phase, approve, reopen, pause, resume, decide, decisions, config, lang, verify-snapshot (hidden: `__debug hot`) | native |
| `cli/hook.rs` | hook | native |
| `cli/comments.rs` | comments | native (every subcommand, incl. wait and inbox) |
| `cli/design.rs` | build, tokens, lint, check, system, style, type, palette, icons, fonts, blueprint (hidden: `__debug compose`, `__debug color`, `__debug lint`) | native |
| `cli/edit.rs` | none (hidden: `__debug edit`, `undo`, `system-page`, `changed-screens`, `mark-checked`) | native |
| `cli/studio.rs` | studio, review, __studio-server, __review-server | native |
| `cli/browser.rs` | shoot (job list in `browser/shoot`), diff, layout (hidden: `__debug a11y`) | native |
| `cli/penpot.rs` | penpot (export, plugin, import); `tokens export\|import`, called from `cli/design.rs` (hidden: `__debug penpot-build-export`, `__debug penpot-sync`) | native |
| `cli/tools.rs` | inspector, install, uninstall, setup, doctor, mcp, stack, handoff (`cmdHandoff` of cli.mjs is `tools/handoff`) | native |
| `cli/telemetry.rs` | telemetry (new: status, on, off, endpoint, show, flush; hidden: `__debug telemetry-build`) | native; the Node CLI has no such command |

The `HELP` text lives in `core/lib/cli.mjs` and in `src/cli/help.txt` until the Node CLI is deleted; the native
text has one line more (the telemetry command).
