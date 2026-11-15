//! The splitter's first fault: the merged constants it zeroes in the main module.
//!
//! For each data symbol only a chunk reaches, wasm-split-cli 0.7.10 writes zeros over its bytes in
//! the main module (`prune_main_symbols`), to be written back when that chunk loads. The linker
//! merges constants that end alike, so one symbol can lie over another's bytes (a `format!`
//! template `/{}` at the end of `@{}/{}`): when the chunk's symbol is zeroed, the main module's
//! reads zeros there until the chunk loads, silently. The splitter zeroes whole symbols and nothing
//! else, so a byte it zeroed that one symbol alone holds is that chunk's symbol's, which the main
//! module never reads; a byte two or more hold may be the main module's, and [`repair`] writes every
//! such byte back, as active data segments after the module's own. [`check`] fails a main module
//! that still reads zero where two symbols lie, or whose data is otherwise not the program's.

use super::Failure;
use super::wasm::{self, DATA, DATA_COUNT, DataSymbol, Fill, Mode, Module};

/// The program the linker made, before wasm-bindgen and the splitter: its data, placed.
pub(super) struct Program {
    segments: Vec<Placed>,
}

/// One of the program's data segments, at its address.
struct Placed {
    base: u32,
    bytes: Vec<u8>,
    /// How many of the program's data symbols lie over each of its bytes (at most 255).
    cover: Vec<u8>,
    /// The symbols in it.
    symbols: Vec<DataSymbol>,
}

impl Program {
    /// The program's data segments and the data symbols its linking section places in them.
    pub(super) fn read(bytes: &[u8]) -> wasm::Result<Self> {
        let module = Module::read(bytes)?;
        let symbols = wasm::data_symbols(&module)?;
        let data = module.section(DATA).ok_or("it has no data section")?;
        let mut segments = Vec::new();
        for (index, segment) in wasm::data_segments(data.body())?.into_iter().enumerate() {
            let (
                Mode::Active {
                    index: 0,
                    offset: Some(base),
                },
                Fill::Bytes(bytes),
            ) = (segment.mode, segment.fill)
            else {
                return Err(format!(
                    "its data segment {index} is not placed at a constant address of memory 0"
                ));
            };
            segments.push(Placed {
                base: base as u32,
                bytes: bytes.to_vec(),
                cover: vec![0; bytes.len()],
                symbols: Vec::new(),
            });
        }
        // A symbol of no bytes zeroes none, as the splitter reads it.
        for symbol in symbols.into_iter().filter(|symbol| symbol.size > 0) {
            let placed = segments.get_mut(symbol.segment as usize).ok_or_else(|| {
                format!(
                    "its symbol {} is in data segment {}, which it does not have",
                    symbol.name, symbol.segment
                )
            })?;
            let (start, end) = (
                symbol.offset as usize,
                symbol.offset as usize + symbol.size as usize,
            );
            if end > placed.bytes.len() {
                return Err(format!(
                    "its symbol {} runs past the end of its data segment",
                    symbol.name
                ));
            }
            for cover in &mut placed.cover[start..end] {
                *cover = cover.saturating_add(1);
            }
            placed.symbols.push(symbol);
        }
        Ok(Self { segments })
    }
}

/// A main module with the merged constants it needs written back.
pub(super) struct Repaired {
    pub(super) bytes: Vec<u8>,
    /// The runs of bytes written back, each a data segment of its own, and their bytes in all.
    pub(super) runs: usize,
    pub(super) written: usize,
}

/// `main`, the splitter's main module after wasm-opt, with every byte the splitter zeroed that two
/// or more of `program`'s data symbols lie over written back; then [`check`]ed.
pub(super) fn repair(program: &Program, main: &[u8]) -> Result<Repaired, Failure> {
    let mut module = Module::read(main).map_err(Failure::Input)?;
    let image = Image::of(&module)?;
    let mut runs: Vec<(u32, Vec<u8>)> = Vec::new();
    for placed in &program.segments {
        let seen = image.read(placed.base, placed.bytes.len());
        for (at, &was) in placed.bytes.iter().enumerate() {
            if was == 0 || seen[at] != 0 || placed.cover[at] < 2 {
                continue;
            }
            let address = placed.base + at as u32;
            match runs.last_mut() {
                Some((start, bytes)) if *start + bytes.len() as u32 == address => bytes.push(was),
                _ => runs.push((address, vec![was])),
            }
        }
    }
    if runs.is_empty() {
        check(program, main)?;
        return Ok(Repaired {
            bytes: main.to_vec(),
            runs: 0,
            written: 0,
        });
    }
    let data = module
        .section(DATA)
        .ok_or_else(|| Failure::Input("the main module has no data section".into()))?;
    let segments = wasm::data_segments(data.body()).map_err(Failure::Input)?;
    let mut appended = Vec::new();
    for (address, bytes) in &runs {
        // Active in memory 0 at `i32.const address`.
        appended.push(0);
        appended.push(0x41);
        wasm::write_i32(&mut appended, *address as i32);
        appended.push(0x0b);
        wasm::write_u32(&mut appended, bytes.len() as u32);
        appended.extend_from_slice(bytes);
    }
    let count = segments.len() + runs.len();
    let mut body = Vec::new();
    wasm::write_u32(&mut body, count as u32);
    for segment in &segments {
        body.extend_from_slice(segment.raw);
    }
    body.extend_from_slice(&appended);
    module
        .section_mut(DATA)
        .expect("the data section read above")
        .set_body(body);
    if let Some(data_count) = module.section_mut(DATA_COUNT) {
        let mut body = Vec::new();
        wasm::write_u32(&mut body, count as u32);
        data_count.set_body(body);
    }
    let bytes = module.write();
    check(program, &bytes)?;
    Ok(Repaired {
        written: runs.iter().map(|(_, bytes)| bytes.len()).sum(),
        runs: runs.len(),
        bytes,
    })
}

