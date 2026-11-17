# Impeccable: network audit

What the vendored Impeccable skill (`plugins/impeccable`, engine `0.1.23`, client `impeccable/4.9.0`) downloads and contacts, when, with what payload, and how dfw turns it off. Audited 2026-11-16 on macOS arm64.

**Summary.** With dfw's environment, the only things dfw itself runs are `impeccable detect` on local files and the one-time engine download. `detect` makes no network connection. Telemetry and the update check are off. If the Impeccable skill runs its own new-design flow, it still makes one anonymous `GET impeccable.style/api/roll`. That request only fetches direction cards. It carries a random key and no project data, and it is flagged `telemetry: off`. `impeccable help` always fetches the command list. The account, image-generation and installer features contact other hosts. dfw never runs any of these.

## How it was checked

- Read the launchers (`scripts/impeccable`, `scripts/impeccable.cmd`), the browser scripts and every `SKILL.md` and `reference/*.md`.
- Ran `strings` on the engine binary (`~/.impeccable/bin/0.1.23/impeccable`, sha256 `b10fccb5…a868`, which matches `engine-checksums.txt`) to list URLs, env names and API paths.
- Ran the engine in throwaway directories, with a throwaway `HOME` and dummy `index.html`/`PRODUCT.md` files. Each run went through a local logging HTTP CONNECT proxy (`HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`). `lsof -i` sampled the process and its children every 50 ms. The engine's HTTP client (`ureq/2.12.1`) honours the proxy variables. `lsof` saw no socket that bypassed the proxy or the sink.
- To read payloads, the API base was pointed at a local plain-HTTP sink with `IMPECCABLE_API_URL` / `IMPECCABLE_UPDATE_HOST` / `IMPECCABLE_DOWNLOAD_BASE`. The sink logged method, path, headers and body. It relayed only the roll request upstream and answered everything else locally, so no ping reached the real service.
- Each command ran twice: **bare** (no privacy variables) and with **dfw env** (`DO_NOT_TRACK=1 IMPECCABLE_NO_UPDATE_CHECK=1 IMPECCABLE_HOOK_DISABLED=1`).

## 1. Downloads

| What | From | When | Integrity | Cache |
|---|---|---|---|---|
| Engine binary (~19 MB, Rust) | `https://github.com/pbakaus/impeccable/releases/download/engine-v<VERSION>/impeccable-<os>-<arch>[.exe]`. `IMPECCABLE_DOWNLOAD_BASE` overrides the base (`scripts/impeccable:234-235`) | First run only, and only when no usable engine exists. dfw triggers it only from `dfw check --setup` / `npx … install` (`core/lib/cli.mjs:760-767`, `core/lib/install.mjs:135-138`). Hooks and plain `dfw check` never download: `runImpeccable` returns `not-installed` (`core/lib/check.mjs:72-74`). If the model runs a skill command first, the launcher downloads on that run | sha256 against `scripts/engine-checksums.txt` (pinned in the repo, never fetched). Without a digest it fails closed before any traffic (`:237-241`). A mismatch, an empty file or a missing sha256 tool means refusal (`:270-299`). Verified locally: a bogus mirror gave `checksum mismatch … refusing`, exit 127 | `${IMPECCABLE_HOME:-~/.impeccable}/bin/<VERSION>/impeccable`, plus a `.verified` stamp (digest + size) so later runs skip rehashing (`:160-180`) |
| Skill bundle (`impeccable install/update`) | `impeccable.style/api/download/bundle/universal` + `.sig.json` signature | Only on `impeccable install` or `update`. dfw never runs these; dfw's vendored copy is updated by `scripts/skills.mjs` | Signed bundle (`crates/skills/src/bundle_signature.rs`) | project / harness skill dirs |
| Impeccable Loop app (macOS) | GitHub release zip `impeccable-loop-<v>.zip` | Only on `impeccable loop install/update` | sha256 pinned inside the engine | `~/Applications` |

Lookup order before any download (`scripts/impeccable:115-183`):

1. `$IMPECCABLE_BIN` (run without a probe).
2. `scripts/bin/<os>-<arch>/impeccable` (must report the pinned version).
3. `~/.impeccable/bin/impeccable` (probed).
4. The version-pinned cache (hash-verified).
5. `impeccable` on PATH (probed).
6. Download.

**Offline provisioning.** Copy the matching release asset to `~/.impeccable/bin/0.1.23/impeccable` and `chmod +x` it; the launcher verifies it against the checksum file on first use. Or set `IMPECCABLE_BIN=/path/to/impeccable`, which dfw also honours (`core/lib/locate.mjs:46-47`). `IMPECCABLE_HOME` moves the cache.

## 2. Outbound calls

