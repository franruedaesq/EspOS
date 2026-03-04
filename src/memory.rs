//! # EspOS – Memory Allocator Setup
//!
//! This module configures two heap regions:
//!
//! * **SRAM heap** – 128 KB of on-chip RAM used for execution stacks, atomic
//!   data structures, and small allocations that must be DMA-accessible.
//! * **PSRAM heap** – 8 MB of external SPI-PSRAM used for large buffers such
//!   as the graphics framebuffer and audio ring buffers.
//!
//! Both regions are exposed through `esp_alloc`, which declares a
//! `#[global_allocator]` internally.

/// Size of the SRAM heap region: 128 KiB.
/// The dram_seg is ~338 KiB total; leave room for stacks, .bss, and .data.
/// Use PSRAM for larger allocations once enabled.
pub const SRAM_HEAP_SIZE: usize = 128 * 1024;

/// Initialise the SRAM heap region using the esp_alloc macro.
///
/// Must be called **once**, early in `main`, before any heap allocation is
/// attempted.
pub fn init_heap() {
    esp_alloc::heap_allocator!(SRAM_HEAP_SIZE);
}

/// Initialise the PSRAM heap region (8 MiB of external SPI-PSRAM).
///
/// This macro wraps [`esp_alloc::psram_allocator!`] to provide a consistent
/// `memory::` namespace and keep the PSRAM boot sequence co-located with the
/// SRAM initialisation above.  It configures the PSRAM controller via
/// `esp_hal::psram` and adds the external memory window to the existing global
/// allocator so that large allocations automatically use PSRAM.
///
/// Call this **after** [`init_heap`] and after the PSRAM peripheral has been
/// enabled via `esp_hal::psram`.
///
/// # Example
/// ```rust,no_run
/// memory::init_heap();
/// memory::init_psram!(peripherals.PSRAM);
/// ```
#[macro_export]
macro_rules! init_psram {
    ($psram_peripheral:expr) => {
        esp_alloc::psram_allocator!($psram_peripheral, esp_hal::psram);
    };
}

/// Convenience re-export so callers can write `memory::init_psram!(...)`.
pub use init_psram;

// ---------------------------------------------------------------------------
// Runtime heap statistics
// ---------------------------------------------------------------------------

/// Returns the number of bytes currently allocated from the SRAM heap.
pub fn heap_used() -> usize {
    esp_alloc::HEAP.used()
}

/// Returns the number of bytes not yet allocated in the SRAM heap.
pub fn heap_free() -> usize {
    esp_alloc::HEAP.free()
}
