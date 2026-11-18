# Porting map

Every Node file of `core/lib` and `core/studio/*.mjs` belongs to exactly one Rust module of `native/src`.
A file that is split by job lists its other modules under "Also". The owner group is the one that ports
it and edits only its own files; `Cargo.toml`, `src/main.rs` and `src/cli/mod.rs` belong to the integrator.
Set the status to `done` in the same commit that registers the commands as native.

| Node file | Rust module | Also | Owner group | Status |
|---|---|---|---|---|
| `core/lib/a11y.mjs` | `browser/a11y` |  | browser | todo |
| `core/lib/binary.mjs` | `tools/binary` |  | tools | done |
| `core/lib/cdp.mjs` | `browser/cdp` | findBrowser, launch, guard: browser/launch; installBrowser, checkedDownloadUrl: browser/download | browser | done |
| `core/lib/check.mjs` | `tools/check` | engine lookup and project env filter of the hardened locate.mjs / config.mjs: tools/check/engine (to move to review/locate and core/config) | tools | done (follows `fix/impeccable-hardening`; the rendered a11y check of a whole-design or `--url` run forwards) |
| `core/lib/cli.mjs` | `cli (mod.rs + one file per group)` | decision log cases: workflow/decisions | every group, its own `cli/<group>.rs` | dispatch, version, help done; commands todo |
| `core/lib/color.mjs` | `design/color` |  | design | done |
| `core/lib/comments.mjs` | `review/comments` |  | review | done |
| `core/lib/compose.mjs` | `design/compose` | parsing moves to design/html | design | done |
| `core/lib/config.mjs` | `core/config` |  | core | done |
| `core/lib/dsaudit.mjs` | `design/dsaudit` |  | design | done |
| `core/lib/edit.mjs` | `design/edit` |  | design | todo |
| `core/lib/extract.mjs` | `browser/extract` |  | browser | todo |
| `core/lib/fonts.mjs` | `design/fonts` |  | design | done |
| `core/lib/freeze.mjs` | `workflow/freeze` |  | workflow | done (its command, `approve G2`, still forwards) |
| `core/lib/gate.mjs` | `workflow/gates` |  | workflow | done |
| `core/lib/hooks.mjs` | `hooks (one file per handler)` | session_start, pre_tool_use, post_tool_use, user_prompt_submit, stop | hooks | done, except the layout check of stop (browser group): that call forwards |
| `core/lib/host.mjs` | `core/host` |  | core | done |
| `core/lib/icons.mjs` | `design/icons` |  | design | done |
| `core/lib/inbox.mjs` | `review/inbox` |  | review | done |
| `core/lib/inspector.mjs` | `tools/inspector` |  | tools | done |
| `core/lib/install.mjs` | `tools/install` |  | tools | done |
| `core/lib/inventory.mjs` | `design/inventory` |  | design | done |
| `core/lib/lang.mjs` | `workflow/lang` |  | workflow | done |
| `core/lib/layout.mjs` | `browser/layout_check` | changedScreens, markChecked: design/layout_cache | browser | todo |
| `core/lib/lighthouse.mjs` | `tools/lighthouse` |  | tools | done |
| `core/lib/lint.mjs` | `design/lint` | JavaScript regular expressions for the regex crate: design/lint/jsre | design | done |
| `core/lib/live.mjs` | `studio/live_proxy` |  | studio | todo |
| `core/lib/locate.mjs` | `review/locate` |  | review | done (no caller yet: check runner and doctor, tools group) |
| `core/lib/mcp.mjs` | `tools/mcp` |  | tools | todo |
| `core/lib/penpot-tokens.mjs` | `penpot/tokens` |  | penpot | todo |
| `core/lib/penpot.mjs` | `penpot/export` | split by job: penpot/diff, penpot/import, penpot/live | penpot | todo |
| `core/lib/pixeldiff.mjs` | `browser/pixeldiff` |  | browser | done (pixelmatch ported: no browser needed) |
| `core/lib/runtime.mjs` | `browser/launch` | installBrowser: browser/download; screenshot: browser/screenshot | browser | done |
| `core/lib/stack.mjs` | `workflow/stack` |  | workflow | done (its command, `stack`, is in `cli/tools.rs` and still forwards) |
| `core/lib/state.mjs` | `workflow/state` | decision log entry: workflow/decisions | workflow | done |
| `core/lib/style.mjs` | `design/style` |  | design | done |
| `core/lib/system-page.mjs` | `design/system_page` |  | design | todo |
| `core/lib/system.mjs` | `design/system` |  | design | done |
| `core/lib/tokens.mjs` | `design/tokens` |  | design | done (also holds `js`: JavaScript number, string and key-order rules, to move to core/util) |
| `core/lib/util.mjs` | `core/util` | paths: core/paths; DfwError: core/error | core | done (skeleton) |
| `core/studio/pages.mjs` | `studio/server` |  | studio | todo |
| `core/studio/server.mjs` | `studio/server` | split by job: studio/watch, studio/sse, studio/penpot_routes | studio | todo |

Rust modules with no Node file of their own: `design/html` (the tokenizer, design decision 2, owner design, done)
and `assets` (embedded files, design decision 6, owner integrator).

## Commands

| Group file | Commands | State |
|---|---|---|
| `cli/mod.rs` | version, help | native |
| `cli/debug.rs` | __debug (hidden: config, host-format, host-render, lint, ...; for the conformance tests) | native |
| `cli/workflow.rs` | init, status, mode, phase, approve, reopen, pause, resume, decide, decisions, config, lang, verify-snapshot | native; forwards at run time: `init` without `design/` (token build), `status` as text in the system phase before DS (counts masters), `approve G2` (its gate needs the rendered a11y check). Also `__debug hot` |
| `cli/hook.rs` | hook | native: session-start, pre-tool-use, post-tool-use, user-prompt-submit always; stop forwards when the layout check applies |
| `cli/comments.rs` | comments | native (every subcommand, incl. wait and inbox) |
| `cli/design.rs` | build, tokens, lint, check, system, style, type, palette, icons, fonts, blueprint (hidden: `__debug compose`, `__debug color`, `__debug lint`) | native; forwards `tokens extract\|export\|import`, and `check` when it renders pages for the a11y check (whole design or `--url`, without `--no-a11y`) |
| `cli/studio.rs` | studio, review, __studio-server, __review-server | forward |
| `cli/browser.rs` | shoot, diff | native |
| `cli/browser.rs` | layout | forward |
| `cli/penpot.rs` | penpot | forward |
| `cli/tools.rs` | inspector, install, uninstall, setup (`--a11y` forwards), doctor | native |
| `cli/tools.rs` | mcp, stack, handoff | forward |

The `HELP` text lives in `core/lib/cli.mjs` and in `src/cli/help.txt` until the Node CLI is deleted.
