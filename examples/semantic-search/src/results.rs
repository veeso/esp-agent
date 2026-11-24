use core::fmt::Arguments;

use esp_agent::TextHit;

/// Sends each ranked hit to the console as one formatted line.
pub(crate) fn render_hits(hits: &[TextHit<'_, ()>], mut print_line: impl FnMut(Arguments<'_>)) {
    for (rank, hit) in (1..).zip(hits) {
        let score = hit.score;
        let text = hit.text;
        print_line(format_args!("  {rank}. [{score:.3}] {text}"));
    }
}
