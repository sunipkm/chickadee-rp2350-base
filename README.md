# embassy-rp-base

An Embassy-based firmware example for **RP2xxx** boards (RP2040, RP2350A/B) that routes [`defmt`](https://defmt.rs/) log output over USB CDC-ACM instead of (or in addition to) RTT.

## What it does

- Exposes **two USB CDC-ACM serial ports**:
  - **Port 0** – interactive command shell.
  - **Port 1** – `defmt` log sink (binary COBS-framed frames sent to a host decoder).
- Also writes `defmt` frames to **RTT** simultaneously, so probe-rs and a USB host can both receive logs.
- Includes a **vendor USB interface** that accepts a BOOTSEL-reset request, allowing tools to reboot the board into the ROM bootloader without pressing the BOOTSEL button.
- Blinks a `tick` counter via `defmt::info!` once per second.

## Supported chips

| Chip | Cortex core | Rust target | Cargo feature |
|------|------------|-------------|---------------|
| RP2040 | M0+ | `thumbv6m-none-eabi` | `rp2040` |
| RP2350A | M33 | `thumbv8m.main-none-eabihf` | `rp235xa` *(default)* |
| RP2350B | M33 | `thumbv8m.main-none-eabihf` | `rp235xb` |

## Prerequisites

Install Rust targets for the chips you intend to use:

```sh
rustup target add thumbv8m.main-none-eabihf   # RP2350
rustup target add thumbv6m-none-eabi           # RP2040
```

[`probe-rs`](https://probe.rs/) for flashing / RTT viewing:

### Linux / macOS

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.sh | sh
```

### Windows

```powershell
irm https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.ps1 | iex
```

[`probe-rp-usb`](https://github.com/sunipkm/probe-rp-usb) — a lightweight runner that flashes and runs RP2xxx boards over USB (no debug probe required):

```sh
cargo install probe-rp-usb
```

[`defmt-print`](https://github.com/knurling-rs/defmt) for decoding USB defmt frames:

```sh
cargo install defmt-print
```

## Building

The `Makefile` keeps the `--target` flag and `--features` flag in sync automatically:

```sh
make                      # RP2350A debug (default)
make CHIP=rp2040          # RP2040 debug
make CHIP=rp235xb         # RP2350B debug
make release              # RP2350A release
make release CHIP=rp2040  # RP2040 release
```

Or directly with Cargo (both flags must be set together):

```sh
cargo build --no-default-features --features rp235xa --target thumbv8m.main-none-eabihf
cargo build --no-default-features --features rp2040  --target thumbv6m-none-eabi
```

`build.rs` will emit a clear error if the feature and target triplet do not match.

## Flashing

```sh
make flash                  # build + flash default chip via configured runner
make flash CHIP=rp2040      # build + flash RP2040
make flash-release          # build release + flash
```

Or directly with a runner:

**`probe-rp-usb`** (USB, no debug probe required — default runner in `.cargo/config.toml`):

```sh
make flash                  # build + flash default chip
make flash CHIP=rp2040      # build + flash RP2040
make flash-release          # build release + flash
```

**`probe-rs`** (requires a debug probe / J-Link / CMSIS-DAP):

```sh
# RP2350
probe-rs run --chip RP2350 target/thumbv8m.main-none-eabihf/release/embassy-rp-base
# RP2040
probe-rs run --chip RP2040 target/thumbv6m-none-eabi/release/embassy-rp-base
```

## Viewing logs

**Via RTT (probe-rs):**

```sh
probe-rs attach --chip RP2350 target/thumbv8m.main-none-eabihf/release/embassy-rp-base
```

**Via USB serial (defmt-print):**

```sh
defmt-print -e target/thumbv8m.main-none-eabihf/release/embassy-rp-base < /dev/tty.usbmodem*
```

Select the second enumerated serial port (Port 1, the defmt sink).

## Project structure

```
src/
  main.rs           # Entry point; spawns USB tasks and logs tick counter
  usb_serial.rs     # USB device setup and task spawning
  resources.rs      # Peripheral resource assignment and interrupt bindings
  reset.rs          # Vendor USB interface for BOOTSEL reset
defmt-embassy-usb/  # Generic defmt USB CDC-ACM transport crate
memory.x.rp2040     # Linker script for RP2040 flash/RAM layout
memory.x.rp2350     # Linker script for RP2350 flash/RAM layout
build.rs            # Selects the correct memory.x and validates feature/target
Makefile            # Convenience targets for each chip
```

## Linux udev

Copy `99-embassy-rp-base.rules` to `/etc/udev/rules.d/` and run:

```sh
sudo udevadm control --reload-rules && sudo udevadm trigger
```

This grants read/write access to the USB device without `sudo`.

## License

MIT OR Apache-2.0

