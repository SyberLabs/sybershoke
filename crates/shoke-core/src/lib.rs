//! Sybershoke core.
//!
//! Three ideas, each small enough to read in one sitting:
//!
//! * **A seed is a bug report.** [`Rng`] is a fixed algorithm (SplitMix64), so the same seed
//!   gives the same scenario on every machine.
//! * **Running is separate from checking.** A run produces a [`History`], a list of timed
//!   events that can be written to a text file (`shoke-history/v1`) and read back. Invariants
//!   only ever see the history, so a real system can be checked from a file.
//! * **Failures shrink.** [`shrink`] reduces a failing fault list to a minimal one, removing
//!   faults first and then making the survivors shorter and earlier.

pub mod checker;
pub mod history;
pub mod rng;
pub mod shrink;

pub use checker::{check, Invariant, InvariantResult, Report, Violation};
pub use history::{Event, History, ParseError, FORMAT};
pub use rng::{mix, Rng};
pub use shrink::{shrink, Shrunk, Simplify};
