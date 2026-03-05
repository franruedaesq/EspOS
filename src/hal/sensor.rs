//! # Sensor HAL
//!
//! Defines the sensor traits used throughout EspOS together with the shared
//! [`Vector3D`] type.  Concrete implementations delegate to the low-level
//! drivers in [`crate::drivers`].

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// A three-axis vector used to represent accelerometer, gyroscope, and
/// magnetometer readings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vector3D {
    /// Construct a new vector from its components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Return the Euclidean magnitude of the vector.
    pub fn magnitude(&self) -> f32 {
        libm::sqrtf(self.x * self.x + self.y * self.y + self.z * self.z)
    }

    /// Return the normalised unit vector, or `(0, 0, 0)` if the magnitude is
    /// below the given `epsilon`.
    pub fn normalize(&self, epsilon: f32) -> Self {
        let mag = self.magnitude();
        if mag > epsilon {
            Self::new(self.x / mag, self.y / mag, self.z / mag)
        } else {
            Self::new(0.0, 0.0, 0.0)
        }
    }
}

impl core::ops::Add for Vector3D {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl core::ops::Sub for Vector3D {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

// ---------------------------------------------------------------------------
// Trait definitions
// ---------------------------------------------------------------------------

/// Reads ambient environmental conditions (temperature and humidity).
pub trait EnvironmentalSensor {
    /// Error type returned when a read fails.
    type Error;

    /// Read the ambient temperature in degrees Celsius.
    fn read_temperature(&mut self) -> Result<f32, Self::Error>;

    /// Read the relative humidity as a percentage `[0, 100]`.
    fn read_humidity(&mut self) -> Result<f32, Self::Error>;
}

/// Reads spatial / motion data from an IMU and a time-of-flight distance
/// sensor.
pub trait SpatialSensor {
    /// Error type returned when a read fails.
    type Error;

    /// Read the linear acceleration vector in m/s².
    fn read_acceleration(&mut self) -> Result<Vector3D, Self::Error>;

    /// Read the angular velocity vector in rad/s.
    fn read_gyro(&mut self) -> Result<Vector3D, Self::Error>;

    /// Read the distance to the nearest obstacle in millimetres.
    fn read_distance(&mut self) -> Result<f32, Self::Error>;
}

// ---------------------------------------------------------------------------
// MPU-6500 + VL53L0X combined spatial sensor implementation
// ---------------------------------------------------------------------------

use crate::drivers::{mpu6500::Mpu6500, vl53l0x::Vl53l0x};


/// Combines an MPU-6500 IMU and a VL53L0X distance sensor on the same I²C bus
/// into a single [`SpatialSensor`] implementation.
pub struct CombinedSpatialSensor<'d> {
    imu: Mpu6500<'d>,
    tof: Vl53l0x<'d>,
}

impl<'d> CombinedSpatialSensor<'d> {
    /// Create a combined sensor from pre-configured driver instances.
    pub fn new(imu: Mpu6500<'d>, tof: Vl53l0x<'d>) -> Self {
        Self { imu, tof }
    }
}

impl<'d> SpatialSensor for CombinedSpatialSensor<'d> {
    type Error = crate::drivers::mpu6500::Mpu6500Error;

    fn read_acceleration(&mut self) -> Result<Vector3D, Self::Error> {
        self.imu.read_accel()
    }

    fn read_gyro(&mut self) -> Result<Vector3D, Self::Error> {
        self.imu.read_gyro()
    }

    fn read_distance(&mut self) -> Result<f32, Self::Error> {
        self.tof
            .read_range_mm()
            .map(|mm| mm as f32)
            .map_err(|_| crate::drivers::mpu6500::Mpu6500Error::I2cError)
    }
}
