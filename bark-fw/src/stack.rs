//! The call stack's high-water mark: the stack's free memory, from the
//! end of static memory (cortex-m-rt's `__sheap`) to the stack pointer,
//! is painted with a pattern at boot, and the deepest the stack has gone
//! is where the pattern first ends. It is logged in the node's 10 s line,
//! to measure the depth `docs/core-interface.md` §6 can only estimate.

use core::ptr::{addr_of, read_volatile, write_volatile};

/// The pattern: a word no stack frame is likely to hold.
const PAINT: u32 = 0xA5A5_5A5A;

extern "C" {
    /// The end of static memory (cortex-m-rt's `link.x`).
    static __sheap: u32;
    /// The top of the stack (cortex-m-rt's `link.x`).
    static _stack_start: u32;
}

/// Paint the stack's free memory below the current frame. With
/// interrupts masked, so that no handler's frame is painted over.
pub fn paint() {
    cortex_m::interrupt::free(|_| {
        let sp = cortex_m::register::msp::read() as usize;
        let mut p = addr_of!(__sheap) as usize;
        while p + 4 <= sp {
            // SAFETY: between the end of static memory and the stack
            // pointer lies only stack not yet used, and no interrupt runs.
            unsafe { write_volatile(p as *mut u32, PAINT) };
            p += 4;
        }
    });
}

/// The stack's bytes: from the end of static memory to its top.
pub fn size() -> usize {
    addr_of!(_stack_start) as usize - addr_of!(__sheap) as usize
}

/// The most bytes of stack used since [`paint`].
pub fn used() -> usize {
    let bottom = addr_of!(__sheap) as usize;
    let top = bottom + size();
    let mut p = bottom;
    // SAFETY: the words read lie between the end of static memory and the
    // top of the stack, all RAM.
    while p < top && unsafe { read_volatile(p as *const u32) } == PAINT {
        p += 4;
    }
    top - p
}
