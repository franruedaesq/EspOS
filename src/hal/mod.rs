//! HAL trait definitions and shared types for EspOS.
//!
//! Re-exports the three hardware-abstraction sub-modules so the rest of the
//! firmware can write `use crate::hal::chassis::RoverChassis` etc.

pub mod chassis;
pub mod sensor;
pub mod audio;
