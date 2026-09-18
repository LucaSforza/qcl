//! Executable, deliberately small AI-containment game form and QCL audit.
//!
//! The transition function is the only containment semantics. Effectivity is
//! enumerated from it; no effectivity rule is hand-written.

mod containment;

pub use containment::{
    Action, Agent, AuditCheck, AuditReport, ContainmentState, ContainmentSystem,
    DerivedEffectivity, JointAction, Scenario, StateSetNames, run_audit, run_simulation,
};

/// Package name used by this showcase.
pub const PACKAGE_NAME: &str = "ai-containment";
