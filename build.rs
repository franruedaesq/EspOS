/// Build script for EspOS.
/// The esp-hal crate manages linker scripts automatically for the ESP32-S3.
/// This build script emits any additional configuration needed at compile time.
fn main() {
    // Ensure the build is re-run if the memory layout changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/memory.rs");
}
