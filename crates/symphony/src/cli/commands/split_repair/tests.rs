//! The repair on small programs built byte by byte to show each fault as dx's splitter makes it:
//! the main module it writes with a chunk's merged constant zeroed and its zero runs packed away as
//! wasm-opt packs them, and the chunks two runs write with the same segments in two orders.

use super::*;
use crate::test_tree::Tree;
use wasm::{write_i32, write_u32};

/// A section: its id and its body.
type Built = (u8, Vec<u8>);

fn module(sections: &[Built]) -> Vec<u8> {
    let mut out = b"\0asm\x01\0\0\0".to_vec();
    for (id, body) in sections {
        out.push(*id);
        write_u32(&mut out, body.len() as u32);
        out.extend_from_slice(body);
    }
    out
}

fn name(out: &mut Vec<u8>, name: &str) {
    write_u32(out, name.len() as u32);
    out.extend_from_slice(name.as_bytes());
}

fn custom(called: &str, contents: &[u8]) -> Built {
    let mut body = Vec::new();
    name(&mut body, called);
    body.extend_from_slice(contents);
    (0, body)
}

fn vector(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    write_u32(&mut out, items.len() as u32);
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// `i32.const value`, `end`.
fn at(value: i32) -> Vec<u8> {
    let mut out = vec![0x41];
    write_i32(&mut out, value);
    out.push(0x0b);
    out
}

/// A data segment active in memory 0 at `address`.
fn active(address: i32, bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(at(address));
    write_u32(&mut out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    out
}

fn passive(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    write_u32(&mut out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    out
}

/// An element segment active in table 0 at `slot`, of these functions.
fn entries(slot: i32, functions: &[u32]) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(at(slot));
    write_u32(&mut out, functions.len() as u32);
    for function in functions {
        write_u32(&mut out, *function);
    }
    out
}

/// One memory of one page.
fn memory() -> Built {
    (5, vec![1, 0x00, 1])
}

/// What a chunk imports from the main module: its memory and its table.
fn chunk_imports() -> Built {
    let mut body = Vec::new();
    write_u32(&mut body, 2);
    name(&mut body, "__wasm_split");
    name(&mut body, "memory");
    body.extend([0x02, 0x00, 1]);
    name(&mut body, "__wasm_split");
    name(&mut body, "__indirect_function_table");
    body.extend([0x01, 0x70, 0x01, 4, 64]);
    (2, body)
}

/// A main module's own import from the page's bindings, which is no chunk's.
fn main_imports() -> Built {
    let mut body = Vec::new();
    write_u32(&mut body, 1);
    name(&mut body, "wbg");
    name(&mut body, "__wbg_log");
    body.extend([0x00, 0]);
    (2, body)
}

/// The program's read-only data at [`BASE`] as the linker merges it: `ok`, then `@{}/{}`, whose last
/// three bytes are also the template `/{}` (`format!("/{}", …)` compiles to it), then `!`.
const BASE: i32 = 0x400;
const DATA: &[u8] = b"ok@{}/{}!";

/// Its data symbols: name, offset and size.
const SYMBOLS: [(&str, u32, u32); 4] = [
    (".L.str.ok", 0, 2),
    (".L.str.route", 2, 6),
    (".L.str.template", 5, 3),
    (".L.str.bang", 8, 1),
];

/// The linking section of a program whose data symbols are `symbols` in segment 0, beside what the
/// repair reads past: a function symbol, a section symbol, an undefined data symbol and the
/// linker's absolute `__heap_base`, whose offset is an address past every segment's end.
fn linking(symbols: &[(&str, u32, u32)]) -> Built {
    linking_with(symbols, 0)
}

/// [`linking`], its absolute `__heap_base` naming `heap` bytes at its address.
fn linking_with(symbols: &[(&str, u32, u32)], heap: u8) -> Built {
    let mut table = Vec::new();
    write_u32(&mut table, symbols.len() as u32 + 4);
    table.extend([0, 0x00, 0]);
    name(&mut table, "main");
    table.extend([3, 0x02, 7]);
    table.extend([1, 0x10]);
    name(&mut table, "elsewhere");
    table.extend([1, 0x84, 0x04]);
    name(&mut table, "__heap_base");
    table.extend([0, 0x80, 0x80, 0x04, heap]);
    for (called, offset, size) in symbols {
        table.extend([1, 0x00]);
        name(&mut table, called);
        write_u32(&mut table, 0);
        write_u32(&mut table, *offset);
        write_u32(&mut table, *size);
    }
    let mut contents = vec![2];
    // An init functions subsection first, which the repair skips.
    contents.extend([6, 1, 0]);
    contents.push(8);
    write_u32(&mut contents, table.len() as u32);
    contents.extend(table);
    custom("linking", &contents)
}

/// The program the linker made, with its relocations' linking section.
fn program_of(data: &[u8], symbols: &[(&str, u32, u32)]) -> Vec<u8> {
    module(&[
        memory(),
        (11, vector(&[active(BASE, data)])),
        linking(symbols),
    ])
}

fn program() -> Vec<u8> {
    program_of(DATA, &SYMBOLS)
}

/// The main module the splitter writes for `data`, after wasm-opt: the bytes of each symbol only a
/// chunk reaches (by name) zeroed, as `prune_main_symbols` zeroes them, then every run of zeros
/// left out of its segments, as wasm-opt's memory packing leaves them out.
fn split_main_of(data: &[u8], symbols: &[(&str, u32, u32)], chunks: &[&str]) -> Vec<u8> {
    let mut bytes = data.to_vec();
    for (called, offset, size) in symbols {
        if chunks.contains(called) {
            bytes[*offset as usize..(*offset + *size) as usize].fill(0);
        }
    }
    let mut segments = Vec::new();
    let mut start = None;
    for at in 0..=bytes.len() {
        match (start, bytes.get(at).copied().unwrap_or(0)) {
            (None, byte) if byte != 0 => start = Some(at),
            (Some(from), 0) => {
                segments.push(active(BASE + from as i32, &bytes[from..at]));
                start = None;
            }
            _ => {}
        }
    }
    let count = segments.len() as u32;
    let mut data_count = Vec::new();
    write_u32(&mut data_count, count);
    module(&[
        main_imports(),
        memory(),
        (12, data_count),
        (11, vector(&segments)),
    ])
}

/// The main module as dx writes it for this program: `.L.str.route` and `.L.str.bang` are a
/// chunk's, the others the main module's.
fn split_main() -> Vec<u8> {
    split_main_of(DATA, &SYMBOLS, &[".L.str.route", ".L.str.bang"])
}

fn linked() -> constants::Program {
    constants::Program::read(&program()).expect("the program")
}

/// The bytes `module`'s active data segments write at [`BASE`], as many as the program's data.
fn memory_of(module: &[u8]) -> Vec<u8> {
    let module = wasm::Module::read(module).unwrap();
    let mut seen = vec![0; DATA.len()];
    for segment in wasm::data_segments(module.section(wasm::DATA).unwrap().body()).unwrap() {
        let (
            wasm::Mode::Active {
                offset: Some(start),
                ..
            },
            wasm::Fill::Bytes(bytes),
        ) = (segment.mode, segment.fill)
        else {
            continue;
        };
        for (at, byte) in bytes.iter().enumerate() {
            if let Some(slot) = seen.get_mut((start - BASE) as usize + at) {
                *slot = *byte;
            }
        }
    }
    seen
}

fn unsafe_why(result: Result<impl Sized, Failure>) -> String {
    match result {
        Err(Failure::Unsafe(why)) => why,
        Err(Failure::Input(why)) => panic!("read as unreadable input: {why}"),
        Ok(_) => panic!("passed"),
    }
}

fn input_why(result: Result<impl Sized, Failure>) -> String {
    match result {
        Err(Failure::Input(why)) => why,
        Err(Failure::Unsafe(why)) => panic!("read as unsafe: {why}"),
        Ok(_) => panic!("passed"),
    }
}

// The merged constants.

#[test]
fn the_splitter_s_main_module_reads_a_merged_template_as_zeros_and_the_check_fails_it() {
    let main = split_main();
    // The template's three bytes read zero until the route's chunk loads.
    assert_eq!(memory_of(&main), b"ok\0\0\0\0\0\0\0");
    let why = unsafe_why(constants::check(&linked(), &main));
    assert_eq!(
        why,
        "the main module reads zero at 0x405, where .L.str.route and .L.str.template lie: a merged \
         constant the splitter zeroed for a chunk, which the main module may read"
    );
}

#[test]
fn the_repair_writes_the_merged_template_back_and_leaves_the_chunks_own_bytes_zero() {
    let main = split_main();
    let repaired = constants::repair(&linked(), &main).expect("repaired");
    assert_eq!((repaired.runs, repaired.written), (1, 3));
    // `/{}` reads whole again; `@{}` and `!`, which no symbol of the main module holds, stay zero.
    assert_eq!(memory_of(&repaired.bytes), b"ok\0\0\0/{}\0");
    constants::check(&linked(), &repaired.bytes).expect("the repaired module passes");
    // Written back after the module's own segments, which stay as they were, and counted.
    let module = wasm::Module::read(&repaired.bytes).unwrap();
    let segments = wasm::data_segments(module.section(wasm::DATA).unwrap().body()).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1].raw, active(BASE + 5, b"/{}"));
    assert_eq!(module.section(wasm::DATA_COUNT).unwrap().body(), [2]);
    // A second repair finds nothing to write back.
    let again = constants::repair(&linked(), &repaired.bytes).unwrap();
    assert_eq!((again.runs, again.bytes), (0, repaired.bytes));
}

