//! Build script: selects the correct `memory.x` linker script based on the
//! active chip feature (`rp2040`, `rp235xa`, or `rp235xb`) and validates that
//! the Cargo feature and the `--target` triplet agree.
//!
//! | Feature  | `--target` / `[build] target`  |
//! |----------|-------------------------------|
//! | `rp2040` | `thumbv6m-none-eabi`          |
//! | `rp235xa`| `thumbv8m.main-none-eabihf`   |
//! | `rp235xb`| `thumbv8m.main-none-eabihf`   |

use std::{env, fs::File, io::Write, path::PathBuf};

fn main() {
    let rp2040  = env::var("CARGO_FEATURE_RP2040").is_ok();
    let rp235xa = env::var("CARGO_FEATURE_RP235XA").is_ok();
    let rp235xb = env::var("CARGO_FEATURE_RP235XB").is_ok();

    // Validate feature vs. target triplet.
    // Both must agree or cortex-m PAC register layouts will not match.
    let target = env::var("TARGET").unwrap();
    let is_cm0 = target == "thumbv6m-none-eabi";
    let is_cm33 = target.starts_with("thumbv8m");

    if rp2040 && !is_cm0 {
        panic!(
            "\n\nFeature `rp2040` requires the Cortex-M0+ target.\n\
             Add `--target thumbv6m-none-eabi` to the build command,\n\
             or set `target = \"thumbv6m-none-eabi\"` in .cargo/config.toml.\n\
             Current target: {target}\n"
        );
    }
    if (rp235xa || rp235xb) && !is_cm33 {
        panic!(
            "\n\nFeatures `rp235xa`/`rp235xb` require the Cortex-M33 target.\n\
             Add `--target thumbv8m.main-none-eabihf` to the build command,\n\
             or set `target = \"thumbv8m.main-none-eabihf\"` in .cargo/config.toml.\n\
             Current target: {target}\n"
        );
    }

    let memory_x: &[u8] = match (rp2040, rp235xa || rp235xb) {
        (true,  false) => include_bytes!("memory.x.rp2040"),
        (false, true)  => include_bytes!("memory.x.rp2350"),
        (true,  true)  => panic!("`rp2040` cannot be combined with `rp235xa`/`rp235xb`"),
        (false, false) => panic!("enable exactly one of: `rp2040`, `rp235xa`, `rp235xb`"),
    };

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(memory_x)
        .unwrap();

    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rerun-if-changed=memory.x.rp2350");
    println!("cargo::rerun-if-changed=memory.x.rp2040");
    println!("cargo::rerun-if-changed=build.rs");
}
