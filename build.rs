//! Build script: selects the correct `memory.x` linker script based on the
//! active chip feature (`rp2040`, `rp235xa`, or `rp235xb`) and validates that
//! the Cargo feature and the `--target` triplet agree.
//!
//! | Feature  | `--target` / `[build] target`          |
//! |----------|----------------------------------------|
//! | `rp2040` | `thumbv6m-none-eabi`                   |
//! | `rp235xa`| `thumbv8m.main-none-eabihf`            |
//! | `rp235xa`| `riscv32imac-unknown-none-elf`          |
//! | `rp235xb`| `thumbv8m.main-none-eabihf`            |
//! | `rp235xb`| `riscv32imac-unknown-none-elf`          |

use std::{env, fs::File, io::Write, path::PathBuf};

fn main() {
    let rp2040 = env::var("CARGO_FEATURE_RP2040").is_ok();
    let rp235xa = env::var("CARGO_FEATURE_RP235XA").is_ok();
    let rp235xb = env::var("CARGO_FEATURE_RP235XB").is_ok();

    // Validate feature vs. target triplet.
    // Both must agree or cortex-m PAC register layouts will not match.
    let target = env::var("TARGET").unwrap();
    let is_cm0 = target == "thumbv6m-none-eabi";
    let is_cm33 = target.starts_with("thumbv8m");
    let is_riscv32 = target.starts_with("riscv32");

    if is_riscv32 {
        panic!(
            "\n\nRISC-V (Hazard3) target is not yet supported.\n\
             Interrupt handling infrastructure for RISC-V is not implemented in embassy-rp.\n\
             Use `thumbv8m.main-none-eabihf` for RP2350 builds.\n\
             Current target: {target}\n"
        );
    }

    if rp2040 && !is_cm0 {
        panic!(
            "\n\nFeature `rp2040` requires the Cortex-M0+ target.\n\
             Add `--target thumbv6m-none-eabi` to the build command,\n\
             or set `target = \"thumbv6m-none-eabi\"` in .cargo/config.toml.\n\
             Current target: {target}\n"
        );
    }
    if (rp235xa || rp235xb) && !is_cm33 && !is_riscv32 {
        panic!(
            "\n\nFeatures `rp235xa`/`rp235xb` require either the Cortex-M33 target\n\
             (`thumbv8m.main-none-eabihf`) or the RISC-V target\n\
             (`riscv32imac-unknown-none-elf`).\n\
             Current target: {target}\n"
        );
    }

    let memory_x: &[u8] = match (rp2040, rp235xa || rp235xb, is_riscv32) {
        (true, false, _) => include_bytes!("rp2040.memory.x"),
        (false, true, false) => include_bytes!("rp2350.memory.x"),
        (false, true, true) => include_bytes!("hazard3.memory.x"),
        (true, true, _) => panic!("`rp2040` cannot be combined with `rp235xa`/`rp235xb`"),
        (false, false, _) => panic!("enable exactly one of: `rp2040`, `rp235xa`, `rp235xb`"),
    };

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(memory_x)
        .unwrap();

    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rerun-if-changed=rp2350.memory.x");
    println!("cargo::rerun-if-changed=rp2040.memory.x");
    println!("cargo::rerun-if-changed=hazard3.memory.x");
    println!("cargo::rerun-if-changed=build.rs");
}
