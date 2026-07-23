//! USB serial command interface.
//!
//! [`setup_usb`] wires up three things on the single USB device:
//!   - A vendor interface that accepts the BOOTSEL reset request ([`crate::reset`]).
//!   - A CDC-ACM interface for the interactive command shell.
//!   - A CDC-ACM interface used exclusively as the defmt log sink.
//!
//! Call [`setup_usb`] once from `main` after initialising the peripherals.

use defmt::{error, info, trace};
use embassy_executor::Spawner;
use embassy_rp::{peripherals::USB, usb::Driver as UsbDriver};
use embassy_time::{Duration, Timer};
use embassy_usb::{
    Config, UsbDevice,
    class::cdc_acm::{CdcAcmClass, State as CdcAcmState},
};
use heapless::String;
use static_cell::StaticCell;

use crate::resources;

// ─────────────────────────────────────────────────────────────────────────────
// Type aliases
// ─────────────────────────────────────────────────────────────────────────────

type SerialDev = CdcAcmClass<'static, UsbDriver<'static, USB>>;
type UsbDevType = UsbDevice<'static, UsbDriver<'static, USB>>;
/// Concrete type for the defmt drain task argument.
type DefmtTask = embassy_defmt_usb::UsbDefmtTask<UsbDriver<'static, USB>>;

// ─────────────────────────────────────────────────────────────────────────────
// Setup
// ─────────────────────────────────────────────────────────────────────────────

