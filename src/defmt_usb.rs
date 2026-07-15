//! Defmt global logger that simultaneously writes to RTT and USB CDC ACM.
//!
//! This module inlines two separate implementations:
//!  - The RTT transport from `defmt-rtt 1.1.0` (SEGGER RTT control block +
//!    ring-buffer write logic)
//!  - The USB double-buffer transport from `defmt-embassy-usbserial`
//!    (ported to embassy-usb 0.6)
//!
//! Both transports are driven by a single `#[defmt::global_logger]`.  Because
//! defmt allows only **one** global logger, this replaces `defmt-rtt` as a
//! direct dependency.
//!
//! # Usage
//!
//! Remove `use {defmt_rtt as _, …}` from `main.rs` and add `mod defmt_usb;`.
//! Then in `usb_task` split the logger CDC ACM class and spawn
//! [`usb_defmt_task`].

#![allow(clippy::missing_safety_doc)]

use core::{
    cell::UnsafeCell,
    ptr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use embassy_rp::{peripherals::USB, usb::Driver as UsbDriver};
use embassy_time::{Duration, Timer};
use embassy_usb::{Builder, Handler, types::StringIndex};
use embassy_usb::driver::{Driver as UsbDriverTrait, Endpoint as UsbEndpoint, EndpointIn as UsbEndpointIn, EndpointError};
use portable_atomic::AtomicBool as PortableAtomicBool;
use static_cell::StaticCell;

// ─────────────────────────────────────────────────────────────────────────────
// RTT infrastructure  (ported verbatim from defmt-rtt 1.1.0)
// ─────────────────────────────────────────────────────────────────────────────

const RTT_BUF_SIZE: usize = 1024;

/// Relevant bits in the RTT channel `flags` field.
const MODE_MASK: usize = 0b11;
/// Block application until host reads data.
const MODE_BLOCK_IF_FULL: usize = 2;
/// Skip data if buffer is full — the default on start-up.
const MODE_NON_BLOCKING_TRIM: usize = 1;

#[repr(C)]
struct RttHeader {
    id: [u8; 16],
    max_up_channels: usize,
    max_down_channels: usize,
    up_channel: RttChannel,
}
// SAFETY: only written within a critical section held by the defmt global logger.
unsafe impl Sync for RttHeader {}

#[repr(C)]
struct RttChannel {
    name: *const u8,
    buffer: *mut u8,
    size: usize,
    /// Write cursor (written by target).
    write: AtomicUsize,
    /// Read cursor (written by host).
    read: AtomicUsize,
    /// Channel properties / mode flags.
    flags: AtomicUsize,
}
// SAFETY: access is serialised by the global-logger critical section.
unsafe impl Sync for RttChannel {}

impl RttChannel {
    fn write_all(&self, mut bytes: &[u8]) {
        // Probe-rs sets blocking mode; non-probe builds use non-blocking trim.
        let write_fn: fn(&Self, &[u8]) -> usize = if self.host_is_connected() {
            Self::blocking_write
        } else {
            Self::nonblocking_write
        };
        while !bytes.is_empty() {
            let consumed = write_fn(self, bytes);
            if consumed != 0 {
                bytes = &bytes[consumed..];
            }
        }
    }

    fn blocking_write(&self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        let available = rtt_available(read, write, RTT_BUF_SIZE);
        if available == 0 {
            return 0;
        }
        self.write_impl(bytes, write, available)
    }

    fn nonblocking_write(&self, bytes: &[u8]) -> usize {
        let write = self.write.load(Ordering::Acquire);
        self.write_impl(bytes, write, RTT_BUF_SIZE)
    }

    fn write_impl(&self, bytes: &[u8], cursor: usize, available: usize) -> usize {
        let len = bytes.len().min(available);
        // SAFETY: `self.buffer` points to a static array of `RTT_BUF_SIZE` bytes.
        unsafe {
            if cursor + len > RTT_BUF_SIZE {
                let pivot = RTT_BUF_SIZE - cursor;
                ptr::copy_nonoverlapping(bytes.as_ptr(), self.buffer.add(cursor), pivot);
                ptr::copy_nonoverlapping(bytes.as_ptr().add(pivot), self.buffer, len - pivot);
            } else {
                ptr::copy_nonoverlapping(bytes.as_ptr(), self.buffer.add(cursor), len);
            }
        }
        self.write
            .store(cursor.wrapping_add(len) % RTT_BUF_SIZE, Ordering::Release);
        len
    }

    fn flush(&self) {
        if !self.host_is_connected() {
            return;
        }
        while self.read.load(Ordering::Relaxed) != self.write.load(Ordering::Relaxed) {
            core::hint::spin_loop();
        }
    }

    fn host_is_connected(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & MODE_MASK == MODE_BLOCK_IF_FULL
    }
}

fn rtt_available(read: usize, write: usize, size: usize) -> usize {
    if read > write {
        read - write - 1
    } else if read == 0 {
        size - write - 1
    } else {
        size - write
    }
}

struct RttBuffer {
    inner: UnsafeCell<[u8; RTT_BUF_SIZE]>,
}
impl RttBuffer {
    const fn new() -> Self {
        Self {
            inner: UnsafeCell::new([0; RTT_BUF_SIZE]),
        }
    }
    const fn get(&self) -> *mut u8 {
        self.inner.get() as *mut u8
    }
}
// SAFETY: the buffer is only accessed through the serialised global logger.
unsafe impl Sync for RttBuffer {}

#[unsafe(link_section = ".uninit.defmt-rtt.BUFFER")]
static RTT_BUFFER: RttBuffer = RttBuffer::new();

#[unsafe(link_section = ".data.defmt-rtt.NAME")]
static RTT_NAME: [u8; 6] = *b"defmt\0";

/// The SEGGER RTT control block — must be named `_SEGGER_RTT` and exported
/// without mangling so that probe-rs / OpenOCD can locate it in RAM.
#[unsafe(no_mangle)]
static _SEGGER_RTT: RttHeader = RttHeader {
    id: *b"SEGGER RTT\0\0\0\0\0\0",
    max_up_channels: 1,
    max_down_channels: 0,
    up_channel: RttChannel {
        name: RTT_NAME.as_ptr(),
        buffer: RTT_BUFFER.get(),
        size: RTT_BUF_SIZE,
        write: AtomicUsize::new(0),
        read: AtomicUsize::new(0),
        flags: AtomicUsize::new(MODE_NON_BLOCKING_TRIM),
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// USB double-buffer  (ported from defmt-embassy-usbserial, embassy-usb 0.6)
// ─────────────────────────────────────────────────────────────────────────────

const USB_BUF_SIZE: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BufState {
    Active,
    Flush,
}

struct LogBuffer {
    state: BufState,
    cursor: usize,
    data: [u8; USB_BUF_SIZE],
}

impl LogBuffer {
    const fn new() -> Self {
        Self {
            state: BufState::Active,
            cursor: 0,
            data: [0u8; USB_BUF_SIZE],
        }
    }

    /// Mark buffer as ready to flush.
    fn set_flushing(&mut self) {
        self.state = BufState::Flush;
    }

    /// Reset to empty/active state.
    fn reset(&mut self) {
        self.state = BufState::Active;
        self.cursor = 0;
    }

    /// Write bytes into the buffer.
    ///
    /// # Precondition
    ///
    /// Caller must have verified `accepts(bytes.len())` first.
    fn write(&mut self, bytes: &[u8]) {
        let cursor = self.cursor;
        self.data[cursor..cursor + bytes.len()].copy_from_slice(bytes);
        self.cursor += bytes.len();
    }

    fn accepts(&self, n: usize) -> bool {
        (self.cursor + n) < USB_BUF_SIZE && self.state == BufState::Active
    }

    fn is_flushing(&self) -> bool {
        self.state == BufState::Flush
    }
}

struct Controller {
    /// Index of the active (writable) buffer: 0 or 1.
    current_idx: AtomicUsize,
    /// Whether logging is enabled (disabled while USB is disconnected).
    enabled: PortableAtomicBool,
    /// Double-buffer pair.
    ///
    /// SAFETY: writes happen only within a critical section; reads only after
    /// the buffer has been marked `Flush` (and are therefore disjoint from
    /// writes).
    buffers: [UnsafeCell<LogBuffer>; 2],
}

// SAFETY: see field comments above.
unsafe impl Sync for Controller {}

impl Controller {
    const fn new() -> Self {
        Self {
            current_idx: AtomicUsize::new(0),
            enabled: PortableAtomicBool::new(true),
            buffers: [
                UnsafeCell::new(LogBuffer::new()),
                UnsafeCell::new(LogBuffer::new()),
            ],
        }
    }

    fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    fn disable(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        let (a, b) = (self.buffers[0].get(), self.buffers[1].get());
        critical_section::with(|_| {
            // SAFETY: inside a critical section.
            unsafe {
                (*a).reset();
                (*b).reset();
            }
        });
    }

    /// Mark the current buffer as flushing and flip to the other one.
    ///
    /// # Safety
    ///
    /// Must be called from within a critical section.
    unsafe fn swap(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let idx = self.current_idx.load(Ordering::Relaxed);
        // SAFETY: inside a critical section, no concurrent mutation.
        unsafe {
            (*self.buffers[idx].get()).set_flushing();
        }
        self.current_idx.store(idx ^ 1, Ordering::Relaxed);
    }

    /// Append encoded bytes to the active buffer, swapping if necessary.
    ///
    /// # Safety
    ///
    /// Must be called from within a critical section.
    unsafe fn write(&self, bytes: &[u8]) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let idx = self.current_idx.load(Ordering::Relaxed);
        let other = idx ^ 1;
        // SAFETY: inside critical section.
        let cur = unsafe { &mut *self.buffers[idx].get() };
        let oth = unsafe { &mut *self.buffers[other].get() };

        if cur.accepts(bytes.len()) {
            cur.write(bytes);
        } else {
            // Active buffer is full mid-frame (frame exceeds USB_BUF_SIZE).
            // Swap and fall back to the other buffer; bytes are silently
            // dropped if both buffers are full.
            unsafe { self.swap() };
            if oth.accepts(bytes.len()) {
                oth.write(bytes);
            }
        }
    }

    fn get_flushing(&self) -> Option<(usize, &LogBuffer)> {
        for (i, cell) in self.buffers.iter().enumerate() {
            // SAFETY: the task loop is the only reader, and it only reads
            // buffers in Flush state, which are not written to concurrently.
            let buf = unsafe { &*cell.get() };
            if buf.is_flushing() {
                return Some((i, buf));
            }
        }
        None
    }

    fn reset_buffer(&self, idx: usize) {
        critical_section::with(|_| {
            // SAFETY: inside a critical section.
            unsafe { (*self.buffers[idx].get()).reset() };
        });
    }
}

static CONTROLLER: Controller = Controller::new();

// ─────────────────────────────────────────────────────────────────────────────
// Combined defmt global logger
// ─────────────────────────────────────────────────────────────────────────────

struct MultiEncoder {
    /// Re-entrancy guard / exclusive-access flag.
    taken: AtomicBool,
    /// Saved interrupt-enable state for the critical section.
    restore: UnsafeCell<critical_section::RestoreState>,
    /// The defmt COBS encoder that frames outgoing bytes.
    encoder: UnsafeCell<defmt::Encoder>,
}

// SAFETY: access is serialised by the `taken` flag and the critical section.
unsafe impl Sync for MultiEncoder {}

impl MultiEncoder {
    const fn new() -> Self {
        Self {
            taken: AtomicBool::new(false),
            restore: UnsafeCell::new(critical_section::RestoreState::invalid()),
            encoder: UnsafeCell::new(defmt::Encoder::new()),
        }
    }

    fn acquire(&self) {
        // Enter a critical section so only one caller can proceed.
        // SAFETY: paired with the `release` call below.
        let restore = unsafe { critical_section::acquire() };

        if self.taken.load(Ordering::Relaxed) {
            panic!("defmt logger taken reentrantly");
        }
        self.taken.store(true, Ordering::Relaxed);

        // SAFETY: we are in a critical section.
        unsafe {
            self.restore.get().write(restore);
            (*self.encoder.get()).start_frame(Self::sink);
        }
    }

    /// # Safety
    ///
    /// Must only be called after `acquire` and before `release`.
    unsafe fn write(&self, bytes: &[u8]) {
        // SAFETY: inside a critical section held since `acquire`.
        unsafe {
            (*self.encoder.get()).write(bytes, Self::sink);
        }
    }

    /// # Safety
    ///
    /// Must only be called after `acquire` and before `release`.
    unsafe fn flush(&self) {
        // SAFETY: inside a critical section.
        unsafe {
            CONTROLLER.swap();
        }
        _SEGGER_RTT.up_channel.flush();
    }

    /// # Safety
    ///
    /// Must only be called once after `acquire`.
    unsafe fn release(&self) {
        if !self.taken.load(Ordering::Relaxed) {
            panic!("defmt release out of context");
        }
        // SAFETY: inside a critical section.
        unsafe {
            (*self.encoder.get()).end_frame(Self::sink);
            let restore = self.restore.get().read();
            self.taken.store(false, Ordering::Relaxed);
            critical_section::release(restore);
        }
    }

    /// Byte sink: writes encoded defmt bytes to **both** RTT and the USB
    /// double-buffer.
    ///
    /// Called by the `defmt::Encoder` inside `start_frame`, `write`, and
    /// `end_frame` — all of which occur within the critical section held by
    /// `acquire`/`release`.
    fn sink(bytes: &[u8]) {
        _SEGGER_RTT.up_channel.write_all(bytes);
        // SAFETY: called within the critical section held since `acquire`.
        unsafe {
            CONTROLLER.write(bytes);
        }
    }
}

static LOGGER: MultiEncoder = MultiEncoder::new();

#[defmt::global_logger]
struct MultiLogger;

unsafe impl defmt::Logger for MultiLogger {
    fn acquire() {
        LOGGER.acquire();
    }

    unsafe fn write(bytes: &[u8]) {
        // SAFETY: contract delegated to `MultiEncoder::write`.
        unsafe { LOGGER.write(bytes) }
    }

    unsafe fn flush() {
        // SAFETY: contract delegated to `MultiEncoder::flush`.
        unsafe { LOGGER.flush() }
    }

    unsafe fn release() {
        // SAFETY: contract delegated to `MultiEncoder::release`.
        unsafe { LOGGER.release() }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// USB logger task
// ─────────────────────────────────────────────────────────────────────────────

/// Concrete sender type for the defmt CDC ACM interface.
pub type DefmtSender = <UsbDriver<'static, USB> as UsbDriverTrait<'static>>::EndpointIn;

// ── Private handler ──────────────────────────────────────────────────────────

struct DefmtStringHandler {
    defmt_str: StringIndex,
}

impl Handler for DefmtStringHandler {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        (index == self.defmt_str).then_some("defmt")
    }
}

/// Builder for the defmt USB logger task.
///
/// Call [`build`](Self::build) to register the CDC-ACM interface (with the
/// `"defmt"` iInterface string) on the USB [`Builder`], then call
/// [`UsbDefmtTask::spawn`] on the returned value after [`Builder::build`] to
/// start the drain task.
///
/// Buffers are flushed only at defmt frame boundaries (end-of-frame), never
/// mid-frame, to preserve COBS stream integrity.
///
/// # Example
///
/// ```no_run
/// let task = UsbDefmtLogger::new()
///     .with_timeout(Duration::from_millis(5))
///     .build(&mut builder);          // registers interface + string handler
/// let usb = builder.build();
/// task.spawn(&spawner);              // starts the drain task
/// ```
pub struct UsbDefmtLogger {
    /// How long to wait between drain attempts when the buffer is empty.
    timeout: Duration,
}

impl Default for UsbDefmtLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbDefmtLogger {
    /// Create a logger with default settings:
    /// - `timeout` = 10 ms
    pub const fn new() -> Self {
        Self {
            timeout: Duration::from_millis(10),
        }
    }

    /// Set the idle poll interval — how long `usb_defmt_task` waits between
    /// drain attempts when no buffer is ready to flush.
    #[allow(dead_code)]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Register the defmt CDC-ACM interface on `builder` and return a
    /// [`UsbDefmtTask`] holding the allocated bulk-IN endpoint.
    /// Must be called before `builder.build()`.
    ///
    /// Builds a CDC function with two interfaces:
    /// - Comm interface (class 0x02/0x02/0x01) with iInterface = `"defmt"`
    /// - Data interface (class 0x0A/0x00/0x00) with bulk IN/OUT endpoints
    pub fn build(self, builder: &mut Builder<'static, UsbDriver<'static, USB>>) -> UsbDefmtTask {
        static DEFMT_STRING_HANDLER: StaticCell<DefmtStringHandler> = StaticCell::new();

        let (ep_in, defmt_str) = {
            let mut func = builder.function(0x02, 0x02, 0x00);

            // Comm interface: carries the "defmt" iInterface string.
            let defmt_str = {
                let mut comm = func.interface();
                let str_idx = comm.string();
                let num = comm.interface_number();
                let mut alt = comm.alt_setting(0x02, 0x02, 0x01, Some(str_idx));
                // CDC Header functional descriptor (CDC spec v1.10).
                alt.descriptor(0x24, &[0x00, 0x10, 0x01]);
                // CDC ACM functional descriptor (capabilities 0x06).
                alt.descriptor(0x24, &[0x02, 0x06]);
                // CDC Union functional descriptor (comm = this, data = comm + 1).
                alt.descriptor(0x24, &[0x06, num.0, num.0 + 1]);
                // Notification endpoint (required by spec; not used by defmt).
                alt.endpoint_interrupt_in(None, 8, 255);
                str_idx
            };

            // Data interface: the bulk IN endpoint is the defmt byte stream.
            let ep_in = {
                let mut data = func.interface();
                let mut alt = data.alt_setting(0x0A, 0x00, 0x00, None);
                let _ = alt.endpoint_bulk_out(None, 64);
                alt.endpoint_bulk_in(None, 64)
            };

            (ep_in, defmt_str)
        };

        let handler = DEFMT_STRING_HANDLER.init(DefmtStringHandler { defmt_str });
        builder.handler(handler);

        UsbDefmtTask { timeout: self.timeout, sender: ep_in }
    }
}

/// A configured defmt USB logger ready to be spawned.
///
/// Obtained from [`UsbDefmtLogger::build`].  Call [`spawn`](Self::spawn) after
/// [`Builder::build`] to start the drain task.
pub struct UsbDefmtTask {
    timeout: Duration,
    sender: DefmtSender,
}

impl UsbDefmtTask {
    /// Spawn [`usb_defmt_task`].
    ///
    /// Panics if the task has already been spawned (embassy task singleton
    /// contract).
    pub fn spawn(self, spawner: &embassy_executor::Spawner) {
        spawner.spawn(usb_defmt_task(self.sender, self.timeout)).unwrap();
    }
}

/// Embassy task that drains the USB double-buffer out over the CDC ACM sender.
///
/// Spawn this alongside the USB device task.  When the host disconnects the
/// controller is disabled (discarding defmt frames); it is re-enabled on
/// reconnect.
#[embassy_executor::task]
async fn usb_defmt_task(mut sender: DefmtSender, timeout: Duration) {
    'main: loop {
        // Wait for the endpoint to be enabled (USB enumeration complete).
        sender.wait_enabled().await;
        CONTROLLER.enable();

        loop {
            // Drain one flushing buffer per iteration (if any).
            if let Some((idx, buf)) = CONTROLLER.get_flushing() {
                let bytes = &buf.data[..buf.cursor];
                let max = sender.info().max_packet_size as usize;
                let mut last_was_max = false;

                let mut write_err: Option<EndpointError> = None;
                for chunk in bytes.chunks(max) {
                    last_was_max = chunk.len() == max;
                    match sender.write(chunk).await {
                        Ok(()) => {}
                        Err(e) => {
                            write_err = Some(e);
                            break;
                        }
                    }
                }

                // Per USB CDC spec: if the last transfer was exactly max-packet-
                // size, send a zero-length packet to signal end-of-transfer.
                if write_err.is_none() && last_was_max {
                    write_err = sender.write(&[]).await.err();
                }

                // Always reset the buffer whether or not an error occurred.
                CONTROLLER.reset_buffer(idx);

                match write_err {
                    Some(EndpointError::Disabled) => {
                        CONTROLLER.disable();
                        continue 'main;
                    }
                    Some(EndpointError::BufferOverflow) => {
                        unreachable!("chunks are bounded by sender max_packet_size")
                    }
                    None => {}
                }
            }

            Timer::after(timeout).await;
        }
    }
}
