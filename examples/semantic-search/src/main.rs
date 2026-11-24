#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use alloc::vec::Vec;

use esp_agent::{Agent, Embedder, Hit, Knowledge, Model, Scratch};
use esp_backtrace as _;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_println::{print, println};
use log::info;

use crate::board::PROMPT;
use crate::console::LineEditor;
use crate::corpus::{CORPUS, TOP_K};

extern crate alloc;

mod board;
mod console;
mod corpus;
mod results;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// The potion-2M static embedding model, embedded in the firmware image and read in place
/// from flash. The path is relative to this crate.
static POTION: Model = esp_agent::model!("model");

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 1.4.0
    // generator parameters: -o esp32s3 -o esp32s3-wroom-2 -o alloc -o log -o esp-backtrace -o ci -o agents -o zed -o esp

    let mut input = board::init_serial();

    // Building the embedder copies nothing: the model stays in flash.
    let embedder = Embedder::new(POTION);
    info!(
        "potion ready: {vocab_size} tokens x {dim} dims, table {kib} KiB in flash",
        vocab_size = POTION.vocab_size(),
        dim = POTION.dim(),
        kib = POTION.vocab_size() * POTION.dim() * size_of::<f32>() / 1024
    );

    // ANCHOR: search_setup
    let mut knowledge = Knowledge::with_capacity(embedder, CORPUS.len());
    knowledge.extend(CORPUS.map(|text| (text, ())));
    let agent = Agent::new(POTION, &knowledge).expect("the corpus is embedded with POTION");
    // ANCHOR_END: search_setup
    info!("indexed {count} sentences", count = CORPUS.len());

    println!();
    println!(
        "Type a question or a sentence and press Enter to find the closest indexed sentences."
    );
    println!("Indexed sentences:");
    CORPUS.iter().for_each(|text| println!("  - {text}"));
    println!();

    let mut editor = LineEditor::new();
    // Reused by every query, so searching does not touch the heap once they are warm.
    let mut scratch = Scratch::new();
    let mut hits = Vec::with_capacity(TOP_K);
    loop {
        print!("{PROMPT}");
        let line = board::read_line(&mut input, &mut editor);
        let query = line.text().trim();
        if query.is_empty() {
            continue;
        }
        answer(&agent, query, &mut scratch, &mut hits);
    }
}

// ANCHOR: search_answer
/// Prompts `agent` with `query` and prints the [`TOP_K`] closest indexed sentences.
fn answer<'a>(
    agent: &Agent<'a, ()>,
    query: &str,
    scratch: &mut Scratch,
    hits: &mut Vec<Hit<'a, ()>>,
) {
    let start = Instant::now();
    agent.prompt_with(query, TOP_K, scratch, hits);
    let total_us = start.elapsed().as_micros();

    println!("{total_us} us");
    results::render_hits(hits, &CORPUS, |line| println!("{line}"));
    println!();
}
// ANCHOR_END: search_answer
