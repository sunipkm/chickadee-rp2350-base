# Makefile — build, flash, and run targets for chickadee-rp2350-base.
#
# Chip targets:
#   rp235xa       (default) — RP2350A, Cortex-M33, thumbv8m.main-none-eabihf
#   rp235xb                 — RP2350B, Cortex-M33, thumbv8m.main-none-eabihf
#   rp235xa-riscv           — RP2350A, Hazard3 RISC-V, riscv32imac-unknown-none-elf
#   rp235xb-riscv           — RP2350B, Hazard3 RISC-V, riscv32imac-unknown-none-elf
#   rp2040                  — RP2040,  Cortex-M0+,     thumbv6m-none-eabi
#
# Usage:
#   make                          # build for RP2350A (default)
#   make CHIP=rp235xa-riscv       # build for RP2350A RISC-V
#   make CHIP=rp2040              # build for RP2040
#   make flash                    # build + flash default chip
#   make flash CHIP=rp235xa-riscv # build + flash RP2350A RISC-V
#   make clean                    # remove build artefacts

CHIP ?= rp235xa

# ─── Per-chip settings ───────────────────────────────────────────────────────

ifeq ($(CHIP),rp2040)
  TARGET   := thumbv6m-none-eabi
  FEATURES := --no-default-features --features rp2040
else ifeq ($(CHIP),rp235xb)
  TARGET   := thumbv8m.main-none-eabihf
  FEATURES := --no-default-features --features rp235xb
else ifeq ($(CHIP),rp235xa)
  TARGET   := thumbv8m.main-none-eabihf
  FEATURES := --no-default-features --features rp235xa
else ifeq ($(CHIP),rp235xa-riscv)
  TARGET   := riscv32imac-unknown-none-elf
  FEATURES := --no-default-features --features rp235xa
else ifeq ($(CHIP),rp235xb-riscv)
  TARGET   := riscv32imac-unknown-none-elf
  FEATURES := --no-default-features --features rp235xb
else
  $(error Unknown CHIP "$(CHIP)". Use rp235xa, rp235xb, rp235xa-riscv, rp235xb-riscv, or rp2040.)
endif

CARGO_FLAGS := --target $(TARGET) $(FEATURES)

# ─── Targets ─────────────────────────────────────────────────────────────────

.PHONY: all build flash run clean

all: build

## Build the firmware.
build:
	cargo build $(CARGO_FLAGS)

## Build in release mode.
release:
	cargo build --release $(CARGO_FLAGS)

## Build and flash via the configured runner (embassy-rp-base / probe-rs / picotool).
flash:
	cargo run $(CARGO_FLAGS)

## Build and flash in release mode.
flash-release:
	cargo run --release $(CARGO_FLAGS)

## Remove build artefacts.
clean:
	cargo clean
