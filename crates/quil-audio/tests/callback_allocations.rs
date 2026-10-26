use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use quil_audio::mono_ring;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

// SAFETY: this wrapper forwards every allocation with the unchanged layout to
// the process system allocator; the counter itself is lock-free and thread-safe.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller supplies a valid layout and System upholds the
        // allocation contract; no pointer is inspected by this wrapper.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: pointer/layout are the same values forwarded by the matching
        // allocation path, as required by GlobalAlloc.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn mono_callback_write_does_not_allocate() {
    let (mut writer, mut reader) = mono_ring(4096, 2).expect("ring");
    let samples = vec![0.25_f32; 1024];

    ALLOCATIONS.store(0, Ordering::SeqCst);
    writer.write(&samples);
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);

    assert_eq!(reader.pending(), 512);
    assert!((reader.pop().expect("sample") - 0.25).abs() < f32::EPSILON);
}
