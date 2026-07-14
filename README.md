# chickadee-rp2350-base

An Embassy-based firmware example for the **RP235xA/B** (Raspberry Pi Pico 2) that routes [`defmt`](https://defmt.rs/) log output over USB CDC-ACM instead of (or in addition to) RTT.

## What it does

- Exposes **two USB CDC-ACM serial ports**:
  - **Port 0** – interactive command shell.
  - **Port 1** – `defmt` log sink (binary framed frames sent to a host decoder).
- Also writes `defmt` frames to **RTT** simultaneously, so probe-rs and a USB host can both receive logs.
- Includes a **vendor USB interface** that accepts a BOOTSEL-reset request, allowing tools to reboot the board into the ROM bootloader without pressing the BOOTSEL button.
- Blinks a `tick` counter via `defmt::info!` once per second.

## Hardware

| Item | Details |
|------|---------|
| MCU  | RP235xA/B (embassy-rp `rp235xa`/`rp235xb` feature) |
| Flash | 2 MiB |
| RAM  | 512 KiB |

## Prerequisites

- Rust toolchain with the `thumbv8m.main-none-eabihf` target:
  ```sh
  rustup target add thumbv8m.main-none-eabihf
  ```
- [`probe-rs`](https://probe.rs/) for flashing / RTT viewing:
  ```sh
  cargo install probe-rs-tools
  ```
- [`defmt-print`](https://github.com/knurling-rs/defmt) or `probe-rs run` for decoding USB defmt frames.

## Building

```sh
cargo build --release
```

## Flashing

```sh
probe-rs run --chip RP2350 target/thumbv8m.main-none-eabihf/release/chickadee-rp2350-base
```

## Viewing logs

**Via RTT (probe-rs):**
```sh
probe-rs attach --chip RP2350 target/thumbv8m.main-none-eabihf/release/chickadee-rp2350-base
```

**Via USB serial (defmt-print):**
```sh
defmt-print -e target/thumbv8m.main-none-eabihf/release/chickadee-rp2350-base < /dev/tty.usbmodem*
```
Select the second enumerated serial port (Port 1, the defmt sink).

## Project structure

```
src/
  main.rs         # Entry point; spawns USB tasks and logs tick counter
  defmt_usb.rs    # Custom defmt global logger (RTT + USB CDC-ACM)
  usb_serial.rs   # USB device setup and task spawning
  resources.rs    # Peripheral resource assignment and interrupt bindings
  reset.rs        # Vendor USB interface for BOOTSEL reset
memory.x          # Linker script for RP2350B flash/RAM layout
build.rs          # Copies memory.x into OUT_DIR for the linker
```

## License

MIT OR Apache-2.0