#[test]
fn a_kept_constant_whose_tail_a_chunk_s_constant_shares_reads_whole_after_the_repair() {
    // Here the route is the main module's and the template a chunk's: zeroing the template zeroes
    // the route's tail.
    let main = split_main_of(DATA, &SYMBOLS, &[".L.str.template", ".L.str.bang"]);
    assert_eq!(memory_of(&main), b"ok@{}\0\0\0\0");
    let repaired = constants::repair(&linked(), &main).unwrap();
    assert_eq!(memory_of(&repaired.bytes), b"ok@{}/{}\0");
}

#[test]
fn constants_that_overlap_in_part_are_written_back_where_they_overlap() {
    // `{}/` (2..5 of `@{}/{}` read from 3) lies over the route's middle and nothing else.
    let symbols = [
        (".L.str.ok", 0, 2),
        (".L.str.route", 2, 6),
        (".L.str.middle", 3, 3),
        (".L.str.bang", 8, 1),
    ];
    let program = constants::Program::read(&program_of(DATA, &symbols)).unwrap();
    let main = split_main_of(DATA, &symbols, &[".L.str.route"]);
    let repaired = constants::repair(&program, &main).unwrap();
    assert_eq!(memory_of(&repaired.bytes), b"ok\0{}/\0\0!");
    assert_eq!((repaired.runs, repaired.written), (1, 3));
}

#[test]
fn a_main_module_the_splitter_zeroed_nothing_in_is_kept_byte_for_byte() {
    let main = split_main_of(DATA, &SYMBOLS, &[]);
    let repaired = constants::repair(&linked(), &main).unwrap();
    assert_eq!((repaired.runs, repaired.written), (0, 0));
    assert_eq!(repaired.bytes, main);
}

#[test]
fn the_check_fails_a_zero_no_symbol_holds() {
    // The program's `!` has no symbol, and the main module reads zero there.
    let program = constants::Program::read(&program_of(DATA, &SYMBOLS[..3])).unwrap();
    let main = split_main_of(DATA, &SYMBOLS, &[".L.str.bang"]);
    assert_eq!(
        unsafe_why(constants::repair(&program, &main)),
        "the main module reads zero at 0x408, which no data symbol holds"
    );
}

#[test]
fn the_check_fails_a_symbol_zeroed_in_part() {
    // Two bytes of `ok`, which no other symbol shares, and only one of them zero.
    let main = split_main_of(b"o\0@{}/{}!", &SYMBOLS, &[]);
    assert_eq!(
        unsafe_why(constants::check(&linked(), &main)),
        "the main module reads zero over part of .L.str.ok at 0x400: the splitter zeroes a whole \
         symbol or none of it, and the repair rests on that"
    );
}

#[test]
fn the_check_fails_a_main_module_whose_data_is_not_the_program_s() {
    let main = split_main_of(b"OK@{}/{}!", &SYMBOLS, &[]);
    assert_eq!(
        unsafe_why(constants::repair(&linked(), &main)),
        "the main module holds 0x4f at 0x400, where the program holds 0x6f: it is not that \
         program's main module"
    );
}

#[test]
fn a_program_linked_without_its_relocations_is_refused_naming_emit_relocs() {
    let bare = module(&[memory(), (11, vector(&[active(BASE, DATA)]))]);
    let Err(why) = constants::Program::read(&bare) else {
        panic!("read")
    };
    assert_eq!(
        why,
        "it has no linking section: the program is linked with --emit-relocs, as dx links it for \
         --wasm-split"
    );
}

