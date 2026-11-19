# Third-party notices

This repository vendors unmodified copies of the following projects (adapted only by the declarative overlays in `overlays/`, applied at build time). Each plugin directory contains the upstream license file(s) and an `UPSTREAM.json` with the exact commit.

| Plugin | Upstream | Commit | License | Author |
|---|---|---|---|---|
| frontend-design | https://github.com/anthropics/skills | `2c5958ecac7c` | Apache-2.0 | Anthropic |
| ux-research | https://github.com/anthropics/knowledge-work-plugins | `02717ef04446` | Apache-2.0 | Anthropic |
| ui-ux-pro-max | https://github.com/nextlevelbuilder/ui-ux-pro-max-skill | `6ae86f61f520` | MIT | Next Level Builder |
| impeccable | https://github.com/pbakaus/impeccable | `c4cd1924c389` | Apache-2.0 | Paul Bakaus |
| emil-design-eng | https://github.com/emilkowalski/skills | `f7ce8c02d6e3` | MIT | Emil Kowalski |
| hallmark | https://github.com/nutlope/hallmark | `04977e15f3df` | MIT | Nutlope |
| taste-reference | https://github.com/Leonxlnx/taste-skill | `f6b76575f949` | MIT | Leonxlnx |

The `dfw` plugin, the `dfw` binary (the Rust crate in `native/`), scripts, overlays and policies are original work under the MIT license (see LICENSE).

## Code and data in the dfw binary

| Item | Upstream | License | How it is used |
|---|---|---|---|
| pixelmatch 8.0.0 | https://github.com/mapbox/pixelmatch (Copyright (c) 2025, Mapbox) | ISC | Ported to Rust for `dfw diff` (`native/src/browser/pixeldiff.rs`); the original is kept in `core/assets/vendor/pixelmatch.js` with its licence |
| Lucide icons | https://github.com/lucide-icons/lucide (Copyright (c) Lucide Icons and Contributors) | ISC | The icon set of `dfw icons`, vendored at tag 1.64.0 in `core/assets/icons/` (licence in `LICENSE-lucide`) and embedded in the binary |

Downloaded on the user's machine, not shipped: axe-core 4.14.0 (https://github.com/dequelabs/axe-core, MPL-2.0), fetched by `dfw setup --a11y` (or `dfw install`) from the npm registry with its integrity hash checked and injected unmodified into rendered pages by `dfw check`; Google's chrome-headless-shell (Chrome for Testing, BSD-3-Clause and its own notices), fetched only by `dfw setup --browser`, or by `dfw install` when no Chromium-based browser is found (`--no-runtime` skips it). dfw otherwise drives an already-installed Chromium-based browser.

## Rust crates in the dfw binary

Every crate of `native/Cargo.lock` (108), with the licence its own manifest declares (`cargo metadata`, or the crate's Cargo.toml in the local cargo registry). Direct dependencies are marked. A licence that could not be read is listed as "not recorded"; it is never guessed. Regenerate with `node scripts/crate-licenses.mjs`.

| Crate | Version | License | Direct | Linked on |
|---|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| aho-corasick | 1.1.5 | Unlicense OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| alloc-no-stdlib | 3.0.0 | BSD-3-Clause |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| alloc-stdlib | 0.3.0 | BSD-3-Clause |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| ascii | 1.1.0 | Apache-2.0 OR MIT |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| base64 | 0.23.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| brotli-decompressor | 6.3.2 | BSD-3-Clause/MIT | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| bumpalo | 3.20.4 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| bytes | 1.12.2 | MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| cc | 1.11.2 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| cfg_aliases | 0.2.2 | MIT |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| chunked_transfer | 1.5.0 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| const-oid | 0.10.3 | Apache-2.0 OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| constant_time_eq | 0.6.1 | CC0-1.0 OR MIT-0 OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| crc32fast | 1.5.5 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| data-encoding | 2.11.1 | MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| digest | 0.11.3 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| flate2 | 1.1.11 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| foldhash | 0.2.0 | Zlib |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| http | 1.5.0 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| httparse | 1.10.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| httpdate | 1.0.3 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| include_dir | 0.7.4 | MIT | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| include_dir_macros | 0.7.4 | MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| itoa | 1.0.19 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| js-sys | 0.3.107 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| libc | 0.2.191 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| log | 0.4.35 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| memchr | 2.8.3 | Unlicense OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| nix | 0.32.0 | MIT |  | aarch64-apple-darwin, x86_64-apple-darwin |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| png | 0.18.1 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| quote | 1.0.47 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| r-efi | 6.0.0 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| rand | 0.10.4 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| regex | 1.13.2 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| regex-automata | 0.4.19 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| regex-syntax | 0.8.12 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| ring | 0.17.14 | Apache-2.0 AND ISC |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| rustls-pki-types | 1.16.0 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| rustls-webpki | 0.103.17 | ISC |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| serde | 1.0.229 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| sha1 | 0.11.0 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| shlex | 2.0.1 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| simd-adler32 | 0.3.10 | MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| subtle | 2.6.1 | BSD-3-Clause |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| syn | 2.0.119 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| syn | 3.0.10 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| tiny_http | 0.12.0 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| tungstenite | 0.30.0 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| typed-path | 0.13.0 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| typenum | 1.20.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| untrusted | 0.9.0 | ISC |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| ureq | 3.5.2 | MIT OR Apache-2.0 | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| ureq-proto | 0.7.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| wasi | 0.11.1+wasi-snapshot-preview1 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| wasm-bindgen | 0.2.130 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| wasm-bindgen-macro-support | 0.2.130 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| webpki-roots | 1.0.12 | CDLA-Permissive-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows-sys | 0.52.0 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows-sys | 0.61.2 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_aarch64_gnullvm | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_aarch64_msvc | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_i686_gnu | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_i686_gnullvm | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_i686_msvc | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_x86_64_gnu | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_x86_64_gnullvm | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| windows_x86_64_msvc | 0.52.6 | not recorded (no licence field could be read) |  | not on the resolved targets (x86_64-pc-windows-msvc not resolved offline; a test-only, build-time, Windows-only or other-platform crate) |
| zeroize | 1.9.2 | Apache-2.0 OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| zip | 9.7.0 | MIT | yes | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| zlib-rs | 0.6.8 | Zlib |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| zmij | 1.0.23 | MIT |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| zopfli | 0.8.4 | Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
