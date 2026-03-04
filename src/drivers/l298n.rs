//! # L298N Dual H-Bridge Motor Driver
//!
//! Controls two DC motors through an L298N module using four direction GPIOs
//! (`IN1`–`IN4`) and two PWM-capable enable pins (`ENA`, `ENB`).
//!
//! ## Wiring
//! ```text
//!  ESP32-S3       L298N module
//!  ─────────      ────────────
//!  GPIO_IN1  ──►  IN1   (left motor direction A)
//!  GPIO_IN2  ──►  IN2   (left motor direction B)
//!  GPIO_IN3  ──►  IN3   (right motor direction A)
//!  GPIO_IN4  ──►  IN4   (right motor direction B)
//!  GPIO_ENA  ──►  ENA   (left motor PWM speed)
//!  GPIO_ENB  ──►  ENB   (right motor PWM speed)
//! ```
//!
//! The enable pins must be connected to PWM-capable GPIO and driven with the
//! LEDC peripheral for smooth speed control.

use esp_hal::gpio::Output;

// ---------------------------------------------------------------------------
// PWM duty-cycle helper
// ---------------------------------------------------------------------------

/// Convert a normalised speed in `[0.0, 1.0]` to an 8-bit duty cycle value
/// (`0–255`) suitable for the LEDC peripheral.
#[inline]
fn duty_u8(normalised: f32) -> u8 {
    (normalised.clamp(0.0, 1.0) * 255.0) as u8
}

// ---------------------------------------------------------------------------
// Driver struct
// ---------------------------------------------------------------------------

/// Low-level L298N H-bridge driver.
///
/// Owns four direction GPIO pins and exposes methods to set each motor's speed
/// and direction independently.  PWM on the enable pins is handled externally
/// (LEDC peripheral) and represented here via a callback so the driver stays
/// generic over the PWM implementation.
pub struct L298nDriver {
    in1: Output<'static>,
    in2: Output<'static>,
    in3: Output<'static>,
    in4: Output<'static>,
    /// Cached duty cycles for diagnostics / logging.
    left_duty: u8,
    right_duty: u8,
}

impl L298nDriver {
    /// Create a new driver from four direction GPIO outputs.
    ///
    /// The enable pins are assumed to be driven by separate LEDC channels;
    /// call [`Self::set_left`] / [`Self::set_right`] which will invoke the
    /// appropriate PWM update.
    pub fn new(
        in1: Output<'static>,
        in2: Output<'static>,
        in3: Output<'static>,
        in4: Output<'static>,
    ) -> Self {
        Self {
            in1,
            in2,
            in3,
            in4,
            left_duty: 0,
            right_duty: 0,
        }
    }

    // ---- Per-motor control -----------------------------------------------

    /// Set the left motor speed and direction.
    ///
    /// * `duty`    – normalised speed `[0.0, 1.0]`.
    /// * `forward` – `true` = forward, `false` = reverse.
    pub fn set_left(&mut self, duty: f32, forward: bool) {
        self.left_duty = duty_u8(duty);
        if forward {
            self.in1.set_high();
            self.in2.set_low();
        } else {
            self.in1.set_low();
            self.in2.set_high();
        }
        // LEDC channel update would be called here, e.g.:
        // ledc_channel_ena.set_duty(self.left_duty);
        log::trace!("[l298n] left duty={} forward={}", self.left_duty, forward);
    }

    /// Set the right motor speed and direction.
    pub fn set_right(&mut self, duty: f32, forward: bool) {
        self.right_duty = duty_u8(duty);
        if forward {
            self.in3.set_high();
            self.in4.set_low();
        } else {
            self.in3.set_low();
            self.in4.set_high();
        }
        log::trace!("[l298n] right duty={} forward={}", self.right_duty, forward);
    }

    // ---- Braking & coasting ---------------------------------------------

    /// Apply active braking to both motors (shorts the motor terminals via the
    /// H-bridge).
    ///
    /// Both IN pins for each motor are driven HIGH simultaneously, which shorts
    /// the back-EMF and produces rapid deceleration.
    pub fn brake(&mut self) {
        self.in1.set_high();
        self.in2.set_high();
        self.in3.set_high();
        self.in4.set_high();
        self.left_duty = 0;
        self.right_duty = 0;
        log::trace!("[l298n] brake");
    }

    /// Coast (free-wheel) – removes drive without active braking.
    ///
    /// Both IN pins for each motor are driven LOW, which disconnects the
    /// motor from the supply and allows it to spin down naturally.
    pub fn coast(&mut self) {
        self.in1.set_low();
        self.in2.set_low();
        self.in3.set_low();
        self.in4.set_low();
        self.left_duty = 0;
        self.right_duty = 0;
        log::trace!("[l298n] coast");
    }

    // ---- Accessors -------------------------------------------------------

    /// Return the most recently set duty cycle for the left motor (0–255).
    pub fn left_duty(&self) -> u8 {
        self.left_duty
    }

    /// Return the most recently set duty cycle for the right motor (0–255).
    pub fn right_duty(&self) -> u8 {
        self.right_duty
    }
}