| Host / path | Triggered by | Payload (observed or from strings) | Bare | dfw env |
|---|---|---|---|---|
| none | `detect --json <files>` (the only command dfw runs), `--help`, `--version`, `doctor`, `signals`, `check` (signed out, no project install), `telemetry status`, `whoami` (signed out) | none: 0 connections, also for HTML that links Google Fonts and external images. The engine starts no browser for static files | none | none |
| `GET https://impeccable.style/api/commands` (path from strings; host not overridable) | `impeccable help` (not `--help`): prints the command list. dfw and the skill text never run it | no input from the project | yes | **yes**: no variable disables it (observed with dfw env) |
| `GET https://impeccable.style/api/version` | `context` (session context load) | no query, no body; headers `user-agent: ureq/2.12.1`, `accept`. Writes `~/.impeccable/update-check.json` | **yes** | **no** (`IMPECCABLE_NO_UPDATE_CHECK=1`) |
| `GET https://impeccable.style/api/roll?scope=&key=&reroll=&mode=` | `concept-seed` (Impeccable's own new-design flow, `reference/new-work.md:40,49`). dfw does not run it | `key` = a 32-hex seed key made with a per-user random salt (`~/.impeccable/seed-salt`), so the server cannot recover anything from it. Headers: `x-impeccable-client: impeccable/4.9.0`, plus **`x-impeccable-telemetry: off`** when telemetry is off. No project names, files or prompts. The proxy saw about 600 B sent and 22 KB received (card catalog). Without network the roll degrades, and the command says so | yes, recorded | yes, **not recorded** (header `off`). It is a feature request, not telemetry |
| `POST https://impeccable.style/api/chosen` | `concept-seed --from <key> --kind …` (the "choice ping" after a round) | JSON `{key, scope, mode, kind, reroll, poolRevision, handId, pingId, source:"agent"}` (244 B) | **yes** | **no**: prints `choice ping skipped` |
| `https://impeccable.style/worlds/cards/*.webp` | URLs that `concept-seed` prints as "quality bar" boards. They are fetched only if the agent or a browser opens them | plain image GET | on view | on view |
| `https://api.impeccable.style` (`/cli/v1/entitlement`, `/health/scans`, `/critiques`, `/setup/reports`, `/design`, `/foundations`, `/manifest`, `/artifact`, `/lease`), `…/mcp` | `login`, `whoami`, `health`, `critiques`, `setup`, `loop connect` (adds an `impeccable` MCP server to Claude/Codex) | When **signed in**, "design health" can upload each full scan's counts and fingerprints, and critique sync can upload saved critique reports. Not exercised: requires an account | only after `impeccable login` | same. dfw never logs in; turn off with `impeccable health off` / `critiques off`, delete with `… forget` |
| `https://api.openai.com/v1` (image model) | `generate-image` (Impeccable's comp/visualize flow) when `OPENAI_API_KEY` is set and the harness has no native image tool | the image prompt (describes the product), billed to the user's key. Not exercised | if key set | if key set (dfw does not route to this flow) |
| `https://fonts.googleapis.com/css2?family=…` | capture/comp HTML that the engine builds for its browser screenshots (`comp-*`, `component-review`) | font family names | on those commands | same |
| `http://localhost:<port>` | `live*` commands: `scripts/live-browser.js` talks only to the engine's local live server (`/events`, `/source`, `/manual-edit-*`, `/status`, `/design-system.json`) and imports same-origin dev-server modules. `modern-screenshot.umd.js` fetches the resources the captured page itself references | local only | n/a | n/a |
| your URL | `detect <url>` / `dfw check --url` | loads your own dev server in a browser | n/a | n/a |

TLS bodies to the real hosts were not decrypted. The engine pins its own roots (rustls), so a MITM CA was not possible. Payloads above come from the plain-HTTP sink, which uses the same code path with a different base URL. The proxy's byte counts to the real host match: the dfw-env roll request is about 29 B larger, the size of the `x-impeccable-telemetry: off` header. Account, OpenAI, Loop and install flows were read from strings only.

## 3. Telemetry and the switches

| Variable | Effect (verified) | Set by dfw |
|---|---|---|
| `DO_NOT_TRACK=1` | roll sent with `x-impeccable-telemetry: off`; choice ping skipped; `telemetry status` → `off (DO_NOT_TRACK is set)` | yes: `core/lib/config.mjs:44-49`, and the dfw overlay at the top of `scripts/impeccable:2-4` (default unless the user overrides) |
| `IMPECCABLE_NO_TELEMETRY=1` | identical to `DO_NOT_TRACK` (same header, same skip). Not needed in addition | no (redundant) |
| `IMPECCABLE_NO_UPDATE_CHECK=1` | no `GET /api/version` | yes (both places) |
| `IMPECCABLE_HOOK_DISABLED=1` | Impeccable's edit hooks do nothing (dfw runs `detect` itself) | yes (both places) |
| `impeccable telemetry off` | persistent per-user equivalent (`~/.impeccable` config) | no |
| `IMPECCABLE_API_URL` / `IMPECCABLE_UPDATE_HOST` / `IMPECCABLE_DOWNLOAD_BASE` | redirect the roll API, the update check and the engine download (for example to an internal mirror or an unreachable host for a fully offline roll) | no |

**Verdict:** dfw's environment leaks nothing. Every telemetry and update-check call is off with the variables dfw already sets, so no change to `core/lib/config.mjs` is needed. A unit test pins those variables (`tests/unit/dfw.test.mjs`).

**Known gaps:**

1. The Windows launcher `scripts/impeccable.cmd` has no dfw overlay. When the model runs Impeccable commands directly on Windows (not through dfw), telemetry and the update check follow the user's own settings.
2. If a user signs in to an Impeccable account, the account sync features are theirs to switch off.
3. With `OPENAI_API_KEY` set, Impeccable's own visualize flow can call OpenAI.
