//! Temporary diagnostic allocator wrapper for the production OOM investigation.
//!
//! The public server dies from a ~3 GB anonymous-memory burst that happens
//! entirely inside a single 10-second memwatch sampling gap — our external
//! probes have never once caught the process mid-growth. This wraps the real
//! allocator ([`mimalloc::MiMalloc`]) with a live-byte counter and, when the
//! running total climbs past a high-water mark on the way to the kill, logs a
//! captured backtrace naming the allocation site.
//!
//! It reports only above [`START_WATERMARK`] (well over the ~1.4 GB legitimate
//! cold-parse peak), so normal operation — CLI, TUI, ordinary serve traffic —
//! pays only two relaxed atomics per allocation and never symbolizes anything.
//!
//! This is scaffolding: delete the module, its `#[global_allocator]` wiring in
//! `main.rs`, and the debuginfo/no-strip build changes once the culprit is
//! identified.

use mimalloc::MiMalloc;
use std::{
    alloc::{GlobalAlloc, Layout},
    backtrace::Backtrace,
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

const MIB: usize = 1024 * 1024;

/// Live allocated bytes below which nothing is ever reported. Set above the
/// ~1.4 GB peak a legitimate cold-crate parse reaches, so only an abnormal
/// climb toward the ~3.6 GB kill trips it.
const START_WATERMARK: usize = 1750 * MIB;

/// Once past [`START_WATERMARK`], report again each time the live total climbs
/// another step — so the burst leaves a trail of backtraces from ~1.75 GB up to
/// the kill rather than a single sample.
const STEP: usize = 256 * MIB;

/// Any single allocation at least this large is reported on its own, with a
/// backtrace, regardless of the running total — a lone giant buffer is exactly
/// the shape we most want to catch.
const SINGLE_ALLOC: usize = 256 * MIB;

/// Live bytes currently allocated through this wrapper (requested sizes; a
/// proxy for heap anon, not exact RSS).
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// The next live-total high-water at which a crossing is reported. Advanced by
/// CAS so a burst reports once per [`STEP`] rather than on every allocation.
static NEXT_REPORT: AtomicUsize = AtomicUsize::new(START_WATERMARK);

/// Monotonic report sequence, so the ordering of trail entries is unambiguous
/// in the journal even if timestamps collide.
static REPORT_SEQ: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Reentrancy guard: capturing a backtrace and logging both allocate, which
    /// would re-enter this allocator. While set, allocations on this thread are
    /// counted but never reported.
    static IN_REPORT: Cell<bool> = const { Cell::new(false) };
}

/// The real allocator every request is ultimately served from.
static INNER: MiMalloc = MiMalloc;

pub struct AllocWatch;

unsafe impl GlobalAlloc for AllocWatch {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { INNER.alloc(layout) };
        if !ptr.is_null() {
            record_growth(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { INNER.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_growth(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { INNER.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { INNER.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            let old = layout.size();
            if new_size >= old {
                record_growth(new_size - old);
            } else {
                LIVE.fetch_sub(old - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

/// Add `size` to the live total and report if this allocation is a giant single
/// block or pushed the running total past the next high-water mark.
#[inline]
fn record_growth(size: usize) {
    let total = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    if size >= SINGLE_ALLOC || total >= NEXT_REPORT.load(Ordering::Relaxed) {
        report(size, total);
    }
}

#[cold]
#[inline(never)]
fn report(size: usize, total: usize) {
    // Backtrace capture and logging allocate; don't recurse.
    if IN_REPORT.with(|flag| flag.replace(true)) {
        return;
    }

    // Advance the high-water past the current total so the next report waits a
    // full STEP; a giant single alloc under the mark reports without moving it.
    let mut current = NEXT_REPORT.load(Ordering::Relaxed);
    while total >= current {
        let next = total - (total % STEP) + STEP;
        match NEXT_REPORT.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }

    let seq = REPORT_SEQ.fetch_add(1, Ordering::Relaxed);
    let backtrace = Backtrace::force_capture();
    log::warn!(
        "[allocwatch #{seq}] live={} MiB, this_alloc={} MiB\n{backtrace}",
        total / MIB,
        size / MIB,
    );
    // Force the record out now: the kill lands ~1+ GB above this and a buffered
    // line would be lost to the SIGKILL.
    log::logger().flush();

    IN_REPORT.with(|flag| flag.set(false));
}
