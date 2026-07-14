use assign_resources::assign_resources;
use embassy_rp::usb::InterruptHandler as UsbIrqHandler;
use embassy_rp::{Peri, bind_interrupts, peripherals, peripherals::USB};

assign_resources! {
    /// USB peripheral resources.
    usbdev: UsbDev {
        usb: USB,
    },
    leddev: LedDev {
        pin: PIN_25
    }
}

bind_interrupts!(
    /// Interrupt handler bindings for this firmware.
    pub struct Irqs {
        USBCTRL_IRQ => UsbIrqHandler<USB>;
    }
);
