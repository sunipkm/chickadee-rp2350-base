use embassy_rp::rom_data::reset_to_usb_boot;
use embassy_usb::Builder;
use embassy_usb::Handler;
use embassy_usb::control::{OutResponse, Recipient, Request, RequestType};
use embassy_usb::driver::Driver;
use embassy_usb::types::InterfaceNumber;

pub struct KiwiUsbReset {
    iface: InterfaceNumber,
}

impl<T: Driver<'static>> From<&mut Builder<'static, T>> for KiwiUsbReset {
    fn from(builder: &mut Builder<'static, T>) -> Self {
        Self::new(builder)
    }
}

impl KiwiUsbReset {
    fn new(builder: &mut Builder<'static, impl Driver<'static>>) -> Self {
        let mut func = builder.function(0xFF, 0x00, 0x01); // class/subclass/protocol
        let mut iface = func.interface();
        let iface_num = iface.interface_number();
        iface.alt_setting(0xFF, 0x00, 0x01, None); // no endpoints
        Self { iface: iface_num }
    }
}

impl Handler for KiwiUsbReset {
    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.index == u8::from(self.iface) as u16
            && req.request == 0x01
        {
            // RP2350: reboot into BOOTSEL via ROM
            reset_to_usb_boot(0, 0);
            Some(OutResponse::Accepted)
        } else {
            None
        }
    }
}
