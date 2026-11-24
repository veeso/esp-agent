/// Number of closest sentences printed for each query.
pub(crate) const TOP_K: usize = 3;

/// Sentences the console searches. Each one is embedded once at startup.
pub(crate) const CORPUS: [&str; 8] = [
    "The quick brown fox jumps over the lazy dog.",
    "A fast auburn fox leaps above a sleepy hound.",
    "Quarterly revenue grew 12% year over year.",
    "Qdrant is a vector search engine written in Rust.",
    "The ESP32-S3 is a dual-core microcontroller with Wi-Fi and Bluetooth.",
    "Paris is the capital of France and sits on the river Seine.",
    "Water boils at 100 degrees Celsius at sea level.",
    "Rust guarantees memory safety without a garbage collector.",
];
