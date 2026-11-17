//! The capacity the reference implementation's `append` gives a list, and the size classes behind it.
//!
//! A list is a slice of 16-byte elements that hold pointers. When an append does not fit, the runtime picks a
//! new capacity (double up to 256 elements, then a quarter more) and rounds the byte size up to an allocation
//! class, which for lists of 64 to 70 elements and others gives a capacity that is not a power of two. A
//! reallocating append charges the whole new array to the memory budget, so where the limit fires follows this
//! rule.

/// Bytes of one element.
const ELEM: usize = 16;
/// Past this many elements a doubling slice grows by a quarter plus 192 instead.
const DOUBLING_LIMIT: usize = 256;
const MAX_SMALL_SIZE: usize = 32768;
/// Objects larger than this carry a header word that the class size includes.
const MIN_SIZE_FOR_HEADER: usize = 512;
const HEADER: usize = 8;
const PAGE: usize = 8192;

/// The sizes of the allocation classes.
const CLASSES: [u16; 68] = [
    0, 8, 16, 24, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288, 320,
    352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 896, 1024, 1152, 1280, 1408, 1536, 1792,
    2048, 2304, 2688, 3072, 3200, 3456, 4096, 4864, 5376, 6144, 6528, 6784, 6912, 8192, 9472, 9728,
    10240, 10880, 12288, 13568, 14336, 16384, 18432, 19072, 20480, 21760, 24576, 27264, 28672,
    32768,
];

/// The usable bytes of the block the allocator hands out for a request of `size` bytes that hold pointers.
fn round_up_size(size: usize) -> usize {
    if size <= MAX_SMALL_SIZE - HEADER {
        let request = if size > MIN_SIZE_FOR_HEADER {
            size + HEADER
        } else {
            size
        };
        let class = CLASSES.partition_point(|&c| usize::from(c) < request);
        return usize::from(CLASSES[class]) - (request - size);
    }
    match size.checked_add(PAGE - 1) {
        Some(padded) => padded & !(PAGE - 1),
        None => size,
    }
}

/// The capacity of a list of `old_cap` that has to hold `new_len` elements after an append that does not fit.
pub fn grown_cap(old_cap: usize, new_len: usize) -> usize {
    let wanted = if new_len > 2 * old_cap {
        new_len
    } else if old_cap < DOUBLING_LIMIT {
        2 * old_cap
    } else {
        let mut cap = old_cap;
        while cap < new_len {
            cap += (cap + 3 * DOUBLING_LIMIT) >> 2;
        }
        cap
    };
    round_up_size(wanted.saturating_mul(ELEM)) / ELEM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appending_one_at_a_time_gives_the_capacities_the_runtime_gives() {
        let mut cap = 0;
        let mut seen = Vec::new();
        for len in 1..=100 {
            if len > cap {
                cap = grown_cap(cap, len);
                seen.push(cap);
            }
        }
        assert_eq!(seen, [1, 2, 4, 8, 16, 32, 71, 143]);
    }

    #[test]
    fn a_big_append_to_an_empty_list_rounds_up_to_a_class() {
        assert_eq!(grown_cap(0, 3), 3);
        assert_eq!(grown_cap(0, 5), 5);
        assert_eq!(grown_cap(0, 17), 18);
        assert_eq!(grown_cap(16, 17), 32);
    }
}
