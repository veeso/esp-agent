#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use alloc::string::String;
use alloc::vec::Vec;
use esp_backtrace as _;
use esp_hal::Blocking;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_hal::uart::{Config as UartConfig, Uart};
use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
use esp_println::{Printer, print, println};
use log::info;
use static_models_bm::console::{Event, LineEditor};
use static_models_bm::{DIM, StaticEmbedder, VOCAB_SIZE, cosine_similarity};

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const PROMPT: &str = "> ";

/// Number of closest sentences printed for each query.
const TOP_K: usize = 3;

/// Sentences the console searches. Each one is embedded once at startup.
const CORPUS: [&str; 8] = [
    "The quick brown fox jumps over the lazy dog.",
    "A fast auburn fox leaps above a sleepy hound.",
    "Quarterly revenue grew 12% year over year.",
    "Qdrant is a vector search engine written in Rust.",
    "The ESP32-S3 is a dual-core microcontroller with Wi-Fi and Bluetooth.",
    "Paris is the capital of France and sits on the river Seine.",
    "Water boils at 100 degrees Celsius at sea level.",
    "Rust guarantees memory safety without a garbage collector.",
];

/// Pause between polls of the serial ports when no byte is waiting.
const POLL_INTERVAL_US: u32 = 500;

/// Both serial inputs the console listens on.
///
/// `esp-println` writes to the USB Serial/JTAG port when a USB host is attached and to UART0
/// otherwise, so input is accepted from both and replies go out through whichever is active.
struct SerialInput<'d> {
    usb: UsbSerialJtag<'d, Blocking>,
    uart: Uart<'d, Blocking>,
    delay: Delay,
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 1.4.0
    // generator parameters: -o esp32s3 -o esp32s3-wroom-2 -o alloc -o log -o esp-backtrace -o ci -o agents -o zed -o esp

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

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

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    // Only the receive side of UART0 is claimed: transmission stays with `esp-println`.
    let uart = Uart::new(peripherals.UART0, UartConfig::default())
        .expect("the default UART configuration is valid")
        .with_rx(peripherals.GPIO44);
    let mut input = SerialInput {
        usb: UsbSerialJtag::new(peripherals.USB_DEVICE),
        uart,
        delay: Delay::new(),
    };

    // The model and the vocabulary are embedded in the firmware image and read in place
    // from flash: building the embedder copies nothing.
    let embedder = static_models_bm::embedder();
    info!(
        "potion ready: {VOCAB_SIZE} tokens x {DIM} dims, table {table_kib} KiB in flash",
        table_kib = static_models_bm::embedding_table().len() / 1024
    );

    let corpus_vectors: Vec<Vec<f32>> = CORPUS.iter().map(|text| embedder.embed(text)).collect();
    info!("indexed {count} sentences", count = CORPUS.len());

    println!();
    println!(
        "Type a question or a sentence and press Enter to find the closest indexed sentences."
    );
    println!("Indexed sentences:");
    CORPUS.iter().for_each(|text| println!("  - {text}"));
    println!();

    let mut editor = LineEditor::new();
    loop {
        print!("{PROMPT}");
        let line = read_line(&mut input, &mut editor);
        let query = line.trim();
        if query.is_empty() {
            continue;
        }
        answer(&embedder, &corpus_vectors, query);
    }
}

/// Blocks until a full line has been typed, echoing the keystrokes back to the terminal.
fn read_line(input: &mut SerialInput<'_>, editor: &mut LineEditor) -> String {
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

/// Embeds `query` and prints the [`TOP_K`] closest indexed sentences.
fn answer(embedder: &StaticEmbedder, corpus_vectors: &[Vec<f32>], query: &str) {
    let start = Instant::now();
    let vector = embedder.embed(query);
    let embed_us = start.elapsed().as_micros();

    let mut ranked: Vec<(usize, f32)> = corpus_vectors
        .iter()
        .map(|candidate| cosine_similarity(&vector, candidate))
        .enumerate()
        .collect();
    ranked.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    let total_us = start.elapsed().as_micros();

    println!("{total_us} us total ({embed_us} us to embed the query)");
    ranked
        .iter()
        .take(TOP_K)
        .enumerate()
        .for_each(|(rank, &(index, score))| {
            let rank = rank + 1;
            let text = CORPUS[index];
            println!("  {rank}. [{score:.3}] {text}");
        });
    println!();
}
