//! The product modules the binary tests take their names from (commands, flags, project paths,
//! fixtures, host ids and the words hooks and telemetry send), included once for every test
//! binary: a test names what the binary reads and writes through the same constants
//! (`testing.md`). Each binary uses a part of them.
#![allow(
    dead_code,
    reason = "each test binary uses a part of the product's names"
)]

#[path = "../../src/core/paths/layout.rs"]
pub mod layout;

#[path = "../../src/core/word.rs"]
pub mod word;

#[path = "../../src/core/fixture.rs"]
pub mod fixture;

#[path = "../../src/core/host/id.rs"]
pub mod host_id;

#[path = "../../src/core/host/wire.rs"]
pub mod wire;

#[path = "../../src/workflow/names.rs"]
pub mod names;

#[path = "../../src/tools/mcp/names.rs"]
pub mod mcp_names;

#[path = "../../src/telemetry/wire.rs"]
pub mod telemetry_wire;
