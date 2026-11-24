use core::fmt::Arguments;

use esp_agent::Hit;

/// Sends each ranked hit to the console as one formatted line, with the text it points at
/// in `texts`.
pub(crate) fn render_hits(
    hits: &[Hit<'_, ()>],
    texts: &[&str],
    mut print_line: impl FnMut(Arguments<'_>),
) {
    for (rank, hit) in (1..).zip(hits) {
        let score = hit.score;
        let text = texts[hit.index];
        print_line(format_args!("  {rank}. [{score:.3}] {text}"));
    }
}