/// Initialise the USB device and spawn all USB-related tasks.
///
/// Must be called once from `main` before entering the application loop.
pub fn setup_usb(spawner: &Spawner, usbdev: resources::UsbDev) {
    static USB_DEVICE: StaticCell<UsbDevType> = StaticCell::new();
    static SERIAL_STATE: StaticCell<CdcAcmState> = StaticCell::new();
    static SERIAL_DEV: StaticCell<SerialDev> = StaticCell::new();

    let driver = UsbDriver::new(usbdev.usb, resources::Irqs);

    let mut config = Config::new(0xc0de, 0xaa00);
    config.manufacturer = Some("Example");
    config.product = Some("embassy-rp-base");
    config.serial_number = Some("0001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    let mut builder = rp_usb_reset::build_usb_builder!(driver, config);

    // CDC-ACM #0: interactive command shell.
    let serial = SERIAL_DEV.init(CdcAcmClass::new(
        &mut builder,
        SERIAL_STATE.init(CdcAcmState::new()),
        64,
    ));

    // CDC-ACM #1: defmt log sink — interface and "defmt" string handler
    // are registered by UsbDefmtLogger::build().
    let defmt_task = embassy_defmt_usb::UsbDefmtLogger::new().build(&mut builder);

    // Finalise the USB device and get a handle to it.
    let usb = USB_DEVICE.init(builder.build());

    // Spawn the USB device task, defmt drain task, and serial command task.
    spawner.spawn(usb_device_task(usb).unwrap());
    spawner.spawn(defmt_drain_task(defmt_task).unwrap());
    spawner.spawn(serial_task(serial).unwrap());
}

// ─────────────────────────────────────────────────────────────────────────────
// Embassy tasks
// ─────────────────────────────────────────────────────────────────────────────

#[embassy_executor::task]
// Embassy task that runs the USB device. Must be spawned with a reference to the `UsbDevice` returned by `setup_usb`.
async fn usb_device_task(usb: &'static mut UsbDevType) {
    usb.run().await;
}

/// Embassy task that drains defmt bytes over the CDC-ACM bulk-IN endpoint.
#[embassy_executor::task]
async fn defmt_drain_task(task: DefmtTask) {
    task.run().await;
}

/// USB serial command shell task.
///
/// Waits for a host connection, prints a prompt, then dispatches commands
/// line-by-line.  Reconnects automatically after a disconnect.
#[embassy_executor::task]
async fn serial_task(dev: &'static mut SerialDev) {
    let mut rx_buf = [0u8; 64];
    let mut line = String::<64>::new();

    loop {
        dev.wait_connection().await;
        trace!("Serial connected");
        dev.write_message(b"embassy-rp-base\r\n> ").await;

        loop {
            let Ok(n) = dev.read_packet(&mut rx_buf).await else {
                break;
            };
            let Ok(s) = core::str::from_utf8(&rx_buf[..n]) else {
                error!("Received invalid UTF-8");
                continue;
            };
            let Some(()) = dev.process_input(&mut line, s).await else {
                continue;
            };

            dispatch(dev, line.as_str()).await;
            line.clear();
            dev.write_message(b"\r\n> ").await;
        }

        trace!("Serial disconnected");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Command dispatch
// ─────────────────────────────────────────────────────────────────────────────

enum Command<'a> {
    Help,
    Version,
    Reset,
    Clear,
    Unknown(&'a str),
}

fn parse_command(s: &str) -> Command<'_> {
    match s.trim() {
        "help" | "h" | "?" => Command::Help,
        "version" | "ver" => Command::Version,
        "reset" => Command::Reset,
        "clear" => Command::Clear,
        other => Command::Unknown(other),
    }
}

async fn dispatch(dev: &mut SerialDev, line: &str) {
    match parse_command(line) {
        Command::Help => print_help(dev).await,
        Command::Version => {
            dev.write_message(b"embassy-rp-base v").await;
            dev.write_message(env!("CARGO_PKG_VERSION").as_bytes())
                .await;
            dev.write_message(b"\r\n").await;
        }
        Command::Reset => {
            dev.write_message(b"Resetting...\r\n").await;
            Timer::after(Duration::from_millis(200)).await;
            #[cfg(target_arch = "arm")]
            cortex_m::peripheral::SCB::sys_reset();
            #[cfg(not(target_arch = "arm"))]
            panic!("soft reset");
        }
        Command::Clear => {
            // ANSI: clear screen + move cursor to home.
            dev.write_message(b"\x1b[2J\x1b[H").await;
        }
        Command::Unknown(cmd) => {
            dev.write_message(b"Unknown command: ").await;
            dev.write_message(cmd.as_bytes()).await;
            dev.write_message(b"\r\nType 'help' for available commands.\r\n")
                .await;
        }
    }
}

async fn print_help(dev: &mut SerialDev) {
    dev.write_message(env!("CARGO_PKG_NAME").as_bytes()).await;
    dev.write_message(b" v").await;
    dev.write_message(env!("CARGO_PKG_VERSION").as_bytes())
        .await;
    dev.write_message(b"\r\n---\r\n").await;
    dev.write_message(b"  help / h / ?  show this message\r\n")
        .await;
    dev.write_message(b"  version        show firmware version\r\n")
        .await;
    dev.write_message(b"  reset          soft-reset the device\r\n")
        .await;
    dev.write_message(b"  clear          clear the terminal screen\r\n")
        .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// CDC-ACM helper trait
// ─────────────────────────────────────────────────────────────────────────────

trait AcmExt {
    /// Write `bytes` in ≤ 32-byte chunks (CDC-ACM max packet size constraint).
    async fn write_message(&mut self, bytes: &[u8]);

    /// Accumulate one USB packet worth of characters into `msg`.
    ///
    /// Returns `Some(())` when a complete line (CR / LF) has been received and
    /// the caller should dispatch it.  Handles backspace, delete, and ANSI
    /// escape sequences.
    async fn process_input<const N: usize>(
        &mut self,
        msg: &mut String<N>,
        input: &str,
    ) -> Option<()>;
}

impl AcmExt for SerialDev {
    async fn write_message(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(32) {
            let _ = self.write_packet(chunk).await;
        }
    }

    async fn process_input<const N: usize>(
        &mut self,
        msg: &mut String<N>,
        input: &str,
    ) -> Option<()> {
        let mut skip = 0usize;
        for c in input.chars() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if c.is_control() {
                match c {
                    // Backspace / Delete
                    '\x08' | '\x7f' if msg.pop().is_some() => {
                        self.write_message(b"\x08 \x08").await;
                    }
                    // Line feed / Carriage return → dispatch
                    '\n' | '\r' => {
                        if !msg.is_empty() {
                            info!("Command: {=[u8]:a}", msg.as_bytes());
                            self.write_message(b"\r\n").await;
                            return Some(());
                        } else {
                            self.write_message(b"\r\n> ").await;
                        }
                    }
                    // ESC: skip the following two characters (CSI sequences)
                    '\x1b' => skip = 2,
                    _ => {}
                }
            } else if msg.push(c).is_err() {
                self.write_message(b"\r\nError: input too long\r\n> ").await;
                msg.clear();
            } else {
                // Echo the character back.
                let mut buf = [0u8; 4];
                self.write_message(c.encode_utf8(&mut buf).as_bytes()).await;
            }
        }
        None
    }
}
