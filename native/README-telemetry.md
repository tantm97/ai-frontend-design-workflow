# Telemetry in the native binary

Usage telemetry of the `telemetry` OpenSpec change: code in `src/telemetry/`, the command in
`src/cli/telemetry.rs`, the build-time address in `build.rs`.

## What decides whether it is on

In this order (`telemetry::decide`):

1. `DO_NOT_TRACK` set (not `0` or `false`), `DFW_TELEMETRY=off` (also `0`, `false`, `no`) or `CI` set
   (not `0` or `false`): **off**.
2. The user's setting, `telemetry.enabled` in `<dfw home>/config.json` (`dfw telemetry on | off`):
   `false` is **off**; `true` is **on** when an address exists.
3. No address (none compiled in, none set with `dfw telemetry endpoint <url>`): **off**. This is every
   local build and every fork.
4. An address and no setting: the first command prints the notice on stderr and records nothing; from
   the next command on it is **on**. A hook and the MCP server never print the notice (the user would
   not see their stderr), so they record nothing until a command has printed it.

Step 4 is the open question of the design (opt-out or opt-in). It is one constant:
`telemetry::DEFAULT_ON_WITH_ADDRESS` in `src/telemetry/mod.rs`. `true` (today) is opt-out with the
notice; `false` makes step 4 "off until `dfw telemetry on`".

An address the user sets wins over the compiled one, and the compiled headers are sent only to the
compiled address.

## What is sent

One OTLP `LogRecord` per event, `POST <address>/v1/logs`, JSON. The events are a closed enum
(`telemetry::events::Event`); every text field is a name from a list built into the binary (the command
table, the hook names, the hosts, gates, modes, phases, check tools). A value that is not on its list
cannot be put in an event, and a spool line that is not such an event is never sent.

| Event | Fields |
|---|---|
| `dfw.command` | command name (`unknown` for a first argument that is no command), duration range, exit code |
| `dfw.hook` | hook name, host, duration range, outcome kind (`none`, `context`, `deny`, `block`, `forwarded`, `error`) |
| `dfw.gate_approved` | gate id |
| `dfw.checks` | finding count per check tool |
| `dfw.workflow` | mode, phase (after `init`, `mode`, `phase`, `approve`, `reopen`, `pause`, `resume`) |

Resource attributes: `service.name=dfw`, `service.version`, `os.type`, `host.arch`, `dfw.install_id`
(128 random bits, made with the first recorded event, deleted by `dfw telemetry off`). The time of an
event is cut to whole seconds.

`dfw telemetry show` prints the request bodies that are waiting (`--raw`: byte for byte as sent).

## Files

All under `<dfw home>/telemetry/` (`~/.config/dfw/telemetry/`): `spool.jsonl` (events waiting, at most
1 MB, the oldest half is dropped when it is full), `id`, `notice` (the notice was printed),
`last-flush` (its modification time), and the lock folders `.lock` and `.flushing`. A build without an
address never creates the folder.

## Sending

A command appends its events to the spool (under the lock; when the lock is busy for 2 ms the event is
dropped) and, when the last flush was started more than 60 seconds ago, starts `dfw telemetry flush` as
a process of its own and does not wait for it. The flush sends up to 500 events with a 3 second
timeout, follows no redirect, and removes from the spool what the collector accepted (any 2xx).

## The address comes from the build

`build.rs` reads two variables from the build environment and passes them to the crate
(`cargo:rustc-env`); the code reads them with `option_env!`. Unset or empty means no address.

- `DFW_OTLP_ENDPOINT`: the collector, for example `https://otel.example.org` (`/v1/logs` is added).
- `DFW_OTLP_HEADERS`: optional request headers, `name=value,name=value`, for example
  `Authorization=Bearer <ingest token>`.

A value compiled into a binary can be read out of it: the token must be a write-only ingest token,
never an account credential, and the collector should rate-limit by install id.

`dfw __debug telemetry-build` prints whether the running binary has an address and headers (never the
values).

### Release workflow (for the integrator)

The native build step of `.github/workflows/release-binaries.yml` gets the two secrets, on that step
only (not as job or workflow `env`, so no other step sees them):

```yaml
      - name: Build the native binary
        working-directory: native
        env:
          DFW_OTLP_ENDPOINT: ${{ secrets.DFW_OTLP_ENDPOINT }}
          DFW_OTLP_HEADERS: ${{ secrets.DFW_OTLP_HEADERS }}
        run: cargo build --release --target ${{ matrix.target }}
```

and one step after it that fails the release when the binary and the secret disagree (it passes a
boolean, never the secret; drop it for a target that cannot run its own binary):

```yaml
      - name: The binary has the collector address when the secret is set
        env:
          EXPECT_ENDPOINT: ${{ secrets.DFW_OTLP_ENDPOINT != '' }}
        run: |
          built=$(native/target/${{ matrix.target }}/release/dfw __debug telemetry-build)
          echo "$built"
          echo "$built" | grep -q "\"endpoint\":$EXPECT_ENDPOINT"
```

The workflow has no cargo step yet (it still builds the Node binary): `matrix.target` stands for
whatever the native build matrix calls its Rust target.

Nothing changes in `validate.yml`: pull-request builds get no secrets (GitHub passes an empty string),
so they have no address. Do not share a `native/target` cache between the release job and other jobs,
and do not run the release build with `cargo -vv` (it prints the build script's output).

Maintainer: add the two secrets under Settings > Secrets and variables > Actions.

## Left for the integrator

Done at the merge: `dfw doctor` prints the telemetry row after the global config dir, `check` and
`lint` record `dfw.checks` (one `CheckCount` per result whose tool is on the list), and the native
help lists `dfw telemetry status|on|off|endpoint <url>|show|flush` (the conformance help test allows
exactly that line). Still open:

- **Docs** (tasks 4.1, 4.2 of the change): README privacy section and AGENTS.md rule 3.
- `DFW_NATIVE=off` hands every command to Node before the hook dispatcher runs: such hook calls are not
  recorded.
