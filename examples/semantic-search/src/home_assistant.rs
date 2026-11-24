#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_agent::{Agent, Embedder, Knowledge, Model};
use esp_backtrace as _;
use esp_hal::main;
use esp_println::{print, println};
use log::info;

use crate::board::PROMPT;
use crate::console::LineEditor;
use crate::home::{EXAMPLES, Home};

extern crate alloc;

mod board;
mod console;
mod home;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// The potion-2M static embedding model, embedded in the firmware image and read in place
/// from flash. The path is relative to this crate.
static POTION: Model = esp_agent::model!("model");

/// How many lamps the agent remembers or offers in one question.
const CONTEXT_TARGETS: usize = 2;

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 1.4.0
    // generator parameters: -o esp32s3 -o esp32s3-wroom-2 -o alloc -o log -o esp-backtrace -o ci -o agents -o zed -o esp

    let mut input = board::init_serial();

    // ANCHOR: agent_setup
    let mut knowledge = Knowledge::with_capacity(Embedder::new(POTION), EXAMPLES.len());
    knowledge.extend(EXAMPLES);
    let mut agent = Agent::builder(POTION, &knowledge)
        .context(CONTEXT_TARGETS)
        .build()
        .expect("the examples are embedded with POTION");
    // ANCHOR_END: agent_setup
    info!("registered {count} examples", count = EXAMPLES.len());

    println!();
    println!("Control two simulated lamps: the desk lamp and the floor lamp.");
    println!("Try:");
    println!("  Turn on the lamp");
    println!("  The desk one");
    println!("  Dim it to 30 percent");
    println!("  Turn on the living room lights");
    println!();

    let mut home = Home::new();
    let mut editor = LineEditor::new();
    loop {
        print!("{PROMPT}");
        let line = board::read_line(&mut input, &mut editor);
        if let Some(reply) = home.respond(&mut agent, &line) {
            println!("{reply}");
        }
    }
}
