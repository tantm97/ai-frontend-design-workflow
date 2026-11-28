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

Every crate of `native/Cargo.lock` (108), with the licence its own manifest declares (`cargo metadata` for each release target: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-pc-windows-msvc). Direct dependencies are marked. Regenerate with `node scripts/crate-licenses.mjs`.

| Crate | Version | License | Direct | Linked on |
|---|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |  | all release targets |
| aho-corasick | 1.1.5 | Unlicense OR MIT |  | all release targets |
| alloc-no-stdlib | 3.0.0 | BSD-3-Clause |  | all release targets |
| alloc-stdlib | 0.3.0 | BSD-3-Clause |  | all release targets |
| ascii | 1.1.0 | Apache-2.0 OR MIT |  | no release target (a test-only, build-time or other-platform crate) |
| base64 | 0.23.1 | MIT OR Apache-2.0 |  | all release targets |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |  | all release targets |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 |  | all release targets |
| brotli-decompressor | 6.3.2 | BSD-3-Clause/MIT | yes | all release targets |
| bumpalo | 3.20.4 | MIT OR Apache-2.0 |  | all release targets |
| bytes | 1.12.2 | MIT |  | all release targets |
| cc | 1.11.2 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |  | all release targets |
| cfg_aliases | 0.2.2 | MIT |  | no release target (a test-only, build-time or other-platform crate) |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |  | all release targets |
| chunked_transfer | 1.5.0 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| const-oid | 0.10.3 | Apache-2.0 OR MIT |  | all release targets |
| constant_time_eq | 0.6.1 | CC0-1.0 OR MIT-0 OR Apache-2.0 |  | all release targets |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |  | all release targets |
| crc32fast | 1.5.5 | MIT OR Apache-2.0 |  | all release targets |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 |  | all release targets |
| data-encoding | 2.11.1 | MIT |  | all release targets |
| digest | 0.11.3 | MIT OR Apache-2.0 |  | all release targets |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |  | all release targets |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |  | all release targets |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| flate2 | 1.1.11 | MIT OR Apache-2.0 | yes | all release targets |
| foldhash | 0.2.0 | Zlib |  | all release targets |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |  | all release targets |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |  | all release targets |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |  | all release targets |
| http | 1.5.0 | MIT OR Apache-2.0 |  | all release targets |
| httparse | 1.10.1 | MIT OR Apache-2.0 |  | all release targets |
| httpdate | 1.0.3 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 |  | all release targets |
| include_dir | 0.7.4 | MIT | yes | all release targets |
| include_dir_macros | 0.7.4 | MIT |  | all release targets |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |  | all release targets |
| itoa | 1.0.19 | MIT OR Apache-2.0 |  | all release targets |
| js-sys | 0.3.107 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| libc | 0.2.191 | MIT OR Apache-2.0 |  | aarch64-apple-darwin, aarch64-unknown-linux-gnu, x86_64-apple-darwin, x86_64-unknown-linux-gnu |
| log | 0.4.35 | MIT OR Apache-2.0 |  | all release targets |
| memchr | 2.8.3 | Unlicense OR MIT |  | all release targets |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |  | all release targets |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |  | all release targets |
| nix | 0.32.0 | MIT |  | aarch64-apple-darwin, x86_64-apple-darwin |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |  | all release targets |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |  | all release targets |
| png | 0.18.1 | MIT OR Apache-2.0 | yes | all release targets |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |  | all release targets |
| quote | 1.0.47 | MIT OR Apache-2.0 |  | all release targets |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |  | no release target (a test-only, build-time or other-platform crate) |
| rand | 0.10.4 | MIT OR Apache-2.0 |  | all release targets |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |  | all release targets |
| regex | 1.13.2 | MIT OR Apache-2.0 | yes | all release targets |
| regex-automata | 0.4.19 | MIT OR Apache-2.0 |  | all release targets |
| regex-syntax | 0.8.12 | MIT OR Apache-2.0 |  | all release targets |
| ring | 0.17.14 | Apache-2.0 AND ISC |  | all release targets |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT |  | all release targets |
| rustls-pki-types | 1.16.0 | MIT OR Apache-2.0 |  | all release targets |
| rustls-webpki | 0.103.17 | ISC |  | all release targets |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| serde | 1.0.229 | MIT OR Apache-2.0 | yes | all release targets |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |  | all release targets |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |  | all release targets |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | yes | all release targets |
| sha1 | 0.11.0 | MIT OR Apache-2.0 |  | all release targets |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | yes | all release targets |
| shlex | 2.0.1 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| simd-adler32 | 0.3.10 | MIT |  | all release targets |
| subtle | 2.6.1 | BSD-3-Clause |  | all release targets |
| syn | 2.0.119 | MIT OR Apache-2.0 |  | all release targets |
| syn | 3.0.10 | MIT OR Apache-2.0 |  | all release targets |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |  | all release targets |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |  | all release targets |
| tiny_http | 0.12.0 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| tungstenite | 0.30.0 | MIT OR Apache-2.0 | yes | all release targets |
| typed-path | 0.13.0 | MIT OR Apache-2.0 |  | all release targets |
| typenum | 1.20.1 | MIT OR Apache-2.0 |  | all release targets |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |  | all release targets |
| untrusted | 0.9.0 | ISC |  | all release targets |
| ureq | 3.5.2 | MIT OR Apache-2.0 | yes | all release targets |
| ureq-proto | 0.7.1 | MIT OR Apache-2.0 |  | all release targets |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 |  | all release targets |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |  | no release target (a test-only, build-time or other-platform crate) |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| webpki-roots | 1.0.12 | CDLA-Permissive-2.0 |  | all release targets |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |  | x86_64-pc-windows-msvc |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |  | x86_64-pc-windows-msvc |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |  | no release target (a test-only, build-time or other-platform crate) |
| zeroize | 1.9.2 | Apache-2.0 OR MIT |  | all release targets |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT |  | all release targets |
| zip | 9.7.0 | MIT | yes | all release targets |
| zlib-rs | 0.6.8 | Zlib |  | all release targets |
| zmij | 1.0.23 | MIT |  | all release targets |
| zopfli | 0.8.4 | Apache-2.0 |  | all release targets |
