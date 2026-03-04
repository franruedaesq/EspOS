//! # EspOS – Memory Allocator Setup
//!
//! This module configures two heap regions:
//!
//! * **SRAM heap** – 512 KB of on-chip RAM used for execution stacks, atomic
//!   data structures, and small allocations that must be DMA-accessible.
//! * **PSRAM heap** – 8 MB of external SPI-PSRAM used for large buffers such
//!   as the graphics framebuffer and audio ring buffers.
//!
//! Both regions are exposed through a single [`esp_alloc::EspHeap`] global
//! allocator, which is fully `no_std` compatible.

use core::mem::MaybeUninit;

// ---------------------------------------------------------------------------
// Global allocator
// ---------------------------------------------------------------------------

/// The single global allocator for the entire firmware.
///
/// `EspHeap` supports multiple disjoint memory regions and is safe to use from
/// multiple cores simultaneously (uses a critical section internally).
#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

// ---------------------------------------------------------------------------
// SRAM heap
// ---------------------------------------------------------------------------

/// Size of the SRAM heap region: 512 KiB.
pub const SRAM_HEAP_SIZE: usize = 512 * 1024;

/// Static backing store for the SRAM heap.
///
/// Placed in the default `.bss` / `.data` region, which maps to internal SRAM.
static mut SRAM_HEAP: MaybeUninit<[u8; SRAM_HEAP_SIZE]> = MaybeUninit::uninit();

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the SRAM heap region.
///
/// Must be called **once**, early in `main`, before any heap allocation is
/// attempted.  Calling it more than once is safe but wastes memory.
///
/// # Safety
/// This function writes to the `SRAM_HEAP` static.  It is safe to call once
/// from a single-threaded context during boot.
pub fn init_heap() {
    unsafe {
        ALLOCATOR.init(SRAM_HEAP.as_mut_ptr() as *mut u8, SRAM_HEAP_SIZE);
    }
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
