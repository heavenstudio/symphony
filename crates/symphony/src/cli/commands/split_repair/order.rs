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
//! different contents would make their order matter, and that is refused, as is a chunk whose name,
//! linking or relocation sections name its segments by index, which a new order would leave naming
//! others (the splitter removes the last two; a chunk still holding one is not its output).
//!
//! The splitter writes a segment a symbol and a table entry a function, so a chunk's segments are
//! thousands, each with its own address and length, where the symbols lie side by side or over one
//! another: [`order`] also joins each segment that touches or overlaps the one before it into that
//! one, which fills the same places with the same contents (they agree where they overlap) in fewer
//! bytes. It joins them only where every segment of the section is active at a constant address of
//! memory or table 0: only `memory.init`, `data.drop`, `table.init` and `elem.drop` name a segment
//! by index, and the toolchain uses them for passive segments alone, which a chunk then has none of.

use super::Failure;
use super::wasm::{self, DATA, DATA_COUNT, ELEMENT, Fill, Mode, Module, Segment};

/// A chunk with its segments in order.
pub(super) struct Ordered {
    pub(super) bytes: Vec<u8>,
    /// The active segments that changed place, the segments in all, and those joined to the one
    /// before them.
    pub(super) moved: usize,
    pub(super) segments: usize,
    pub(super) joined: usize,
}

pub(super) fn order(chunk: &[u8]) -> Result<Ordered, Failure> {
    let mut module = Module::read(chunk).map_err(Failure::Input)?;
    let (mut moved, mut total, mut joined) = (0, 0, 0);
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
        let ordered: Vec<&Segment> = order.iter().map(|&from| &segments[from]).collect();
        let written = join(&ordered);
        if changed == 0 && written.is_none() {
            continue;
        }
        moved += changed;
        let body = match &written {
            Some(written) => {
                joined += segments.len() - written.len();
                wasm::segments_body(written.iter().map(Vec::as_slice))
            }
            None => wasm::segments_body(ordered.iter().map(|segment| segment.raw)),
        };
        let count = written.as_ref().map_or(segments.len(), Vec::len);
        module
            .section_mut(id)
            .expect("the section read above")
            .set_body(body);
        // The data count section, when the chunk has one, says how many data segments follow.
        if id == DATA
            && let Some(section) = module.section_mut(DATA_COUNT)
        {
            let mut body = Vec::new();
            wasm::write_u32(&mut body, count as u32);
            if body != section.body() {
                section.set_body(body);
            }
        }
    }
    if moved == 0 && joined == 0 {
        return Ok(Ordered {
            bytes: chunk.to_vec(),
            moved,
            segments: total,
            joined,
        });
    }
    if wasm::names_segments(&module).map_err(Failure::Input)? {
        return Err(Failure::Unsafe(
            "its name section names its segments, which a new order of them would leave naming \
             others"
                .into(),
        ));
    }
    if let Some(section) = module
        .sections
        .iter()
        .filter_map(wasm::Section::custom)
        .map(|(name, _)| name)
        .find(|name| *name == "linking" || name.starts_with("reloc."))
    {
        return Err(Failure::Unsafe(format!(
            "its {section} section names its segments by index, which a new order of them would \
             leave naming others"
        )));
    }
    Ok(Ordered {
        bytes: module.write(),
        moved,
        segments: total,
        joined,
    })
}

/// The `ordered` segments' encodings with each one that touches or overlaps the one before it
/// joined into that one, in the short form (flags 0: memory or table 0, `i32.const ADDRESS`, and
/// for a table, function indices); `None` when none touches another, or when a segment of the
/// section is not active at a constant address in that form, whose index may be named.
fn join(ordered: &[&Segment]) -> Option<Vec<Vec<u8>>> {
    let short = |segment: &Segment| match segment.mode {
        Mode::Active {
            index: 0,
            offset: Some(offset),
        } if segment.raw.first() == Some(&0) => Some(offset as u32),
        _ => None,
    };
    let mut runs: Vec<(u32, Vec<&[u8]>)> = Vec::new();
    let mut joined = false;
    for segment in ordered {
        let start = short(segment)?;
        // A data segment's bytes, or an element segment's function indices, one a slot.
        let slots: Vec<&[u8]> = match &segment.fill {
            Fill::Bytes(bytes) => bytes.chunks(1).collect(),
            Fill::Items(items) => items.clone(),
        };
        if let Some((run_start, run)) = runs.last_mut() {
            let end = u64::from(*run_start) + run.len() as u64;
            if u64::from(start) <= end {
                // The slots they share agree (`agree`): the run takes this one's slots past its end.
                let shared = (end - u64::from(start)) as usize;
                run.extend(slots.into_iter().skip(shared));
                joined = true;
                continue;
            }
        }
        runs.push((start, slots));
    }
    if !joined {
        return None;
    }
    Some(
        runs.into_iter()
            .map(|(start, slots)| {
                // Flags 0, `i32.const start`, `end`, then the slots' count and the slots.
                let mut raw = vec![0, 0x41];
                wasm::write_i32(&mut raw, start as i32);
                raw.push(0x0b);
                wasm::write_u32(&mut raw, slots.len() as u32);
                for slot in slots {
                    raw.extend_from_slice(slot);
                }
                raw
            })
            .collect(),
    )
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
