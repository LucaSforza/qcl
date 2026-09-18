//! Executable, deliberately small AI-containment game form and QCL audit.
//!
//! The transition function is the only containment semantics. Effectivity is
//! enumerated from it; no effectivity rule is hand-written.

mod containment;
mod lab;
mod live;

pub use containment::{
    Action, Agent, AuditCheck, AuditReport, ContainmentState, ContainmentSystem,
    DerivedEffectivity, JointAction, Scenario, StateSetNames, run_audit, run_simulation,
};
pub use lab::{ContainmentLab, LabConfig, LabError, LabScenario, ObservedEvent, ObservedOutcome};
pub use live::{
    LiveAction, LiveAgent, LiveDerivedModel, LiveJointAction, LiveScenario, LiveState,
    ObservedGameForm, ObservedTransition, ObservedTransitionTable, run_live_audit, run_live_demo,
    run_live_extract,
};

/// Package name used by this showcase.
pub const PACKAGE_NAME: &str = "ai-containment";