#[test]
fn a_main_module_with_a_passive_data_segment_is_not_read() {
    let main = module(&[
        memory(),
        (11, vector(&[active(BASE, b"ok"), passive(b"later")])),
    ]);
    assert_eq!(
        input_why(constants::repair(&linked(), &main)),
        "the main module's data segment 1 is not placed at a constant address of memory 0, which \
         the repair does not read"
    );
}

/// A data segment active in memory `memory` at `i32.const address`, in the form that names it.
fn active_in(memory: u32, address: i32, bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![2];
    write_u32(&mut out, memory);
    out.extend(at(address));
    write_u32(&mut out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    out
}

/// A data segment active in memory 0 at `i64.const address`, as a 64-bit memory places it.
fn active_wide(address: i64, bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0x42];
    // Signed LEB128 of `address`.
    let mut value = address;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0) {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out.push(0x0b);
    write_u32(&mut out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    out
}

#[test]
fn a_main_module_with_data_in_another_memory_is_not_read() {
    // The same bytes at the same address, but in memory 1: not what the repair reads or writes.
    let main = module(&[
        memory(),
        (
            11,
            vector(&[active(BASE, b"ok"), active_in(1, BASE + 5, b"/{}")]),
        ),
    ]);
    assert_eq!(
        input_why(constants::repair(&linked(), &main)),
        "the main module's data segment 1 is not placed at a constant address of memory 0, which \
         the repair does not read"
    );
    // Memory 0 named by the form that names its memory is read as the short form is.
    let named = module(&[
        memory(),
        (
            11,
            vector(&[active_in(0, BASE, b"ok"), active(BASE + 5, b"/{}")]),
        ),
    ]);
    let repaired = constants::repair(&linked(), &named).unwrap();
    assert_eq!((repaired.runs, repaired.written), (0, 0));
    assert_eq!(repaired.bytes, named);
}

#[test]
fn a_segment_placed_by_an_i64_constant_has_no_address_the_repair_reads() {
    let body = vector(&[active_wide(0x400, b"ok"), active(0x500, b"next")]);
    let segments = wasm::data_segments(&body).unwrap();
    // Its whole constant read past, so the segment after it is read in place.
    assert_eq!(
        segments[0].mode,
        wasm::Mode::Active {
            index: 0,
            offset: None
        }
    );
    assert!(matches!(segments[0].fill, wasm::Fill::Bytes(b"ok")));
    assert_eq!(
        segments[1].mode,
        wasm::Mode::Active {
            index: 0,
            offset: Some(0x500)
        }
    );
    assert!(matches!(segments[1].fill, wasm::Fill::Bytes(b"next")));
    // Neither the program, the main module nor a chunk is read past it.
    let wide = module(&[
        memory(),
        (11, vector(&[active_wide(BASE.into(), DATA)])),
        linking(&SYMBOLS),
    ]);
    assert_eq!(
        constants::Program::read(&wide).err(),
        Some("its data segment 0 is not placed at a constant address of memory 0".into())
    );
    let main = module(&[memory(), (11, vector(&[active_wide(BASE.into(), b"ok")]))]);
    assert_eq!(
        input_why(constants::repair(&linked(), &main)),
        "the main module's data segment 0 is not placed at a constant address of memory 0, which \
         the repair does not read"
    );
    assert_eq!(
        unsafe_why(order::order(&chunk(
            &[active(0x500, b"b"), active_wide(0x400, b"a")],
            &[]
        ))),
        "its data segment 1 has no constant address: whether its order matters cannot be told"
    );
}

#[test]
fn an_absolute_symbol_names_no_segment_s_bytes_whatever_its_size() {
    // The linker's `__heap_base`, here of four bytes at an address past every segment's end: were
    // it read as a symbol of segment 0, it would run past that segment's end.
    let program = module(&[
        memory(),
        (11, vector(&[active(BASE, DATA)])),
        linking_with(&SYMBOLS, 4),
    ]);
    let module = wasm::Module::read(&program).unwrap();
    let symbols: Vec<(String, u32, u32)> = wasm::data_symbols(&module)
        .unwrap()
        .into_iter()
        .map(|symbol| (symbol.name, symbol.offset, symbol.size))
        .collect();
    assert_eq!(
        symbols,
        SYMBOLS.map(|(called, offset, size)| (called.to_string(), offset, size))
    );
    let linked = constants::Program::read(&program).expect("the program");
    let repaired = constants::repair(&linked, &split_main()).unwrap();
    assert_eq!(memory_of(&repaired.bytes), b"ok\0\0\0/{}\0");
}

// The chunks' order.

/// A chunk: its data segments, then its element segments, each as encoded, in this order.
fn chunk(data: &[Vec<u8>], elements: &[Vec<u8>]) -> Vec<u8> {
    let mut data_count = Vec::new();
    write_u32(&mut data_count, data.len() as u32);
    module(&[
        chunk_imports(),
        (9, vector(elements)),
        (12, data_count),
        (11, vector(data)),
    ])
}

/// The data and table entries of one chunk: a symbol's bytes each, a function each.
fn chunk_data() -> Vec<Vec<u8>> {
    vec![
        active(0x500, b"tasks"),
        active(0x480, b"reports"),
        active(0x510, b"{}"),
    ]
}

fn chunk_entries() -> Vec<Vec<u8>> {
    vec![entries(9, &[3]), entries(4, &[1]), entries(7, &[2])]
}

/// What order a section's segments hold: each one's encoding.
fn segments_of(chunk: &[u8], id: u8) -> Vec<Vec<u8>> {
    let module = wasm::Module::read(chunk).unwrap();
    let body = module.section(id).unwrap().body();
    let segments = if id == wasm::DATA {
        wasm::data_segments(body)
    } else {
        wasm::element_segments(body)
    };
    segments
        .unwrap()
        .iter()
        .map(|segment| segment.raw.to_vec())
        .collect()
}

#[test]
fn two_splits_whose_segments_differ_in_order_are_the_same_bytes_once_ordered() {
    // As two runs of the splitter write one chunk: the same segments, in two hash maps' orders.
    let (data, elements) = (chunk_data(), chunk_entries());
    let first = chunk(&data, &elements);
    let second = chunk(
        &[data[2].clone(), data[0].clone(), data[1].clone()],
        &[
            elements[1].clone(),
            elements[2].clone(),
            elements[0].clone(),
        ],
    );
    assert_ne!(first, second);
    let (one, two) = (
        order::order(&first).unwrap(),
        order::order(&second).unwrap(),
    );
    assert_eq!(one.bytes, two.bytes);
    assert_eq!(one.bytes.len(), first.len());
    // In the order of their addresses.
    assert_eq!(
        segments_of(&one.bytes, wasm::DATA),
        [data[1].clone(), data[0].clone(), data[2].clone()]
    );
    assert_eq!(
        segments_of(&one.bytes, wasm::ELEMENT),
        [
            elements[1].clone(),
            elements[2].clone(),
            elements[0].clone()
        ]
    );
    assert_eq!((one.moved, one.segments), (5, 6));
    assert_eq!((two.moved, two.segments), (2, 6));
}

#[test]
fn a_chunk_in_order_already_is_kept_byte_for_byte() {
    let ordered = order::order(&chunk(&chunk_data(), &chunk_entries())).unwrap();
    let again = order::order(&ordered.bytes).unwrap();
    assert_eq!((again.moved, again.segments), (0, 6));
    assert_eq!(again.bytes, ordered.bytes);
}

#[test]
fn a_passive_segment_keeps_its_index_while_the_active_ones_take_their_order() {
    let data = [active(0x500, b"b"), passive(b"later"), active(0x400, b"a")];
    let ordered = order::order(&chunk(&data, &[])).unwrap();
    assert_eq!(
        segments_of(&ordered.bytes, wasm::DATA),
        [data[2].clone(), data[1].clone(), data[0].clone()]
    );
}

/// What a chunk's active segments put in each place once its segments are written in order: of a
/// memory (`wasm::DATA`), a byte; of a table, a function index as encoded.
fn places(chunk: &[u8], id: u8) -> std::collections::BTreeMap<(u32, u32), Vec<u8>> {
    let mut places = std::collections::BTreeMap::new();
    let module = wasm::Module::read(chunk).unwrap();
    let Some(section) = module.section(id) else {
        return places;
    };
    let segments = if id == wasm::DATA {
        wasm::data_segments(section.body())
    } else {
        wasm::element_segments(section.body())
    };
    for segment in segments.unwrap() {
        let wasm::Mode::Active {
            index,
            offset: Some(start),
        } = segment.mode
        else {
            continue;
        };
        let slots: Vec<Vec<u8>> = match segment.fill {
            wasm::Fill::Bytes(bytes) => bytes.iter().map(|byte| vec![*byte]).collect(),
            wasm::Fill::Items(items) => items.iter().map(|item| item.to_vec()).collect(),
        };
        for (at, slot) in slots.into_iter().enumerate() {
            places.insert((index, start as u32 + at as u32), slot);
        }
    }
    places
}

/// The count a chunk's data count section holds.
fn data_count(chunk: &[u8]) -> u32 {
    let module = wasm::Module::read(chunk).unwrap();
    let body = module.section(wasm::DATA_COUNT).unwrap().body();
    wasm::Reader::new(body).u32().unwrap()
}

#[test]
fn segments_that_touch_or_overlap_are_joined_into_one_that_fills_the_same_places() {
    // As the splitter writes a chunk: a segment a symbol, side by side or one over another, and a
    // table entry a function, in slots that follow one another.
    let data = [
        active(0x405, b"/{}"),
        active(0x410, b"far"),
        active(0x400, b"ok"),
        active(0x402, b"@{}/{}"),
        active(0x408, b""),
    ];
    let elements = [entries(5, &[2, 3]), entries(9, &[4]), entries(4, &[1])];
    let split = chunk(&data, &elements);
    let ordered = order::order(&split).unwrap();
    assert_eq!(
        segments_of(&ordered.bytes, wasm::DATA),
        [active(0x400, b"ok@{}/{}"), active(0x410, b"far")]
    );
    assert_eq!(
        segments_of(&ordered.bytes, wasm::ELEMENT),
        [entries(4, &[1, 2, 3]), entries(9, &[4])]
    );
    assert_eq!(data_count(&ordered.bytes), 2);
    assert_eq!((ordered.segments, ordered.joined), (8, 4));
    for id in [wasm::DATA, wasm::ELEMENT] {
        assert_eq!(places(&ordered.bytes, id), places(&split, id));
    }
    assert!(ordered.bytes.len() < split.len());
    // Joined once, nothing is left to join or move.
    let again = order::order(&ordered.bytes).unwrap();
    assert_eq!((again.moved, again.joined), (0, 0));
    assert_eq!(again.bytes, ordered.bytes);
}

#[test]
fn segments_are_joined_only_where_every_one_of_the_section_is_active_in_the_short_form() {
    // A passive segment, which an instruction names by its index: the others keep theirs.
    let with_passive = [active(0x401, b"b"), passive(b"later"), active(0x400, b"a")];
    let ordered = order::order(&chunk(&with_passive, &[])).unwrap();
    assert_eq!(
        segments_of(&ordered.bytes, wasm::DATA),
        [
            with_passive[2].clone(),
            with_passive[1].clone(),
            with_passive[0].clone()
        ]
    );
    assert_eq!((ordered.joined, data_count(&ordered.bytes)), (0, 3));
    // Touching segments of memory 1, or of memory 0 with one in the long form, are ordered and left
    // apart.
    for data in [
        [active_in(1, 0x401, b"b"), active_in(1, 0x400, b"a")],
        [active_in(0, 0x401, b"b"), active(0x400, b"a")],
    ] {
        let ordered = order::order(&chunk(&data, &[])).unwrap();
        assert_eq!((ordered.moved, ordered.joined), (2, 0));
        assert_eq!(
            segments_of(&ordered.bytes, wasm::DATA),
            [data[1].clone(), data[0].clone()]
        );
    }
}

#[test]
fn segments_that_fill_one_place_alike_are_ordered_and_otherwise_refused() {
    // Two symbols the linker merged, each written whole by the chunk: the same bytes where they meet,
    // so one segment of the outer one writes both.
    let alike = [active(0x405, b"/{}"), active(0x402, b"@{}/{}")];
    let ordered = order::order(&chunk(&alike, &[])).unwrap();
    assert_eq!(segments_of(&ordered.bytes, wasm::DATA), [alike[1].clone()]);
    assert_eq!((ordered.moved, ordered.joined), (2, 1));
    let unlike = [active(0x405, b"/{}"), active(0x402, b"@{}/()")];
    assert_eq!(
        unsafe_why(order::order(&chunk(&unlike, &[]))),
        "its data segments 1 and 0 fill 0x406 with different contents, so their order matters"
    );
    let entries = [entries(4, &[1, 2]), entries(5, &[3])];
    assert_eq!(
        unsafe_why(order::order(&chunk(&[], &entries))),
        "its element segments 0 and 1 fill 0x5 with different contents, so their order matters"
    );
}

#[test]
fn a_segment_whose_address_is_not_a_constant_is_refused() {
    // Placed at `global.get 0`.
    let placed = vec![0, 0x23, 0, 0x0b, 1, b'x'];
    assert_eq!(
        unsafe_why(order::order(&chunk(&[active(0x400, b"a"), placed], &[]))),
        "its data segment 1 has no constant address: whether its order matters cannot be told"
    );
}

#[test]
fn a_chunk_whose_name_section_names_its_segments_is_refused_once_they_move() {
    let data = chunk_data();
    let mut named = chunk(&data, &[]);
    // A name section naming data segment 0.
    let mut names = vec![9];
    let mut map = Vec::new();
    write_u32(&mut map, 1);
    write_u32(&mut map, 0);
    name(&mut map, ".rodata.tasks");
    write_u32(&mut names, map.len() as u32);
    names.extend(map);
    let (id, body) = custom("name", &names);
    named.push(id);
    write_u32(&mut named, body.len() as u32);
    named.extend(body);
    assert_eq!(
        unsafe_why(order::order(&named)),
        "its name section names its segments, which a new order of them would leave naming others"
    );
}

#[test]
fn a_chunk_still_carrying_its_linking_or_relocations_is_refused_once_its_segments_move() {
    let with = |section: Built| {
        let mut bytes = chunk(&chunk_data(), &chunk_entries());
        bytes.extend(module(&[section])[8..].iter());
        bytes
    };
    for (section, called) in [
        (linking(&SYMBOLS), "linking"),
        (custom("reloc.DATA", &[0, 0]), "reloc.DATA"),
        (custom("reloc.CODE", &[3, 0]), "reloc.CODE"),
    ] {
        assert_eq!(
            unsafe_why(order::order(&with(section))),
            format!(
                "its {called} section names its segments by index, which a new order of them \
                 would leave naming others"
            )
        );
    }
    // In order already, nothing moves and nothing it names changes: kept as it is.
    let ordered = order::order(&chunk(&chunk_data(), &chunk_entries()))
        .unwrap()
        .bytes;
    let mut kept = ordered.clone();
    kept.extend(module(&[custom("reloc.DATA", &[0, 0])])[8..].iter());
    assert_eq!(order::order(&kept).unwrap().bytes, kept);
    // Another custom section is no reason to refuse.
    assert!(order::order(&with(custom("producers", b"\0"))).is_ok());
}

// The output folder.

/// The shared chunk the splitter writes empty, as wasm-opt leaves it: no import, no section but
/// its custom ones.
fn empty_chunk() -> Vec<u8> {
    module(&[custom("producers", b"\0")])
}

/// What dx writes for one split build: the main module, the empty shared chunk, the page's chunk,
/// the script naming the three by dx's names, and the page naming the script by its own.
fn dx_output(tree: &Tree, folder: &str, main: &[u8], chunk_bytes: &[u8], dx: &str) {
    let assets = tree.path(&format!("{folder}/assets"));
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("symphony-ui_bg-dxh1a2b.wasm"), main).unwrap();
    std::fs::write(assets.join("chunk_0_split-dxhc0.wasm"), empty_chunk()).unwrap();
    std::fs::write(
        assets.join(format!("module_0_TasksKind-dxh{dx}.wasm")),
        chunk_bytes,
    )
    .unwrap();
    tree.write(
        &format!("{folder}/assets/symphony-ui-dxh{dx}.js"),
        &format!(
            "const c=makeLoad(\"/./assets/chunk_0_split-dxhc0.wasm\",[],f);\
             const a=makeLoad(\"/./assets/module_0_TasksKind-dxh{dx}.wasm\",[c],f);\
             __wbg_init({{module_or_path:\"/./assets/symphony-ui_bg-dxh1a2b.wasm\"}}).then(w=>w);"
        ),
    );
    tree.write(&format!("{folder}/assets/board-dxh77.css"), "main{}");
    tree.write(
        &format!("{folder}/index.html"),
        &format!(
            "<link rel=\"preload\" as=\"script\" href=\"/./assets/symphony-ui-dxh{dx}.js\">\
             <script type=\"module\" async src=\"/./assets/symphony-ui-dxh{dx}.js\"></script>"
        ),
    );
}

