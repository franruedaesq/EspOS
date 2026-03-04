# EspOS Installation Guide (MacBook Air M4)

This guide will walk you step-by-step through installing **EspOS** on an **ESP32-S3** using an Apple Silicon Mac (MacBook Air M4). Even if you have never used Rust or programmed a microcontroller before, these instructions are designed for complete beginners.

---

## Prerequisites

Before starting, ensure you have an active internet connection, a terminal window open (press `Cmd + Space`, type `Terminal`, and press `Return`), and a USB-C data cable connected to your ESP32-S3.

### 1. Install Homebrew
Homebrew is a package manager for macOS that makes installing tools easy.
In your terminal, paste the following command and hit `Return`:
```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```
*(If prompted, enter your Mac password and follow the on-screen instructions.)*

### 2. Install Required System Dependencies
The ESP32-S3 tools require a few basic libraries to compile code correctly. Run:
```bash
brew install cmake ninja pkg-config openssl
```

### 3. Install Rust (`rustup`)
Rust is the programming language used for EspOS. Install it using the official script:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
When prompted, press `1` to proceed with the default installation.
Once it finishes, reload your terminal profile by running:
```bash
source "$HOME/.cargo/env"
```

### 4. Install Espressif Rust Tools (`espup`)
The ESP32-S3 requires a specialized Rust toolchain to compile code for its Xtensa or RISC-V architecture. Espressif provides a tool called `espup` to install this.
```bash
cargo install espup
```
Once `espup` is installed, run it to download the toolchain:
```bash
espup install
```
After installation, you need to load the environment variables so your terminal knows where the tools are:
```bash
. $HOME/export-esp.sh
```
*(Note: You will need to run `. $HOME/export-esp.sh` every time you open a new terminal window to work on this project, or add it to your `~/.zshrc` file).*

### 5. Install the Flashing Tool (`cargo-espflash`)
To send the compiled operating system from your Mac to the ESP32-S3, you need `espflash`.
```bash
cargo install cargo-espflash
```

---

## Building and Flashing EspOS

### 1. Clone the Repository
Download the EspOS source code to your Mac.
```bash
git clone https://github.com/your-username/espos.git
cd espos
```
*(Replace the URL with the actual repository URL if it differs).*

### 2. Set Up WiFi Credentials
EspOS needs to connect to your local WiFi network to download commands and send telemetry. The credentials are baked into the firmware when it compiles.
Export your WiFi SSID and Password as environment variables:
```bash
export WIFI_SSID="Your_WiFi_Network_Name"
export WIFI_PASSWORD="Your_WiFi_Password"
```

### 3. Connect the ESP32-S3
Plug your ESP32-S3 board into your MacBook Air M4 using a USB-C cable.
*(Note: Some cables are "charge-only". Ensure you are using a data-sync cable).*

### 4. Build and Flash the Firmware
Now, compile the operating system and flash it to the board. In the `espos` directory, run:
```bash
cargo espflash flash --monitor
```

**What happens next:**
1. Rust will download all required packages (crates).
2. It will compile the EspOS firmware for the ESP32-S3.
3. `espflash` will detect your board (usually on a port like `/dev/cu.usbmodem...`) and upload the firmware.
4. Once flashed, the `--monitor` flag will automatically open a serial monitor so you can see the log output from the ESP32-S3.

You should see logs indicating the ESP32-S3 is booting, connecting to your WiFi, and entering its Idle state!

---

## Troubleshooting

- **"No serial ports found"**: Your Mac cannot see the ESP32-S3. Try a different USB-C cable, or plug it into a different port on the Mac.
- **"Missing WIFI_SSID environment variable"**: You forgot to run `export WIFI_SSID="..."` before running the `cargo espflash flash` command.
- **Compilation Errors**: Ensure you ran `. $HOME/export-esp.sh` so the custom ESP Rust toolchain is active in your terminal.