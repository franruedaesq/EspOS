//! # MPU-6500 IMU Driver
//!
//! Communicates with an InvenSense MPU-6500 6-DoF IMU over I²C.
//!
//! ## Register map (subset)
//! | Address | Name          | Description                    |
//! |---------|---------------|--------------------------------|
//! | 0x75    | WHO_AM_I      | Should return 0x70             |
//! | 0x6B    | PWR_MGMT_1    | Power management               |
//! | 0x3B    | ACCEL_XOUT_H  | First accelerometer register   |
//! | 0x43    | GYRO_XOUT_H   | First gyroscope register       |
//!
//! The driver uses the `esp-hal` blocking I²C master.

use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;

use crate::hal::sensor::Vector3D;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default I²C address when AD0 pin is low.
pub const MPU6500_ADDR: u8 = 0x68;
/// Alternative address when AD0 pin is high.
pub const MPU6500_ADDR_ALT: u8 = 0x69;

const REG_WHO_AM_I: u8 = 0x75;
const REG_PWR_MGMT_1: u8 = 0x6B;
const REG_ACCEL_XOUT_H: u8 = 0x3B;
const REG_GYRO_XOUT_H: u8 = 0x43;
const REG_ACCEL_CONFIG: u8 = 0x1C;
const REG_GYRO_CONFIG: u8 = 0x1B;

/// Expected `WHO_AM_I` response for the MPU-6500.
const WHO_AM_I_RESPONSE: u8 = 0x70;

/// Accelerometer scale factor for ±2 g range: 16384 LSB/g.
const ACCEL_SCALE: f32 = 16384.0;
/// Gravity constant in m/s².
const G: f32 = 9.80665;
/// Gyroscope scale factor for ±250 °/s range: 131 LSB/(°/s).
const GYRO_SCALE_DEG: f32 = 131.0;
/// Convert degrees per second to radians per second.
const DEG_TO_RAD: f32 = core::f32::consts::PI / 180.0;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when communicating with the MPU-6500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mpu6500Error {
    /// The I²C transaction failed.
    I2cError,
    /// The `WHO_AM_I` register returned an unexpected value.
    InvalidDeviceId(u8),
}

// ---------------------------------------------------------------------------
// Driver struct
// ---------------------------------------------------------------------------

/// Driver for the MPU-6500 IMU.
///
/// Holds a shared reference to the I²C bus and the device's I²C address.
pub struct Mpu6500<'d> {
    i2c: I2c<'d, Blocking>,
    addr: u8,
}

impl<'d> Mpu6500<'d> {
    /// Create a new driver and verify the device identity.
    ///
    /// # Errors
    /// Returns [`Mpu6500Error::InvalidDeviceId`] if the hardware does not
    /// respond with the expected `WHO_AM_I` value.
    pub fn new(i2c: I2c<'d, Blocking>, addr: u8) -> Result<Self, Mpu6500Error> {
        let mut driver = Self { i2c, addr };
        driver.init()?;
        Ok(driver)
    }

    /// Initialise the sensor: wake from sleep, configure full-scale ranges.
    fn init(&mut self) -> Result<(), Mpu6500Error> {
        // Verify device identity.
        let id = self.read_byte(REG_WHO_AM_I)?;
        if id != WHO_AM_I_RESPONSE {
            return Err(Mpu6500Error::InvalidDeviceId(id));
        }

        // Wake the device (clear SLEEP bit in PWR_MGMT_1).
        self.write_byte(REG_PWR_MGMT_1, 0x00)?;

        // Accel: ±2 g (bits[4:3] = 0b00).
        self.write_byte(REG_ACCEL_CONFIG, 0x00)?;

        // Gyro: ±250 °/s (bits[4:3] = 0b00).
        self.write_byte(REG_GYRO_CONFIG, 0x00)?;

        Ok(())
    }

    // ---- Low-level I²C helpers ------------------------------------------

    fn write_byte(&mut self, reg: u8, value: u8) -> Result<(), Mpu6500Error> {
        self.i2c
            .write(self.addr, &[reg, value])
            .map_err(|_| Mpu6500Error::I2cError)
    }

    fn read_byte(&mut self, reg: u8) -> Result<u8, Mpu6500Error> {
        let mut buf = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[reg], &mut buf)
            .map_err(|_| Mpu6500Error::I2cError)?;
        Ok(buf[0])
    }

    fn read_bytes(&mut self, reg: u8, buf: &mut [u8]) -> Result<(), Mpu6500Error> {
        self.i2c
            .write_read(self.addr, &[reg], buf)
            .map_err(|_| Mpu6500Error::I2cError)
    }

    // ---- High-level reads -----------------------------------------------

    /// Read the three-axis acceleration and return it as a [`Vector3D`] in
    /// m/s².
    pub fn read_accel(&mut self) -> Result<Vector3D, Mpu6500Error> {
        let mut raw = [0u8; 6];
        self.read_bytes(REG_ACCEL_XOUT_H, &mut raw)?;
        let ax = i16::from_be_bytes([raw[0], raw[1]]) as f32 / ACCEL_SCALE * G;
        let ay = i16::from_be_bytes([raw[2], raw[3]]) as f32 / ACCEL_SCALE * G;
        let az = i16::from_be_bytes([raw[4], raw[5]]) as f32 / ACCEL_SCALE * G;
        Ok(Vector3D::new(ax, ay, az))
    }

    /// Read the three-axis angular velocity and return it as a [`Vector3D`]
    /// in rad/s.
    pub fn read_gyro(&mut self) -> Result<Vector3D, Mpu6500Error> {
        let mut raw = [0u8; 6];
        self.read_bytes(REG_GYRO_XOUT_H, &mut raw)?;
        let gx = i16::from_be_bytes([raw[0], raw[1]]) as f32 / GYRO_SCALE_DEG * DEG_TO_RAD;
        let gy = i16::from_be_bytes([raw[2], raw[3]]) as f32 / GYRO_SCALE_DEG * DEG_TO_RAD;
        let gz = i16::from_be_bytes([raw[4], raw[5]]) as f32 / GYRO_SCALE_DEG * DEG_TO_RAD;
        Ok(Vector3D::new(gx, gy, gz))
    }
}
