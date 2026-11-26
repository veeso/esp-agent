//! Checks that building a run-time vocabulary reports exhausted memory and never aborts.
//!
//! This binary holds one test, because it replaces the global allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use esp_agent::{RuntimeVocabulary, TurnCommands, VocabularyError};

thread_local! {
    /// How many more allocations the current thread may make, or `None` for no limit.
    static ALLOWED: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The system allocator, refusing the allocations of a thread that used up its allowance.
struct RefusingAllocator;

fn refuse() -> bool {
    // The slot is gone while a thread shuts down; allocations are then never refused.
    ALLOWED
        .try_with(|allowed| match allowed.get() {
            None => false,
            Some(0) => true,
            Some(left) => {
                allowed.set(Some(left - 1));
                false
            }
        })
        .unwrap_or(false)
}

// SAFETY: every method either returns null, which `GlobalAlloc` allows to report failure and
// which leaves an existing block untouched, or forwards its arguments unchanged to `System`,
// which upholds the contract. The allowance is a thread-local `Cell`, which never allocates.
unsafe impl GlobalAlloc for RefusingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller upholds the `GlobalAlloc::alloc_zeroed` contract for `layout`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
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
static ALLOCATOR: RefusingAllocator = RefusingAllocator;

#[derive(TurnCommands)]
#[expect(
    dead_code,
    reason = "the test only adds phrases and never reads an action"
)]
enum Action {
    #[command("dim {target} to {level}")]
    Dim { level: u8 },
}

#[test]
fn insert_reports_out_of_memory_and_keeps_the_vocabulary() {
    let mut words = RuntimeVocabulary::<Action>::new();
    // Fail the first allocation of the insert, then the second, until none is left to fail.
    let mut allowed = 0;
    loop {
        ALLOWED.set(Some(allowed));
        let result = words.insert_explicit("Dim", "imposta {target} al {level}% {{ora}}");
        ALLOWED.set(None);
        let Err(error) = result else {
            break;
        };
        assert_eq!(error, VocabularyError::OutOfMemory, "allocation {allowed}");
        assert_eq!((words.len(), words.bytes()), (0, 0), "allocation {allowed}");
        allowed += 1;
        assert!(allowed < 100, "the insert never succeeded");
    }
    assert!(
        allowed > 5,
        "only {allowed} allocations were there to refuse"
    );
    assert_eq!(words.len(), 1);
}
