//! The splitter's second fault: a chunk's segments in the order of a hash map.
//!
//! wasm-split-cli 0.7.10 adds a chunk's data segments, one a symbol, in the order of a `HashSet`
//! (`clear_data_segments`) and its table entries, one an element segment, in the order of a
//! `HashMap` (`create_ifunc_initializers`), both of which change with every run: two builds of the
//! same sources give chunks of the same size and other bytes, so other names. [`order`] puts every
//! active segment of a chunk's data and element sections in the order of its memory or table, its
//! address and its encoding, in the places active segments hold, so the same segments give the same
//! bytes. Active segments are dropped once written, so no instruction can tell their indices apart;
//! a passive or declared segment keeps its index. Two active segments that fill one place with
//! different contents would make their order matter, and that is refused.

use super::Failure;
use super::wasm::{self, DATA, ELEMENT, Mode, Module, Segment};

/// A chunk with its segments in order.
pub(super) struct Ordered {
    pub(super) bytes: Vec<u8>,
    /// The active segments that changed place, and the segments in all.
    pub(super) moved: usize,
    pub(super) segments: usize,
}

pub(super) fn order(chunk: &[u8]) -> Result<Ordered, Failure> {
    let mut module = Module::read(chunk).map_err(Failure::Input)?;
    let (mut moved, mut total) = (0, 0);
    for (id, what) in [(ELEMENT, "element"), (DATA, "data")] {
        let Some(section) = module.section(id) else {
            continue;
        };
        let segments = if id == DATA {
            wasm::data_segments(section.body())
        } else {
            wasm::element_segments(section.body())
        }
        .map_err(Failure::Input)?;
        agree(&segments, what)?;
        let order = in_order(&segments);
        let changed = order
            .iter()
            .enumerate()
            .filter(|(at, from)| at != *from)
            .count();
        total += segments.len();
        if changed == 0 {
            continue;
        }
        moved += changed;
        let body = wasm::segments_body(order.iter().map(|&from| segments[from].raw));
        module
            .section_mut(id)
            .expect("the section read above")
            .set_body(body);
    }
    if moved == 0 {
        return Ok(Ordered {
            bytes: chunk.to_vec(),
            moved,
            segments: total,
        });
    }
    if wasm::names_segments(&module).map_err(Failure::Input)? {
        return Err(Failure::Unsafe(
            "its name section names its segments, which a new order of them would leave naming \
             others"
                .into(),
        ));
    }
    Ok(Ordered {
        bytes: module.write(),
        moved,
        segments: total,
    })
}

/// The segments' new order, as the index each place takes its segment from: the active ones by
/// memory or table, address and encoding, in the places active ones hold; the others where they
/// are.
fn in_order(segments: &[Segment]) -> Vec<usize> {
    let places: Vec<usize> = (0..segments.len())
        .filter(|&at| matches!(segments[at].mode, Mode::Active { .. }))
        .collect();
    let mut sorted = places.clone();
    sorted.sort_by_key(|&at| (key(&segments[at]), segments[at].raw));
    let mut order: Vec<usize> = (0..segments.len()).collect();
    for (place, from) in places.into_iter().zip(sorted) {
        order[place] = from;
    }
    order
}

/// An active segment's memory or table, and its address (an `i32.const` read unsigned, as a memory
/// or a table reads it), when it has one.
fn key(segment: &Segment) -> (u32, Option<u32>) {
    match segment.mode {
        Mode::Active { index, offset } => (index, offset.map(|offset| offset as u32)),
        Mode::Passive | Mode::Declared => (u32::MAX, None),
    }
}

/// Fails two active segments that fill one place of a memory or a table with different contents,
/// or one whose address is not a constant, whose order could matter.
fn agree(segments: &[Segment], what: &str) -> Result<(), Failure> {
    let mut placed = Vec::new();
    for (at, segment) in segments.iter().enumerate() {
        let Mode::Active { index, offset } = segment.mode else {
            continue;
        };
        let Some(offset) = offset else {
            return Err(Failure::Unsafe(format!(
                "its {what} segment {at} has no constant address: whether its order matters \
                 cannot be told"
            )));
        };
        let start = u64::from(offset as u32);
        placed.push((index, start, start + segment.fill.len() as u64, at));
    }
    placed.sort();
    for (one, &(index, start, end, at)) in placed.iter().enumerate() {
        for &(other_index, other_start, other_end, other_at) in &placed[one + 1..] {
            if other_index != index || other_start >= end {
                break;
            }
            for place in other_start..end.min(other_end) {
                let (mine, theirs) = (&segments[at].fill, &segments[other_at].fill);
                if !mine.same(
                    (place - start) as usize,
                    theirs,
                    (place - other_start) as usize,
                ) {
                    return Err(Failure::Unsafe(format!(
                        "its {what} segments {at} and {other_at} fill {place:#x} with different \
                         contents, so their order matters"
                    )));
                }
            }
        }
    }
    Ok(())
}
