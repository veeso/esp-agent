//! Chip and serial setup shared by the firmware binaries.

use esp_hal::Blocking;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::uart::{Config as UartConfig, Uart};
use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
use esp_println::{Printer, print, println};

use crate::console::{Event, LineEditor, SubmittedLine};

/// Text printed before the user types a line.
pub const PROMPT: &str = "> ";

/// Pause between polls of the serial ports when no byte is waiting.
const POLL_INTERVAL_US: u32 = 500;

/// Both serial inputs the console listens on.
///
/// `esp-println` writes to the USB Serial/JTAG port when a USB host is attached and to UART0
/// otherwise, so input is accepted from both and replies go out through whichever is active.
pub struct SerialInput<'d> {
    usb: UsbSerialJtag<'d, Blocking>,
    uart: Uart<'d, Blocking>,
    delay: Delay,
}

/// Initializes the logger, the chip at its maximum clock, the heap, and both serial inputs.
///
/// Call it once, first thing in the entry point of a binary: it takes the peripherals and
/// registers the global heap.
pub fn init_serial() -> SerialInput<'static> {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    #[cfg(feature = "esp32s3")]
    {
        // The following pins are used to bootstrap the chip. They are available
        // for use, but check the datasheet of the module for more information on them.
        // - GPIO0
        // - GPIO3
        // - GPIO45
        // - GPIO46
        // These GPIO pins are in use by some feature of the module and should not be used.
        let _gpio33 = peripherals.GPIO33;
        let _gpio34 = peripherals.GPIO34;
        let _gpio35 = peripherals.GPIO35;
        let _gpio36 = peripherals.GPIO36;
        let _gpio37 = peripherals.GPIO37;
    }

    #[cfg(feature = "esp32s3")]
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
    #[cfg(feature = "esp32s31")]
    esp_alloc::heap_allocator!(size: 73744);

    // Only the receive side of UART0 is claimed: transmission stays with `esp-println`.
    #[cfg(feature = "esp32s3")]
    let uart_rx = peripherals.GPIO44;
    #[cfg(feature = "esp32s31")]
    let uart_rx = peripherals.GPIO59;
    let uart = Uart::new(peripherals.UART0, UartConfig::default())
        .expect("the default UART configuration is valid")
        .with_rx(uart_rx);
    SerialInput {
        usb: UsbSerialJtag::new(peripherals.USB_DEVICE),
        uart,
        delay: Delay::new(),
    }
}

/// Blocks until a full line has been typed, echoing the keystrokes back to the terminal.
pub fn read_line(input: &mut SerialInput<'_>, editor: &mut LineEditor) -> SubmittedLine {
    loop {
        let Some(byte) = read_byte(input) else {
            input.delay.delay_micros(POLL_INTERVAL_US);
            continue;
        };
        match editor.push(byte) {
            Event::Ignored => {}
            Event::Echo(byte) => Printer::write_bytes(&[byte]),
            Event::Erase => Printer::write_bytes(b"\x08 \x08"),
            Event::Kill => print!("\r\x1b[K{PROMPT}"),
            Event::Submit => {
                println!();
                return editor.take_line();
            }
        }
    }
}

/// Returns the next received byte from either serial port, if one is waiting.
fn read_byte(input: &mut SerialInput<'_>) -> Option<u8> {
    if let Ok(byte) = input.usb.read_byte() {
        return Some(byte);
    }
    let mut byte = [0u8; 1];
    if input.uart.read_ready() && matches!(input.uart.read(&mut byte), Ok(1)) {
        return Some(byte[0]);
    }
    None
}
