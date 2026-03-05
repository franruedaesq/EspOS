# EspOS: ESP32-S3 Memory & Power Optimizations

To make EspOS highly efficient, stable, and power-friendly on an ESP32-S3, several critical improvements must be made. These focus heavily on **Memory Management** (balancing limited internal SRAM with external PSRAM) and **Power Consumption** (allowing the CPU and peripherals to enter low-power sleep states).

---

## 1. Memory Management Improvements

### 1.1 Fix Internal SRAM Heap Allocation Size
In `src/memory.rs`, the SRAM heap is hardcoded to 512 KB (`512 * 1024`).
**Issue:** The ESP32-S3 has 512 KB of total internal SRAM, but a significant portion of it is statically allocated for `.data`, `.bss`, the ROM bootloader, interrupt vectors, and the WiFi baseband/MAC. Allocating exactly 512 KB to the heap will result in a stack/heap collision or a linker error when the project scales.
**Fix:** Reduce `SRAM_HEAP_SIZE` dynamically or conservatively (e.g., 128 KB or 256 KB). Alternatively, use the `esp-alloc` crate's `init_heap()` macro with a start and end address obtained directly from the linker script to safely capture only the remaining free RAM.

```rust
// Instead of a hardcoded 512KB static array:
// static mut SRAM_HEAP: MaybeUninit<[u8; SRAM_HEAP_SIZE]> = MaybeUninit::uninit();

// Use the linker script symbols to give exactly the remaining RAM to the allocator:
extern "C" {
    static mut _heap_start: u32;
    static mut _heap_end: u32;
}

pub fn init_heap() {
    let start = unsafe { &mut _heap_start as *mut _ as usize };
    let end = unsafe { &mut _heap_end as *mut _ as usize };
    unsafe {
        ALLOCATOR.init(start as *mut u8, end - start);
    }
}
```

### 1.2 Enable and Utilize PSRAM Correctly
The code in `src/main.rs` comments out PSRAM initialization.
**Issue:** Without PSRAM, large allocations (like the UI Framebuffer and the audio ring buffers in `state_machine.rs`) will immediately exhaust the limited internal SRAM and panic the allocator.
**Fix:**
- Uncomment `memory::init_psram!(peripherals.PSRAM)` in `main.rs`.
- Ensure that the `esp-alloc` implementation differentiates between allocations that require internal SRAM (like DMA buffers for I2S/SPI/WiFi) and those that can use PSRAM. Currently, `esp-alloc` merges them, meaning a DMA driver might accidentally receive an un-DMA-able PSRAM buffer. Use separate allocators or strictly typed arenas if the HAL does not natively handle PSRAM DMA.

### 1.3 Pre-allocate Embassy Task Arenas
**Issue:** In `Cargo.toml`, `embassy-executor` has `task-arena-size-20480` enabled.
**Fix:** Profile the actual stack usage of tasks. The ESP32-S3 has two cores. Task arena sizes can be tuned per executor (Core 0 and Core 1). Only allocate what is needed.

### 1.4 Optimize the Framebuffer Allocation
In `src/tasks/ui.rs`, the framebuffer is allocated dynamically using `alloc::vec![0u8; FRAMEBUFFER_BYTES];`
**Fix:** Instead of relying on a runtime heap allocation that might fragment, use a linker section to place the framebuffer statically into PSRAM at compile time.

```rust
#[link_section = ".ext_ram.bss"]
static mut FRAMEBUFFER: [u8; FRAMEBUFFER_BYTES] = [0; FRAMEBUFFER_BYTES];
```

---

## 2. Power Consumption Improvements

### 2.1 Eliminate Busy Polling (CAN Bus / TWAI)
In `src/tasks/can_bus.rs`, the `can_bus_task` uses a pseudo-polling loop:
```rust
// Simulated RX poll
Timer::after_millis(1).await;
```
**Issue:** Waking up every 1 millisecond prevents the CPU from entering Light Sleep. It forces the core to remain active, drawing ~40-50mA constantly.
**Fix:** Use the `esp-hal` TWAI driver's native asynchronous `receive_async()` method. This allows the Embassy executor to suspend the task entirely and put the CPU to sleep until a hardware interrupt fires when a frame actually arrives.

### 2.2 Enable Automatic Light Sleep
**Issue:** By default, Embassy loops the idle executor on a WFI (Wait For Interrupt) instruction. WFI halts the CPU but keeps the APB and peripheral clocks running.
**Fix:** Integrate `esp-wifi` and `esp-hal` power management to allow the chip to enter **Light Sleep** during idle periods (e.g., when waiting for the 2-second tick or audio triggers in `state_machine.rs`). During Light Sleep, the ESP32-S3 drops to ~1-2mA.
You must enable the RTC timer as the wake-up source and configure the executor to call `esp_hal::rtc_cntl::sleep::light_sleep()` when no tasks are runnable.

### 2.3 Optimize WiFi Power Delivery
In `src/tasks/wifi.rs`, the WiFi controller is started and connected, but power-saving features are not explicitly configured.
**Issue:** An active WiFi PHY on the ESP32-S3 consumes around 240mA during TX and 95mA during RX. If the rover is idle and just listening for LLM IP commands, this is a massive drain.
**Fix:**
- Enable **802.11 Power Save (Modem Sleep)**. This allows the ESP32-S3 to turn off the radio between DTIM beacons from the router, dropping idle WiFi consumption from ~95mA to ~20mA.
- In `ClientConfiguration`, explicitly request power management features if the `esp-wifi` crate supports it.

### 2.4 CPU Clock Scaling
**Issue:** The project boots at the maximum APB clock, usually locking the CPU at 160 MHz or 240 MHz.
**Fix:** If the rover is in the `Idle` state (waiting for an audio trigger), lower the CPU clock to 80 MHz using the `esp-hal` clock control APIs. When an audio trigger occurs and processing begins (`RoverState::Processing`), scale the CPU back to 240 MHz for maximum performance, then drop it back down when reporting is done.

### 2.5 I2C / Sensor Power Management
In `src/tasks/imu.rs`, the IMU is polled at 100 Hz (`Timer::after_millis(10).await`).
**Issue:** Waking up 100 times a second severely limits deep/light sleep opportunities.
**Fix:** Instead of polling the IMU over I2C, configure the MPU-6500 to generate a hardware interrupt on its `INT` pin when a new sample is ready or when a motion threshold is exceeded. Map this pin to an Embassy `ExtiInput` (External Interrupt) and `.wait_for_high().await` on it. This allows the core to sleep between samples.

---

## Summary of Action Plan
1. **Reduce SRAM Heap:** Use linker symbols to assign only the unused RAM to `init_heap()`.
2. **Enable PSRAM:** Uncomment PSRAM initialization and statically place the `Framebuffer` in `.ext_ram.bss`.
3. **Interrupts over Polling:** Replace all `Timer::after_millis()` polling loops in hardware tasks (CAN, IMU) with hardware-backed `_async().await` interrupts.
4. **Modem Sleep:** Enable WiFi Power Save mode to reduce idle radio current.
5. **Light Sleep:** Hook Embassy's idle executor into ESP32-S3's Light Sleep peripheral.
6. **Clock Scaling:** Dynamically lower the CPU clock to 80MHz during the `Idle` state.
