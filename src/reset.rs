use embassy_rp::rom_data::reset_to_usb_boot;
use embassy_usb::{
    Builder, Handler,
    control::{OutResponse, Recipient, Request, RequestType},
    driver::Driver,
    types::{InterfaceNumber, StringIndex},
};

pub struct KiwiUsbReset {
    iface: InterfaceNumber,
    desc: StringIndex,
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
        let desc = iface.string();
        iface.alt_setting(0xFF, 0x00, 0x01, Some(desc)); // no endpoints
        Self {
            iface: iface_num,
            desc,
        }
    }
}

impl Handler for KiwiUsbReset {
    fn get_string(
        &mut self,
        index: embassy_usb::types::StringIndex,
        _lang_id: u16,
    ) -> Option<&str> {
        (index == self.desc).then_some("rp2xxx-reset")
    }

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
