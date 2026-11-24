//! The names of the workflow's phases and modes, as `.dfw/state.json` and the command line
//! write them. It uses nothing else, so the binary tests include it too
//! (`#[path = "../src/workflow/names.rs"] mod names;`).

/// The phases, in workflow order ([`ALL`](phase::ALL)).
pub mod phase {
    pub const DISCOVERY: &str = "discovery";
    pub const DIRECTION: &str = "direction";
    pub const SYSTEM: &str = "system";
    pub const CONCEPTS: &str = "concepts";
    pub const STACK: &str = "stack";
    pub const IMPLEMENT: &str = "implement";
    pub const VERIFY: &str = "verify";
    pub const AUDIT: &str = "audit";
    pub const DONE: &str = "done";
    /// Every phase, in workflow order.
    pub const ALL: [&str; 9] = [
        DISCOVERY, DIRECTION, SYSTEM, CONCEPTS, STACK, IMPLEMENT, VERIFY, AUDIT, DONE,
    ];
}

/// The modes, by their full names.
pub mod mode {
    pub const NEW_PRODUCT: &str = "new-product";
    pub const EXISTING_PROJECT: &str = "existing-project";
    pub const DESIGN_ONLY: &str = "design-only";
    pub const IMPLEMENT_APPROVED: &str = "implement-approved";
    pub const UI_REFINEMENT: &str = "ui-refinement";
    pub const QUICK_CHANGE: &str = "quick-change";
    pub const REVIEW_ONLY: &str = "review-only";
}
