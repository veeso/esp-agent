# Search

Semantic search finds the stored texts closest in meaning to a query. The
firmware embeds the texts, embeds the query, and ranks the results on the chip.

## What it is good for

- Look up an answer in an offline FAQ or manual.
- Match free text to a fixed set of intents or commands.
- Route a request to a handler based on the best match.
- Find near-duplicate log or sensor messages.

Each search scans the whole index, so search cost grows with the number of
entries. Start with a corpus on the order of hundreds of entries and measure
latency and heap use on your board. The static models mean-pool token vectors,
so word order is lost. The [models in this guide](models.md) are for English
text.

## Two levels

`Index::search(&query_vector, k)` ranks vectors you supply. Each `Hit` gives
its `score`, insertion `index`, and `payload`. When you already have a
`Knowledge` store, you can access its index and look up the original text by
index.

```rust
let query = knowledge.embedder().embed("Which city is France's capital?");
for hit in knowledge.index().search(&query, 3) {
    let text = knowledge.text(hit.index);
    // hit.score, hit.payload, text
}
```

`Agent::prompt(text, k)` does that lookup for you. It embeds the query using the
store's embedder and returns `TextHit` values with the `score`, `index`, `text`,
and `payload`. See the [quick start](index.md#quick-start) for an example.

## Ranking rules

The index ranks entries by descending cosine similarity and keeps insertion
order when scores tie. A `k` larger than the store returns every entry. A `k`
of zero returns no hits. A prompt with no known words scores every entry `0.0`,
so the first entries inserted appear first.

## Search without allocating

Keep a `Scratch` for the embedding buffers and a `Vec::with_capacity(k)` for
the hits. `Agent::prompt_with` fills both, and `Index::search_into` fills the
hits vector when you already have a query vector. Calls reuse the buffers and
allocate only if they need to grow. Use `agent.knowledge().text(hit.index)` to
borrow the original text for a hit from `prompt_with`.

```rust
extern crate alloc;

use alloc::vec::Vec;
use esp_agent::Scratch;

let mut scratch = Scratch::new();
let mut hits = Vec::with_capacity(3);
agent.prompt_with("Which city is France's capital?", 3, &mut scratch, &mut hits);
```

## Example with the semantic search console

The example embeds Potion 2M and searches this corpus.

```rust
{{#include ../../examples/semantic-search/src/corpus.rs}}
```

At startup it embeds each sentence once and builds the agent.

```rust
{{#include ../../examples/semantic-search/src/main.rs:search_setup}}
```

For each query, it reuses its buffers, measures the search time, and prints
the ranked sentences.

```rust
{{#include ../../examples/semantic-search/src/main.rs:search_answer}}
```

From the repository root, run the command for your board.

```sh
just run_s3
# ESP32-S31, ESP32-C5, ESP32-C6, or ESP32-P4
just run_s31
just run_c5
just run_c6
just run_p4
```

```text
potion ready: 29528 tokens x 64 dims, table 7382 KiB in flash
indexed 8 sentences

Indexed sentences:
  - The quick brown fox jumps over the lazy dog.
  - A fast auburn fox leaps above a sleepy hound.
  - ...

> The quick brown fox jumps over the lazy dog.
<timing> us
  1. [<score>] The quick brown fox jumps over the lazy dog.
  2. [<score>] A fast auburn fox leaps above a sleepy hound.
  3. [<score>] ...

>
```