fn run(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// Every file under `folder`, by its path there, with its bytes.
fn files(tree: &Tree, folder: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for name in tree.names(&format!("{folder}/assets")) {
        let bytes = std::fs::read(tree.path(&format!("{folder}/assets/{name}"))).unwrap();
        files.push((format!("assets/{name}"), bytes));
    }
    let page = std::fs::read(tree.path(&format!("{folder}/index.html"))).unwrap();
    files.push(("index.html".into(), page));
    files
}

/// The file whose name under `assets/` starts with `stem`, in `files`.
fn file<'a>(files: &'a [(String, Vec<u8>)], stem: &str) -> &'a (String, Vec<u8>) {
    files
        .iter()
        .find(|(name, _)| name.starts_with(&format!("assets/{stem}")))
        .unwrap_or_else(|| panic!("no {stem} in {files:?}"))
}

/// A program and two outputs of it, `one` and `two`, as two builds write them: the chunk's segments
/// in two orders, so two names for it from dx, and so two scripts and two names for those.
fn two_builds(tree: &Tree) -> String {
    let path = tree.path("program.wasm");
    std::fs::write(&path, program()).unwrap();
    let (data, elements) = (chunk_data(), chunk_entries());
    let reordered = chunk(
        &[data[2].clone(), data[0].clone(), data[1].clone()],
        &[
            elements[2].clone(),
            elements[0].clone(),
            elements[1].clone(),
        ],
    );
    dx_output(tree, "one", &split_main(), &chunk(&data, &elements), "aa11");
    dx_output(tree, "two", &split_main(), &reordered, "bb22");
    path.to_str().unwrap().to_string()
}

