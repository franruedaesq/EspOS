//! # Chassis HAL
//!
//! Defines the [`RoverChassis`] trait that abstracts over any differential-
//! drive or omnidirectional rover chassis.  The concrete implementation
//! [`L298nChassis`] drives an L298N dual H-bridge via four PWM outputs.

use crate::drivers::l298n::L298nDriver;

// ---------------------------------------------------------------------------
// Trait definition
// ---------------------------------------------------------------------------

/// Abstracts the motion-control interface of a wheeled rover chassis.
///
/// All methods are synchronous because PWM register writes complete in a
/// single bus cycle.  Higher-level motion sequences that require timing
/// (e.g. "drive forward for 2 seconds") are composed in async tasks.
pub trait RoverChassis {
    /// Drive both wheels forward at the given normalised speed.
    ///
    /// `speed` is clamped to `[0.0, 1.0]` where `1.0` = 100 % duty cycle.
    fn move_forward(&mut self, speed: f32);

    /// Drive both wheels in reverse at the given normalised speed.
    ///
    /// `speed` is clamped to `[0.0, 1.0]`.
    fn move_backward(&mut self, speed: f32);

    /// Rotate the chassis in place by `angle` degrees.
    ///
    /// Positive angles are clockwise; negative angles are counter-clockwise.
    /// The implementation spins the wheels at a default turn speed until the
    /// requested angle is achieved (open-loop).
    fn rotate(&mut self, angle: f32);

    /// Apply active braking to both motors (shorts the motor terminals).
    fn brake(&mut self);

    /// Coast to a stop (removes drive without braking).
    fn stop(&mut self);
}

// ---------------------------------------------------------------------------
// L298N concrete implementation
// ---------------------------------------------------------------------------

/// A [`RoverChassis`] backed by an [`L298nDriver`].
///
/// Pin mapping (set at construction time):
/// * `IN1`/`IN2` control the left motor direction.
/// * `IN3`/`IN4` control the right motor direction.
/// * `ENA` / `ENB` are PWM-capable GPIO pins that set motor speed.
pub struct L298nChassis {
    driver: L298nDriver,
    /// Default turn speed as a normalised duty cycle `[0.0, 1.0]`.
    turn_speed: f32,
}

impl L298nChassis {
    /// Create a new chassis wrapper around a pre-configured [`L298nDriver`].
    pub fn new(driver: L298nDriver) -> Self {
        Self {
            driver,
            turn_speed: 0.5,
        }
    }

    /// Override the default normalised turn speed used by [`rotate`](Self::rotate).
    pub fn set_turn_speed(&mut self, speed: f32) {
        self.turn_speed = speed.clamp(0.0, 1.0);
    }
}

impl RoverChassis for L298nChassis {
    fn move_forward(&mut self, speed: f32) {
        let duty = speed.clamp(0.0, 1.0);
        self.driver.set_left(duty, true);
        self.driver.set_right(duty, true);
    }

    fn move_backward(&mut self, speed: f32) {
        let duty = speed.clamp(0.0, 1.0);
        self.driver.set_left(duty, false);
        self.driver.set_right(duty, false);
    }

    fn rotate(&mut self, angle: f32) {
        // Open-loop rotation: spin wheels in opposite directions.
        // A positive angle = clockwise → left wheel forward, right backward.
        let duty = self.turn_speed;
        if angle >= 0.0 {
            self.driver.set_left(duty, true);
            self.driver.set_right(duty, false);
        } else {
            self.driver.set_left(duty, false);
            self.driver.set_right(duty, true);
        }
    }

    fn brake(&mut self) {
        self.driver.brake();
    }

    fn stop(&mut self) {
        self.driver.coast();
    }
}
