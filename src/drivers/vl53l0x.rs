//! # VL53L0X Time-of-Flight Distance Sensor Driver
//!
//! Communicates with an ST VL53L0X single-photon avalanche diode (SPAD) ToF
//! sensor over I²C.  The driver implements the minimal register sequence
//! required to perform a single-shot range measurement.
//!
//! ## Measurement modes
//! * **Single-shot** – trigger one measurement on demand (used here).
//! * **Continuous** – the sensor streams measurements at a configured rate.
//!
//! ## Typical usage
//! ```rust,no_run
//! let mut tof = Vl53l0x::new(i2c, VL53L0X_ADDR)?;
//! let dist_mm = tof.read_range_mm()?;
//! ```

use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default I²C address of the VL53L0X (XSHUT floating or high).
pub const VL53L0X_ADDR: u8 = 0x29;

// Register addresses (subset).
const REG_IDENTIFICATION_MODEL_ID: u8 = 0xC0;
const REG_SYSRANGE_START: u8 = 0x00;
const REG_RESULT_INTERRUPT_STATUS: u8 = 0x13;
const REG_RESULT_RANGE_STATUS: u8 = 0x14;
const REG_SYSTEM_INTERRUPT_CLEAR: u8 = 0x0B;
const REG_VHV_CONFIG_PAD_SCL_SDA_EXTSUP_HV: u8 = 0x89;
const REG_MSRC_CONFIG_CONTROL: u8 = 0x60;
const REG_FINAL_RANGE_CONFIG_MIN_COUNT_RATE_RTN_LIMIT: u8 = 0x44;
const REG_SYSTEM_SEQUENCE_CONFIG: u8 = 0x01;
const REG_GLOBAL_CONFIG_VCSEL_WIDTH: u8 = 0x32;

/// Expected `IDENTIFICATION_MODEL_ID` value.
const MODEL_ID: u8 = 0xEE;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by the VL53L0X driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vl53l0xError {
    /// I²C communication error.
    I2cError,
    /// The device returned an unexpected model ID.
    InvalidModelId(u8),
    /// The measurement timed out before a result was ready.
    Timeout,
    /// The range result indicates an error (phase failure, sigma too high …).
    RangeError(u8),
}

// ---------------------------------------------------------------------------
// Driver struct
// ---------------------------------------------------------------------------

/// Driver for the VL53L0X ToF distance sensor.
pub struct Vl53l0x<'d> {
    i2c: I2c<'d, Blocking>,
    addr: u8,
    /// IO mode: 0 = 1.8 V, 1 = 2.8 V.
    io_2v8: bool,
}

impl<'d> Vl53l0x<'d> {
    /// Create a new driver, verify device identity, and perform the mandatory
    /// initialisation sequence.
    pub fn new(i2c: I2c<'d, Blocking>, addr: u8) -> Result<Self, Vl53l0xError> {
        let mut driver = Self {
            i2c,
            addr,
            io_2v8: true,
        };
        driver.init()?;
        Ok(driver)
    }

    // ---- Initialisation --------------------------------------------------

    fn init(&mut self) -> Result<(), Vl53l0xError> {
        let id = self.read_byte(REG_IDENTIFICATION_MODEL_ID)?;
        if id != MODEL_ID {
            return Err(Vl53l0xError::InvalidModelId(id));
        }

        // Configure IO voltage level.
        if self.io_2v8 {
            let val = self.read_byte(REG_VHV_CONFIG_PAD_SCL_SDA_EXTSUP_HV)?;
            self.write_byte(REG_VHV_CONFIG_PAD_SCL_SDA_EXTSUP_HV, val | 0x01)?;
        }

        // Standard initialisation sequence (abbreviated).
        self.write_byte(0x88, 0x00)?;
        self.write_byte(0x80, 0x01)?;
        self.write_byte(0xFF, 0x01)?;
        // The following two writes to register 0x00 are required by the
        // ST undocumented calibration sequence: first clear the register to
        // deselect the private page, then assert bit 0 to latch the
        // calibration result before restoring normal register access.
        self.write_byte(0x00, 0x00)?;
        self.write_byte(0x00, 0x01)?;
        self.write_byte(0xFF, 0x00)?;
        self.write_byte(0x80, 0x00)?;

        // Disable SIGNAL_RATE_MSRC and SIGNAL_RATE_PRE_RANGE limit checks.
        let msrc = self.read_byte(REG_MSRC_CONFIG_CONTROL)?;
        self.write_byte(REG_MSRC_CONFIG_CONTROL, msrc | 0x12)?;

        // Set signal rate limit to 0.25 MCPS (units: 9.7 fixed-point).
        self.write_u16(REG_FINAL_RANGE_CONFIG_MIN_COUNT_RATE_RTN_LIMIT, 0x0020)?;

        self.write_byte(REG_SYSTEM_SEQUENCE_CONFIG, 0xFF)?;

        Ok(())
    }

    // ---- Single-shot measurement -----------------------------------------

    /// Trigger a single-shot range measurement and return the result in
    /// millimetres.
    ///
    /// This is a blocking call – it polls the `RESULT_INTERRUPT_STATUS`
    /// register up to 200 times with a ~1 ms delay each.
    pub fn read_range_mm(&mut self) -> Result<u16, Vl53l0xError> {
        // Trigger measurement.
        self.write_byte(REG_SYSRANGE_START, 0x01)?;

        // Poll until data ready (bit 2 of interrupt status).
        let mut tries = 0u32;
        loop {
            let status = self.read_byte(REG_RESULT_INTERRUPT_STATUS)?;
            if (status & 0x07) != 0 {
                break;
            }
            tries += 1;
            if tries > 200 {
                return Err(Vl53l0xError::Timeout);
            }
            // Busy-wait ~1 ms (rough approximation without a timer available
            // in this blocking context).
            for _ in 0..10_000u32 {
                core::hint::spin_loop();
            }
        }

        // Read result (bytes 10–11 of the RESULT_RANGE_STATUS block).
        let mut buf = [0u8; 12];
        self.read_bytes(REG_RESULT_RANGE_STATUS, &mut buf)?;

        // Clear interrupt.
        self.write_byte(REG_SYSTEM_INTERRUPT_CLEAR, 0x01)?;

        // Range status byte 0, bits[6:3]: 0 = valid range.
        let range_status = (buf[0] >> 3) & 0x0F;
        if range_status != 0 {
            return Err(Vl53l0xError::RangeError(range_status));
        }

        let range_mm = u16::from_be_bytes([buf[10], buf[11]]);
        Ok(range_mm)
    }

    // ---- Low-level I²C helpers ------------------------------------------

    fn write_byte(&mut self, reg: u8, val: u8) -> Result<(), Vl53l0xError> {
        self.i2c
            .write(self.addr, &[reg, val])
            .map_err(|_| Vl53l0xError::I2cError)
    }

    fn write_u16(&mut self, reg: u8, val: u16) -> Result<(), Vl53l0xError> {
        let bytes = val.to_be_bytes();
        self.i2c
            .write(self.addr, &[reg, bytes[0], bytes[1]])
            .map_err(|_| Vl53l0xError::I2cError)
    }

    fn read_byte(&mut self, reg: u8) -> Result<u8, Vl53l0xError> {
        let mut buf = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[reg], &mut buf)
            .map_err(|_| Vl53l0xError::I2cError)?;
        Ok(buf[0])
    }

    fn read_bytes(&mut self, reg: u8, buf: &mut [u8]) -> Result<(), Vl53l0xError> {
        self.i2c
            .write_read(self.addr, &[reg], buf)
            .map_err(|_| Vl53l0xError::I2cError)
    }
}