#[test]
fn two_builds_differing_in_chunk_order_and_dx_s_names_are_the_same_files_once_repaired() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    assert_ne!(files(&tree, "one"), files(&tree, "two"));
    let (code, report, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""), "{report}");
    let (code, _, err) = run(&[&program, tree.path("two").to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""));
    let repaired = files(&tree, "one");
    assert_eq!(repaired, files(&tree, "two"));

    // Each module and the script named by its bytes; the style sheet left as it was.
    let (main, empty, chunk, script) = (
        file(&repaired, "symphony-ui_bg-"),
        file(&repaired, "chunk_0_split-"),
        file(&repaired, "module_0_TasksKind-"),
        file(&repaired, "symphony-ui-"),
    );
    assert_eq!(repaired.len(), 6, "{repaired:?}");
    assert_eq!(file(&repaired, "board-dxh77.css").1, b"main{}");
    assert_eq!(empty.1, empty_chunk());
    for (name, bytes) in [main, empty, chunk, script] {
        let name = name.strip_prefix("assets/").unwrap();
        assert_eq!(name, named(name, bytes));
        assert!(!name.contains("-dxh"), "{name}");
    }
    // The script names the modules so, and the page the script.
    let text = String::from_utf8(script.1.clone()).unwrap();
    assert_eq!(
        text,
        format!(
            "const c=makeLoad(\"/./{}\",[],f);const a=makeLoad(\"/./{}\",[c],f);\
             __wbg_init({{module_or_path:\"/./{}\"}}).then(w=>w);",
            empty.0, chunk.0, main.0
        )
    );
    let page = String::from_utf8(repaired.last().unwrap().1.clone()).unwrap();
    assert_eq!(
        page.matches(&format!("\"/./{}\"", script.0)).count(),
        2,
        "{page}"
    );
    // The main module reads the template again; the chunk's segments are in order.
    assert_eq!(memory_of(&main.1), b"ok\0\0\0/{}\0");
    let data = chunk_data();
    assert_eq!(
        segments_of(&chunk.1, wasm::DATA),
        [data[1].clone(), data[0].clone(), data[2].clone()]
    );
    let short = |file: &(String, Vec<u8>)| file.0["assets/".len()..].to_string();
    assert_eq!(
        report,
        format!(
            "main module symphony-ui_bg-dxh1a2b.wasm -> {}: 1 merged constants written back, 3 \
             bytes\nchunk chunk_0_split-dxhc0.wasm -> {}: 0 of its 0 segments put in order, 0 \
             joined to the one before\nchunk module_0_TasksKind-dxhaa11.wasm -> {}: 5 of its 6 \
             segments put in order, 0 joined to the one before\n\
             script symphony-ui-dxhaa11.js -> {}\n",
            short(main),
            short(empty),
            short(chunk),
            short(script)
        )
    );
}

