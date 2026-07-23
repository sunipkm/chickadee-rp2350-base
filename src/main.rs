#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_time::{Duration, Timer};

#[cfg(target_arch = "riscv32")]
use panic_halt as _;
#[cfg(target_arch = "arm")]
use panic_probe as _;

mod resources;
mod usb_serial;

use crate::resources::{AssignedResources, LedDev, UsbDev};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let r = split_resources!(p);

    usb_serial::setup_usb(&spawner, r.usbdev);

    info!("{} ready", env!("CARGO_PKG_NAME"));

    let mut led = Output::new(r.leddev.pin, Level::Low);
    let mut high = false;
    let mut tick: u32 = 0;
    loop {
        if !high {
            led.set_high();
            high = true;
        } else {
            led.set_low();
            high = false;
        }
        Timer::after(Duration::from_secs(1)).await;
        info!("tick {}", tick);
        tick += 1;
    }
}
