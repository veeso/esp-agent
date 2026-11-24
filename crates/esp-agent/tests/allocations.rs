//! Checks that the reusable-buffer paths do not touch the heap once they are warm.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use esp_agent::{Embedder, Index, Model, Retriever, Scratch, model};

thread_local! {
    /// Heap allocations made by the current thread, so parallel tests do not disturb each other.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting every allocation made through it on the calling thread.
struct CountingAllocator;

fn count_allocation() {
    // The slot is gone while a thread shuts down; allocations then go uncounted.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

// SAFETY: every method forwards its arguments unchanged to `System`, which upholds the
// `GlobalAlloc` contract. The counter only touches a thread-local `Cell`, which never allocates.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller upholds the `GlobalAlloc::alloc_zeroed` contract for `layout`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller upholds the `GlobalAlloc::realloc` contract for `ptr`, `layout`
        // and `new_size`, and `ptr` came from this allocator, which forwards to `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds the `GlobalAlloc::dealloc` contract, and `ptr` came from
        // this allocator, which forwards to `System`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Returns how many heap allocations `f` made on the calling thread.
fn allocations_during(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    f();
    ALLOCATIONS.with(Cell::get) - before
}

#[test]
fn counter_sees_allocations() {
    assert_eq!(
        allocations_during(|| drop(std::hint::black_box(vec![1u8; 16]))),
        1
    );
}

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");

#[test]
fn encode_with_a_warm_scratch_does_not_allocate() {
    let tokenizer = Embedder::new(TINY_2D).tokenizer();
    let mut scratch = Scratch::new();
    tokenizer.encode_with("hello world fox dog", &mut scratch);

    let allocations = allocations_during(|| {
        std::hint::black_box(tokenizer.encode_with("dog hello", &mut scratch));
    });

    assert_eq!(allocations, 0);
}

#[test]
fn embed_with_a_warm_scratch_does_not_allocate() {
    let embedder = Embedder::new(TINY_2D);
    let mut scratch = Scratch::new();
    let mut out = [0.0f32; 2];
    embedder.embed_with("hello world fox dog", &mut scratch, &mut out);

    let allocations = allocations_during(|| {
        embedder.embed_with("fox hello", &mut scratch, &mut out);
    });

    assert_eq!(allocations, 0);
}

#[test]
fn search_into_with_enough_capacity_does_not_allocate() {
    let mut index = Index::new(2);
    for (payload, vector) in [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [-1.0, 0.0], [1.0, -1.0]]
        .iter()
        .enumerate()
    {
        index.insert(vector, payload as u8);
    }
    let mut hits = Vec::with_capacity(3);

    let allocations = allocations_during(|| {
        index.search_into(&[1.0, 0.0], 3, &mut hits);
    });

    assert_eq!(allocations, 0);
    assert_eq!(hits.len(), 3);
}

#[test]
fn retriever_search_with_a_warm_scratch_does_not_allocate() {
    let mut retriever = Retriever::with_capacity(Embedder::new(TINY_2D), 3);
    retriever.extend([("hello", ()), ("world", ()), ("dog", ())]);
    let mut scratch = Scratch::new();
    let mut hits = Vec::with_capacity(3);
    retriever.search_with("hello world fox dog", 3, &mut scratch, &mut hits);

    let allocations = allocations_during(|| {
        retriever.search_with("fox dog", 3, &mut scratch, &mut hits);
    });

    assert_eq!(allocations, 0);
    assert_eq!(hits.len(), 3);
}

#[test]
fn repeated_unicode_embedding_and_search_do_not_allocate() {
    let embedder = Embedder::new(TINY_2D);
    let expected = embedder.embed("hello");
    let mut retriever = Retriever::new(embedder);
    retriever.insert("hello", ());
    let mut out = [0.0; 2];
    let mut hits = Vec::with_capacity(1);
    let mut scratch = Scratch::new();
    let text = "hel\u{0301}\u{0301}\u{0301}\u{0301}lo \u{302e}\u{1b44}";
    retriever.search_with(text, 1, &mut scratch, &mut hits);

    let allocations = allocations_during(|| {
        for _ in 0..2 {
            retriever
                .embedder()
                .embed_with(text, &mut scratch, &mut out);
            retriever.search_with(text, 1, &mut scratch, &mut hits);
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(out.as_slice(), expected);
    assert_eq!(hits[0].index, 0);
}

#[test]
fn search_into_ranks_increasing_scores_in_reused_storage() {
    let mut index = Index::new(2);
    for i in 0..1024 {
        index.insert(&[i as f32 / 1024.0, 1.0], ());
    }
    let mut hits = Vec::with_capacity(index.len());
    for k in [1024, 513, 2] {
        let allocations = allocations_during(|| index.search_into(&[1.0, 0.0], k, &mut hits));
        assert_eq!(allocations, 0);
        assert!(hits.iter().map(|hit| hit.index).eq((1024 - k..1024).rev()));
    }
}