#[test]
fn a_second_run_changes_nothing() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    let public = tree.path("one");
    assert_eq!(run(&[&program, public.to_str().unwrap()]).0, 0);
    let first = files(&tree, "one");
    let (code, report, err) = run(&[&program, public.to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(files(&tree, "one"), first);
    let short = |stem| file(&first, stem).0["assets/".len()..].to_string();
    assert_eq!(
        report,
        format!(
            "main module {}: 0 merged constants written back, 0 bytes\n\
             chunk {}: 0 of its 0 segments put in order, 0 joined to the one before\n\
             chunk {}: 0 of its 6 segments put in order, 0 joined to the one before\n",
            short("symphony-ui_bg-"),
            short("chunk_0_split-"),
            short("module_0_TasksKind-")
        )
    );
}

#[test]
fn a_failed_check_writes_nothing_and_exits_1() {
    let tree = Tree::new();
    two_builds(&tree);
    // Another program: its data is not what the main module holds.
    let other = tree.path("other.wasm");
    std::fs::write(&other, program_of(b"OK@{}/{}!", &SYMBOLS)).unwrap();
    let before = files(&tree, "one");
    let (code, out, err) = run(&[other.to_str().unwrap(), tree.path("one").to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "symphony-build split-repair: symphony-ui_bg-dxh1a2b.wasm: the main module holds 0x6f at \
         0x400, where the program holds 0x4f: it is not that program's main module\n"
    );
    assert_eq!(files(&tree, "one"), before);
}

#[test]
fn an_output_without_a_chunk_or_with_two_main_modules_is_refused_and_left_as_it_was() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    let assets = tree.path("one/assets");
    std::fs::remove_file(assets.join("module_0_TasksKind-dxhaa11.wasm")).unwrap();
    std::fs::remove_file(assets.join("chunk_0_split-dxhc0.wasm")).unwrap();
    let before = files(&tree, "one");
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        format!(
            "symphony-build split-repair: {}: no chunk, a module that defines no memory: it is not the \
             output of dx bundle --wasm-split\n",
            assets.display()
        )
    );
    assert_eq!(files(&tree, "one"), before);
    std::fs::write(assets.join("another_bg-dxh9.wasm"), split_main()).unwrap();
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        err.ends_with(
            ": another_bg-dxh9.wasm and symphony-ui_bg-dxh1a2b.wasm both define a memory, where \
             one main module is expected\n"
        ),
        "{err}"
    );
}

#[test]
fn a_module_no_script_names_is_refused() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    tree.write(
        "one/assets/symphony-ui-dxhaa11.js",
        "makeLoad(\"/./assets/elsewhere.wasm\")",
    );
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build split-repair: symphony-ui_bg-dxh1a2b.wasm: no script in "),
        "{err}"
    );
}

#[test]
fn a_module_the_page_names_is_renamed_there_too() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    // A page that preloads the main module beside its script.
    tree.write(
        "one/index.html",
        "<link rel=\"preload\" as=\"fetch\" href=\"/./assets/symphony-ui_bg-dxh1a2b.wasm\">\
         <script type=\"module\" src=\"/./assets/symphony-ui-dxhaa11.js\"></script>",
    );
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""));
    let repaired = files(&tree, "one");
    let (main, script) = (
        file(&repaired, "symphony-ui_bg-"),
        file(&repaired, "symphony-ui-"),
    );
    assert!(!main.0.contains("dxh") && !script.0.contains("dxh"));
    let page = String::from_utf8(repaired.last().unwrap().1.clone()).unwrap();
    assert_eq!(
        page,
        format!(
            "<link rel=\"preload\" as=\"fetch\" href=\"/./{}\">\
             <script type=\"module\" src=\"/./{}\"></script>",
            main.0, script.0
        )
    );
    assert!(
        !tree.path("one/assets/symphony-ui_bg-dxh1a2b.wasm").exists(),
        "the old name is gone"
    );
}

#[test]
fn a_script_the_page_does_not_name_is_refused() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    tree.write("one/index.html", "<p>no script</p>");
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "symphony-build split-repair: symphony-ui-dxhaa11.js: index.html does not name it\n"
    );
}