/// Fails `main` when it reads zero where two of `program`'s data symbols lie (a merged constant it
/// may need), where none lies, or over part of one symbol alone (the splitter zeroes a whole symbol
/// or none of it, and the repair rests on that), or holds anything else than the program's data.
pub(super) fn check(program: &Program, main: &[u8]) -> Result<(), Failure> {
    let module = Module::read(main).map_err(Failure::Input)?;
    let image = Image::of(&module)?;
    for placed in &program.segments {
        let seen = image.read(placed.base, placed.bytes.len());
        for (at, (&was, &is)) in placed.bytes.iter().zip(&seen).enumerate() {
            if was == is {
                continue;
            }
            let address = placed.base + at as u32;
            if is != 0 {
                return Err(Failure::Unsafe(format!(
                    "the main module holds {is:#04x} at {address:#x}, where the program holds \
                     {was:#04x}: it is not that program's main module"
                )));
            }
            match placed.cover[at] {
                0 => {
                    return Err(Failure::Unsafe(format!(
                        "the main module reads zero at {address:#x}, which no data symbol holds"
                    )));
                }
                1 => {}
                _ => {
                    return Err(Failure::Unsafe(format!(
                        "the main module reads zero at {address:#x}, where {} lie: a merged \
                         constant the splitter zeroed for a chunk, which the main module may read",
                        placed.names_at(at)
                    )));
                }
            }
        }
        for symbol in &placed.symbols {
            let range = symbol.offset as usize..(symbol.offset + symbol.size) as usize;
            let alone: Vec<usize> = range
                .filter(|&at| placed.cover[at] == 1 && placed.bytes[at] != 0)
                .collect();
            let zeroed = alone.iter().filter(|&&at| seen[at] == 0).count();
            if zeroed > 0 && zeroed < alone.len() {
                return Err(Failure::Unsafe(format!(
                    "the main module reads zero over part of {} at {:#x}: the splitter zeroes a \
                     whole symbol or none of it, and the repair rests on that",
                    symbol.name,
                    placed.base + symbol.offset
                )));
            }
        }
    }
    Ok(())
}

impl Placed {
    /// The names of the symbols that lie over byte `at`, in the symbol table's order.
    fn names_at(&self, at: usize) -> String {
        let at = at as u32;
        let names: Vec<&str> = self
            .symbols
            .iter()
            .filter(|symbol| symbol.offset <= at && at < symbol.offset + symbol.size)
            .map(|symbol| symbol.name.as_str())
            .collect();
        names.join(" and ")
    }
}

/// What a module's active data segments write into memory 0 as it is instantiated.
struct Image<'a> {
    segments: Vec<(u32, &'a [u8])>,
}

impl<'a> Image<'a> {
    fn of(module: &'a Module) -> Result<Self, Failure> {
        let Some(data) = module.section(DATA) else {
            return Ok(Self {
                segments: Vec::new(),
            });
        };
        let mut segments = Vec::new();
        for (index, segment) in wasm::data_segments(data.body())
            .map_err(Failure::Input)?
            .into_iter()
            .enumerate()
        {
            match (segment.mode, segment.fill) {
                (
                    Mode::Active {
                        index: 0,
                        offset: Some(base),
                    },
                    Fill::Bytes(bytes),
                ) => segments.push((base as u32, bytes)),
                _ => {
                    return Err(Failure::Input(format!(
                        "the main module's data segment {index} is not placed at a constant \
                         address of memory 0, which the repair does not read"
                    )));
                }
            }
        }
        Ok(Self { segments })
    }

    /// The `length` bytes at `base` once every segment is written, in their order.
    fn read(&self, base: u32, length: usize) -> Vec<u8> {
        let (base, end) = (u64::from(base), u64::from(base) + length as u64);
        let mut seen = vec![0; length];
        for &(start, bytes) in &self.segments {
            let (start, stop) = (u64::from(start), u64::from(start) + bytes.len() as u64);
            let (from, to) = (start.max(base), stop.min(end));
            if from < to {
                seen[(from - base) as usize..(to - base) as usize]
                    .copy_from_slice(&bytes[(from - start) as usize..(to - start) as usize]);
            }
        }
        seen
    }
}
