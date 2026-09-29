//! A virtual-time model of the Jev/Kev decision path, built to be shocked.
//!
//! **Scope, stated once and plainly.** This is a *model* written from the SyberLabs project
//! documents (the RISE request-to-playback design, the local-GPU Kev report and the Kev
//! post-training report). It is not the RISE Worker and it does not call Jev or Kev. What it
//! reproduces are the structural facts those documents state: an 8 second deadline, a Kev
//! scale-to-zero cold start of about 35 seconds, a keyword override that forces the night-drive
//! look, a decision cache, and a menu that answers are validated against. Findings here are
//! findings about *that structure*. Whether the real Worker has the same failure needs the
//! adapter in phase 2 of the roadmap, run against recorded fixtures.

pub mod campaign;
pub mod faults;
pub mod invariants;
pub mod menu;
pub mod provider;
pub mod sim;
pub mod text;
pub mod workload;

pub use campaign::{minimal, sweep, SweepOpts, SweepResult};
pub use faults::{FaultKind, JevFault, Mix};
pub use invariants::default_set;
pub use provider::Profile;
pub use sim::{simulate, Bug, Config, Scenario};