// One chunk put in order as dx writes it (`--chunk`, wasm-opt.sh).

/// `bytes` written to `name` in `tree`, and its path as the command line names it.
fn module_at(tree: &Tree, name: &str, bytes: &[u8]) -> String {
    let path = tree.path(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn two_orders_of_one_chunk_are_the_same_bytes_once_each_is_put_in_order_in_place() {
    let tree = Tree::new();
    let (data, elements) = (chunk_data(), chunk_entries());
    let first = chunk(&data, &elements);
    let second = chunk(
        &[data[2].clone(), data[0].clone(), data[1].clone()],
        &[
            elements[1].clone(),
            elements[2].clone(),
            elements[0].clone(),
        ],
    );
    let (one, two) = (
        module_at(&tree, "one.wasm", &first),
        module_at(&tree, "two.wasm", &second),
    );
    let (code, report, err) = run(&["--chunk", &one]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        report,
        format!("chunk {one}: 5 of its 6 segments put in order, 0 joined to the one before\n")
    );
    assert_eq!(run(&["--chunk", &two]).0, 0);
    let ordered = std::fs::read(&one).unwrap();
    assert_eq!(ordered, std::fs::read(&two).unwrap());
    // The order the repair after dx gives it, so that one finds nothing left to do.
    assert_eq!(ordered, order::order(&first).unwrap().bytes);
    let (code, report, _) = run(&["--chunk", &one]);
    assert_eq!(code, 0);
    assert_eq!(
        report,
        format!("chunk {one}: 0 of its 6 segments put in order, 0 joined to the one before\n")
    );
    assert_eq!(std::fs::read(&one).unwrap(), ordered);
    // Nothing is left beside it.
    assert_eq!(tree.names(""), ["one.wasm", "two.wasm"]);
}

#[test]
fn a_chunk_s_touching_segments_are_joined_in_place_and_its_data_count_follows() {
    let tree = Tree::new();
    let data = [active(0x402, b"@{}"), active(0x400, b"ok")];
    let path = module_at(&tree, "chunk.wasm", &chunk(&data, &[]));
    let (code, report, err) = run(&["--chunk", &path]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        report,
        format!("chunk {path}: 2 of its 2 segments put in order, 1 joined to the one before\n")
    );
    let ordered = std::fs::read(&path).unwrap();
    assert_eq!(
        segments_of(&ordered, wasm::DATA),
        [active(0x400, b"ok@{}")]
    );
    assert_eq!(data_count(&ordered), 1);
}

#[test]
fn a_main_module_or_the_empty_shared_chunk_is_left_byte_for_byte() {
    let tree = Tree::new();
    let main = module_at(&tree, "main.wasm", &split_main());
    let (code, report, err) = run(&["--chunk", &main]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        report,
        format!("main module {main}: left as it is, for the repair after dx\n")
    );
    assert_eq!(std::fs::read(&main).unwrap(), split_main());
    let empty = module_at(&tree, "empty.wasm", &empty_chunk());
    let (code, report, _) = run(&["--chunk", &empty]);
    assert_eq!(code, 0);
    assert_eq!(
        report,
        format!("chunk {empty}: 0 of its 0 segments put in order, 0 joined to the one before\n")
    );
    assert_eq!(std::fs::read(&empty).unwrap(), empty_chunk());
}

#[test]
fn a_chunk_whose_order_matters_or_that_is_no_module_is_refused_and_left_as_it_was() {
    let tree = Tree::new();
    let unlike = chunk(&[active(0x405, b"/{}"), active(0x402, b"@{}/()")], &[]);
    let path = module_at(&tree, "unlike.wasm", &unlike);
    let (code, out, err) = run(&["--chunk", &path]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        format!(
            "symphony-build split-repair: {path}: its data segments 1 and 0 fill 0x406 with \
             different contents, so their order matters\n"
        )
    );
    assert_eq!(std::fs::read(&path).unwrap(), unlike);
    let text = module_at(&tree, "text.wasm", b"not wasm");
    let (code, _, err) = run(&["--chunk", &text]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with(&format!("symphony-build split-repair: {text}: ")),
        "{err}"
    );
    assert_eq!(std::fs::read(&text).unwrap(), b"not wasm");
    let (code, _, err) = run(&["--chunk", "/nowhere/chunk.wasm"]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build split-repair: /nowhere/chunk.wasm: "),
        "{err}"
    );
    assert_eq!(tree.names(""), ["text.wasm", "unlike.wasm"]);
}

#[test]
fn a_command_line_without_its_two_paths_is_its_usage_and_exit_2() {
    for args in [
        &[][..],
        &["program.wasm"],
        &["a", "b", "c"],
        &["--chunk"],
        &["--chunk", "a", "b"],
        &["--module", "a"],
    ] {
        assert_eq!(
            run(args),
            (2, String::new(), format!("{USAGE}\n")),
            "{args:?}"
        );
    }
    let (code, _, err) = run(&["/nowhere/program.wasm", "/nowhere/public"]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build split-repair: /nowhere/program.wasm: "),
        "{err}"
    );
}

// The format.

#[test]
fn numbers_read_to_their_bounds_and_no_further() {
    let unsigned = |bytes: &[u8]| wasm::Reader::new(bytes).u32();
    let signed = |bytes: &[u8]| wasm::Reader::new(bytes).i32();
    assert_eq!(unsigned(&[0xff, 0xff, 0xff, 0xff, 0x0f]), Ok(u32::MAX));
    assert_eq!(unsigned(&[0xe5, 0x8e, 0x26]), Ok(624_485));
    for beyond in [
        &[0xff, 0xff, 0xff, 0xff, 0x1f][..],
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0],
    ] {
        assert_eq!(
            unsigned(beyond),
            Err("the number at byte 0 is too large".into())
        );
    }
    assert_eq!(signed(&[0x80, 0x80, 0x80, 0x80, 0x78]), Ok(i32::MIN));
    assert_eq!(signed(&[0xff, 0xff, 0xff, 0xff, 0x07]), Ok(i32::MAX));
    assert_eq!(signed(&[0x7f]), Ok(-1));
    assert_eq!(
        signed(&[0x80, 0x80, 0x80, 0x80, 0x08]),
        Err("the number at byte 0 is too large".into())
    );
    assert_eq!(
        unsigned(&[0x80]),
        Err("it ends at byte 1 in the middle of a field".into())
    );
    for value in [0, 1, 63, 64, -64, -65, 0x400, i32::MAX, i32::MIN] {
        let mut out = Vec::new();
        write_i32(&mut out, value);
        let mut reader = wasm::Reader::new(&out);
        assert_eq!((reader.i32(), reader.done()), (Ok(value), true), "{value}");
    }
    for value in [0, 127, 128, u32::MAX] {
        let mut out = Vec::new();
        write_u32(&mut out, value);
        assert_eq!(unsigned(&out), Ok(value), "{value}");
    }
    let minus_one = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
    assert_eq!(wasm::Reader::new(&minus_one).i64(), Ok(-1));
}

#[test]
fn a_module_that_is_none_or_ends_early_is_refused() {
    let read = |bytes: &[u8]| wasm::Module::read(bytes).err();
    assert_eq!(
        read(b"\0asm\x02\0\0\0"),
        Some("it is not a WebAssembly module of version 1".into())
    );
    assert_eq!(
        read(b"\0asm\x01\0\0\0\x0b\x05\x01"),
        Some("a field of 5 bytes at byte 2 runs past its end".into())
    );
    // A section left as it was keeps the length as its writer encoded it, padded or not.
    let padded = b"\0asm\x01\0\0\0\x05\x83\x80\x80\x80\x00\x01\x00\x01".to_vec();
    assert_eq!(wasm::Module::read(&padded).unwrap().write(), padded);
    // A vector counting more items than its bytes could hold.
    assert_eq!(
        wasm::data_segments(&[0x05, 0x01]).err(),
        Some("the 5 items counted at byte 0 cannot fit in the bytes left".into())
    );
}

#[test]
fn segments_of_every_form_are_read_with_their_place_and_contents() {
    let mut body = Vec::new();
    write_u32(&mut body, 7);
    // Passive, by function index; active in table 2; declared; then by expression: active in
    // table 0, passive, active in table 1, declared.
    body.extend([1, 0x00, 1, 5]);
    body.extend([2, 2, 0x41, 3, 0x0b, 0x00, 2, 6, 7]);
    body.extend([3, 0x00, 1, 8]);
    body.extend([4, 0x41, 1, 0x0b, 2, 0xd2, 9, 0x0b, 0xd0, 0x70, 0x0b]);
    body.extend([5, 0x70, 1, 0xd2, 10, 0x0b]);
    body.extend([6, 1, 0x23, 0, 0x0b, 0x70, 0]);
    body.extend([7, 0x70, 1, 0xd2, 11, 0x0b]);
    let segments = wasm::element_segments(&body).unwrap();
    let places: Vec<(wasm::Mode, usize)> = segments
        .iter()
        .map(|segment| (segment.mode, segment.fill.len()))
        .collect();
    use wasm::Mode::{Active, Declared, Passive};
    assert_eq!(
        places,
        [
            (Passive, 1),
            (
                Active {
                    index: 2,
                    offset: Some(3)
                },
                2
            ),
            (Declared, 1),
            (
                Active {
                    index: 0,
                    offset: Some(1)
                },
                2
            ),
            (Passive, 1),
            (
                Active {
                    index: 1,
                    offset: None
                },
                0
            ),
            (Declared, 1),
        ]
    );
    let raw: Vec<u8> = segments
        .iter()
        .flat_map(|segment| segment.raw.to_vec())
        .collect();
    assert_eq!(raw, body[1..]);
    // A data segment in memory 1, and one of unknown flags.
    let data = wasm::data_segments(&[1, 2, 1, 0x41, 0x7f, 0x0b, 1, 9]).unwrap();
    assert_eq!(
        data[0].mode,
        Active {
            index: 1,
            offset: Some(-1)
        }
    );
    assert_eq!(
        wasm::data_segments(&[1, 3]).err(),
        Some("a data segment with unknown flags 3".into())
    );
    assert_eq!(
        wasm::element_segments(&[1, 0x00, 0x2a]).err(),
        Some("a constant expression at byte 2 holds an instruction it does not read, 0x2a".into())
    );
    assert_eq!(
        wasm::element_segments(&[1, 8]).err(),
        Some("an element segment with unknown flags 8".into())
    );
}

#[test]
fn a_module_defines_a_memory_when_its_memory_section_holds_one() {
    let defines = |sections: &[Built]| {
        wasm::Module::read(&module(sections))
            .unwrap()
            .defines_memory()
            .unwrap()
    };
    assert!(defines(&[memory()]));
    assert!(!defines(&[(5, vec![0])]));
    assert!(!defines(&[chunk_imports()]));
    assert!(!defines(&[custom("producers", b"")]));
}

// Names.

#[test]
fn a_file_is_named_by_its_stem_and_its_bytes_whatever_hash_it_was_named_by() {
    // The first 16 hex digits of the SHA-256 of `split`.
    let hash = "ad1a64057f9ab34f";
    for name in [
        "module_1_TasksKind-dxh7793f3478548eae.wasm",
        "module_1_TasksKind-ad1a64057f9ab34f.wasm",
        "module_1_TasksKind-0000000000000000.wasm",
        "module_1_TasksKind.wasm",
    ] {
        assert_eq!(
            named(name, b"split"),
            format!("module_1_TasksKind-{hash}.wasm"),
            "{name}"
        );
    }
    // A last part that is no hash stays part of the stem.
    assert_eq!(
        named("symphony-ui_bg.wasm", b"split"),
        format!("symphony-ui_bg-{hash}.wasm")
    );
    assert_eq!(named("a-dxh.wasm", b"split"), format!("a-dxh-{hash}.wasm"));
    assert_eq!(named("a-dxhzz.js", b"split"), format!("a-dxhzz-{hash}.js"));
    assert_eq!(named("bare", b"split"), format!("bare-{hash}"));
}

#[test]
fn only_a_whole_file_name_is_renamed() {
    let mut text = b"\"/a/x.wasm\" x.wasm.map ax.wasm x.wasm-2 'x.wasm'".to_vec();
    assert_eq!(rename_in(&mut text, "x.wasm", "y.wasm"), 2);
    assert_eq!(text, b"\"/a/y.wasm\" x.wasm.map ax.wasm x.wasm-2 'y.wasm'");
    assert_eq!(mentions(&text, "y.wasm"), 2);
    assert_eq!(rename_in(&mut text, "", "z"), 0);
    let mut whole = b"x.wasm".to_vec();
    assert_eq!(rename_in(&mut whole, "x.wasm", "y.wasm"), 1);
    assert_eq!(whole, b"y.wasm");
}
