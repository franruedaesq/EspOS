//! # Audio HAL
//!
//! Defines the [`AudioInput`] and [`AudioOutput`] async traits that abstract
//! over I²S microphone (INMP441) and amplifier (MAX98357A) peripherals.
//!
//! The concrete implementations use the `esp-hal` I²S driver in standard
//! mode (16-bit / 32-bit samples, 44.1 kHz default).

// ---------------------------------------------------------------------------
// Trait definitions
// ---------------------------------------------------------------------------

/// Asynchronous audio capture interface.
///
/// Implementors fill a caller-provided sample buffer by DMA-reading the I²S
/// RX FIFO.  Each sample is a signed 32-bit PCM value (left-justified in the
/// 32-bit word as delivered by the INMP441).
pub trait AudioInput {
    /// Error type returned when a capture fails.
    type Error;

    /// Fill `buf` with signed 32-bit PCM samples from the microphone.
    ///
    /// Returns when `buf` is completely filled or an error occurs.  The
    /// function suspends the current task while DMA is in progress so other
    /// Embassy tasks can run.
    async fn read_samples(&mut self, buf: &mut [i32]) -> Result<(), Self::Error>;

    /// Return the configured sample rate in Hz.
    fn sample_rate(&self) -> u32;

    /// Return the number of channels (1 = mono, 2 = stereo).
    fn channels(&self) -> u8;
}

/// Asynchronous audio playback interface.
///
/// Implementors drain the caller's sample buffer by DMA-writing to the I²S TX
/// FIFO connected to a class-D amplifier such as the MAX98357A.
pub trait AudioOutput {
    /// Error type returned when a playback operation fails.
    type Error;

    /// Transmit all samples in `buf` to the speaker amplifier.
    ///
    /// Blocks the current task (without busy-waiting) until the DMA transfer
    /// completes.
    async fn write_samples(&mut self, buf: &[i32]) -> Result<(), Self::Error>;

    /// Return the configured sample rate in Hz.
    fn sample_rate(&self) -> u32;

    /// Return the number of channels (1 = mono, 2 = stereo).
    fn channels(&self) -> u8;
}

// ---------------------------------------------------------------------------
// INMP441 microphone implementation (I²S RX)
// ---------------------------------------------------------------------------

/// Audio capture driver for the INMP441 MEMS microphone connected via I²S.
///
/// The INMP441 delivers left-justified 24-bit data in a 32-bit frame.  Samples
/// are stored as `i32` with the 24 significant bits in the MSB positions.
pub struct Inmp441Driver {
    sample_rate: u32,
}

impl Inmp441Driver {
    /// Create a new INMP441 driver.
    ///
    /// In a real build this constructor would accept the initialised
    /// `esp_hal::i2s::I2s` peripheral.
    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }
}

impl AudioInput for Inmp441Driver {
    type Error = I2sError;

    async fn read_samples(&mut self, buf: &mut [i32]) -> Result<(), Self::Error> {
        // In a real build: await DMA transfer from the I²S RX peripheral.
        // The DMA descriptor is set up by esp-hal; we just await completion.
        for sample in buf.iter_mut() {
            // Placeholder: zero-fill until the DMA future is wired up.
            *sample = 0i32;
        }
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u8 {
        1 // INMP441 is mono
    }
}

// ---------------------------------------------------------------------------
// MAX98357A amplifier implementation (I²S TX)
// ---------------------------------------------------------------------------

/// Audio playback driver for the MAX98357A class-D amplifier connected via
/// I²S.
pub struct Max98357Driver {
    sample_rate: u32,
}

impl Max98357Driver {
    /// Create a new MAX98357A driver.
    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }
}

impl AudioOutput for Max98357Driver {
    type Error = I2sError;

    async fn write_samples(&mut self, buf: &[i32]) -> Result<(), Self::Error> {
        // In a real build: fill the I²S TX DMA buffer and await completion.
        let _ = buf;
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u8 {
        1
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during I²S audio operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2sError {
    /// The DMA transfer timed out.
    Timeout,
    /// An overrun occurred (RX FIFO full before software drained it).
    Overrun,
    /// An underrun occurred (TX FIFO empty before new data was provided).
    Underrun,
    /// Unrecoverable I²S peripheral error.
    Peripheral,
}
