//! The little of WebAssembly's binary format the repair reads and writes
//! (webassembly.github.io/spec/core/binary): a module's sections, whether it defines a memory, its
//! data and element segments, and the data symbols of the linking section a program linked with
//! `--emit-relocs` keeps (github.com/WebAssembly/tool-conventions, Linking.md). Everything else in a
//! module is carried through as the bytes it was.

/// What went wrong reading a module, said in a sentence.
pub(super) type Result<T> = std::result::Result<T, String>;

/// A section's id: the custom sections, and those the repair reads or writes.
pub(super) const CUSTOM: u8 = 0;
pub(super) const MEMORY: u8 = 5;
pub(super) const ELEMENT: u8 = 9;
pub(super) const DATA: u8 = 11;
pub(super) const DATA_COUNT: u8 = 12;

const MAGIC: &[u8; 8] = b"\0asm\x01\0\0\0";

/// A cursor over a module's bytes.
pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(super) fn done(&self) -> bool {
        self.at == self.bytes.len()
    }

    pub(super) fn at(&self) -> usize {
        self.at
    }

    pub(super) fn byte(&mut self) -> Result<u8> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| format!("it ends at byte {} in the middle of a field", self.at))?;
        self.at += 1;
        Ok(byte)
    }

    pub(super) fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                format!(
                    "a field of {length} bytes at byte {} runs past its end",
                    self.at
                )
            })?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    /// An unsigned LEB128 number of at most `bits` bits.
    fn unsigned(&mut self, bits: u32) -> Result<u64> {
        let start = self.at;
        let mut value: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.byte()?;
            if shift >= bits || (shift > 0 && u64::from(byte & 0x7f) >> (bits - shift) != 0) {
                return Err(format!("the number at byte {start} is too large"));
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    /// A signed LEB128 number of at most `bits` bits.
    fn signed(&mut self, bits: u32) -> Result<i64> {
        let start = self.at;
        let mut value: i64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.byte()?;
            if shift >= bits {
                return Err(format!("the number at byte {start} is too large"));
            }
            value |= i64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && byte & 0x40 != 0 {
                    value |= -1 << shift;
                }
                if bits < 64 && !(-(1i64 << (bits - 1))..1i64 << (bits - 1)).contains(&value) {
                    return Err(format!("the number at byte {start} is too large"));
                }
                return Ok(value);
            }
        }
    }

    pub(super) fn u32(&mut self) -> Result<u32> {
        Ok(self.unsigned(32)? as u32)
    }

    pub(super) fn i32(&mut self) -> Result<i32> {
        Ok(self.signed(32)? as i32)
    }

    pub(super) fn i64(&mut self) -> Result<i64> {
        self.signed(64)
    }

    /// A vector's count of items, each of which takes a byte at least.
    pub(super) fn count(&mut self) -> Result<usize> {
        let at = self.at;
        let count = self.u32()? as usize;
        if count > self.bytes.len() - self.at {
            return Err(format!(
                "the {count} items counted at byte {at} cannot fit in the bytes left"
            ));
        }
        Ok(count)
    }

    pub(super) fn name(&mut self) -> Result<&'a str> {
        let length = self.u32()? as usize;
        let at = self.at;
        std::str::from_utf8(self.take(length)?)
            .map_err(|_| format!("the name at byte {at} is not UTF-8"))
    }
}

pub(super) fn write_u32(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

pub(super) fn write_i32(out: &mut Vec<u8>, value: i32) {
    let mut value = i64::from(value);
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0) {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// One section: its id and its body, the custom sections' name included.
#[derive(Clone)]
pub(super) struct Section {
    pub(super) id: u8,
    body: Vec<u8>,
    /// Its length as the module read it encoded, which [`Module::write`] writes again while the
    /// body is the one read (a tool may pad it), so a section left alone keeps every byte.
    length: Option<Vec<u8>>,
}

impl Section {
    pub(super) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(super) fn set_body(&mut self, body: Vec<u8>) {
        self.body = body;
        self.length = None;
    }

    /// A custom section's name and contents; `None` for any other section or a malformed name.
    pub(super) fn custom(&self) -> Option<(&str, &[u8])> {
        if self.id != CUSTOM {
            return None;
        }
        let mut reader = Reader::new(&self.body);
        let name = reader.name().ok()?;
        Some((name, &self.body[reader.at()..]))
    }
}

/// A module as its sections, in their order.
pub(super) struct Module {
    pub(super) sections: Vec<Section>,
}

impl Module {
    pub(super) fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err("it is not a WebAssembly module of version 1".into());
        }
        let mut reader = Reader::new(&bytes[MAGIC.len()..]);
        let mut sections = Vec::new();
        while !reader.done() {
            let id = reader.byte()?;
            let at = reader.at();
            let length = reader.u32()? as usize;
            let encoded = bytes[MAGIC.len() + at..MAGIC.len() + reader.at()].to_vec();
            let body = reader.take(length)?.to_vec();
            sections.push(Section {
                id,
                body,
                length: Some(encoded),
            });
        }
        Ok(Self { sections })
    }

    pub(super) fn write(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for section in &self.sections {
            out.push(section.id);
            match &section.length {
                Some(encoded) => out.extend_from_slice(encoded),
                None => write_u32(&mut out, section.body.len() as u32),
            }
            out.extend_from_slice(&section.body);
        }
        out
    }

    pub(super) fn section(&self, id: u8) -> Option<&Section> {
        self.sections.iter().find(|section| section.id == id)
    }

    pub(super) fn section_mut(&mut self, id: u8) -> Option<&mut Section> {
        self.sections.iter_mut().find(|section| section.id == id)
    }

    /// The contents of the custom section named `name`.
    pub(super) fn custom(&self, name: &str) -> Option<&[u8]> {
        self.sections
            .iter()
            .filter_map(Section::custom)
            .find(|(named, _)| *named == name)
            .map(|(_, contents)| contents)
    }

    /// Whether it defines a memory of its own, as a main module does and a chunk never.
    pub(super) fn defines_memory(&self) -> Result<bool> {
        let Some(section) = self.section(MEMORY) else {
            return Ok(false);
        };
        Ok(Reader::new(&section.body).count()? > 0)
    }
}

/// A reference type of one byte: `funcref` or `externref`, the only ones a split module holds.
fn reference_type(reader: &mut Reader) -> Result<u8> {
    match reader.byte()? {
        byte @ (0x70 | 0x6f) => Ok(byte),
        byte => Err(format!("a reference type it does not read, {byte:#04x}")),
    }
}

/// Where an active segment goes: a table's or a memory's index, and its offset when that is one
/// `i32.const` (`None` for any other constant expression).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Active { index: u32, offset: Option<i32> },
    Passive,
    Declared,
}

/// What a segment puts in place, one entry a slot: a data segment's bytes, an element segment's
/// items, each as it is encoded.
#[derive(Debug)]
pub(super) enum Fill<'a> {
    Bytes(&'a [u8]),
    Items(Vec<&'a [u8]>),
}

impl Fill<'_> {
    pub(super) fn len(&self) -> usize {
        match self {
            Fill::Bytes(bytes) => bytes.len(),
            Fill::Items(items) => items.len(),
        }
    }

    /// Whether slot `at` holds what `other`'s slot `other_at` holds.
    pub(super) fn same(&self, at: usize, other: &Fill, other_at: usize) -> bool {
        match (self, other) {
            (Fill::Bytes(one), Fill::Bytes(two)) => one[at] == two[other_at],
            (Fill::Items(one), Fill::Items(two)) => one[at] == two[other_at],
            _ => false,
        }
    }
}

/// One data or element segment.
#[derive(Debug)]
pub(super) struct Segment<'a> {
    /// Its whole encoding, as its section holds it.
    pub(super) raw: &'a [u8],
    pub(super) mode: Mode,
    pub(super) fill: Fill<'a>,
}

/// A constant expression's end, and its value when it is `i32.const N` alone.
fn constant(reader: &mut Reader) -> Result<Option<i32>> {
    let mut value = None;
    let mut instructions = 0;
    loop {
        let at = reader.at();
        match reader.byte()? {
            0x0b => return Ok(if instructions == 1 { value } else { None }),
            0x41 => value = Some(reader.i32()?),
            0x42 => {
                reader.i64()?;
            }
            0x43 => {
                reader.take(4)?;
            }
            0x44 => {
                reader.take(8)?;
            }
            // global.get, ref.func
            0x23 | 0xd2 => {
                reader.u32()?;
            }
            // ref.null
            0xd0 => {
                reference_type(reader)?;
            }
            // The extended constant expressions' arithmetic.
            0x6a | 0x6b | 0x6c | 0x7c | 0x7d | 0x7e => {}
            op => {
                return Err(format!(
                    "a constant expression at byte {at} holds an instruction it does not read, {op:#04x}"
                ));
            }
        }
        instructions += 1;
    }
}

/// The segments of a data section's body.
pub(super) fn data_segments(body: &[u8]) -> Result<Vec<Segment<'_>>> {
    let mut reader = Reader::new(body);
    let count = reader.count()?;
    let mut segments = Vec::with_capacity(count);
    for _ in 0..count {
        let start = reader.at();
        let mode = match reader.u32()? {
            0 => Mode::Active {
                index: 0,
                offset: constant(&mut reader)?,
            },
            1 => Mode::Passive,
            2 => Mode::Active {
                index: reader.u32()?,
                offset: constant(&mut reader)?,
            },
            flags => return Err(format!("a data segment with unknown flags {flags}")),
        };
        let length = reader.u32()? as usize;
        let bytes = reader.take(length)?;
        segments.push(Segment {
            raw: &body[start..reader.at()],
            mode,
            fill: Fill::Bytes(bytes),
        });
    }
    if !reader.done() {
        return Err("its data section holds more than its segments".into());
    }
    Ok(segments)
}

/// The segments of an element section's body.
pub(super) fn element_segments(body: &[u8]) -> Result<Vec<Segment<'_>>> {
    let mut reader = Reader::new(body);
    let count = reader.count()?;
    let mut segments = Vec::with_capacity(count);
    for _ in 0..count {
        let start = reader.at();
        let flags = reader.u32()?;
        if flags > 7 {
            return Err(format!("an element segment with unknown flags {flags}"));
        }
        // Bit 0: passive or declared, else active; bit 1: declared (when passive) or an explicit
        // table (when active); bit 2: items as expressions, else as function indices.
        let mode = if flags & 0b001 == 0 {
            let index = if flags & 0b010 != 0 { reader.u32()? } else { 0 };
            Mode::Active {
                index,
                offset: constant(&mut reader)?,
            }
        } else if flags & 0b010 != 0 {
            Mode::Declared
        } else {
            Mode::Passive
        };
        let expressions = flags & 0b100 != 0;
        // The element kind (0x00) or reference type, which only table 0's short forms leave out.
        if flags & 0b011 != 0 {
            if expressions {
                reference_type(&mut reader)?;
            } else if reader.byte()? != 0x00 {
                return Err("an element segment of an unknown element kind".into());
            }
        }
        let items = reader.count()?;
        let mut fill = Vec::with_capacity(items);
        for _ in 0..items {
            let item = reader.at();
            if expressions {
                constant(&mut reader)?;
            } else {
                reader.u32()?;
            }
            fill.push(&body[item..reader.at()]);
        }
        segments.push(Segment {
            raw: &body[start..reader.at()],
            mode,
            fill: Fill::Items(fill),
        });
    }
    if !reader.done() {
        return Err("its element section holds more than its segments".into());
    }
    Ok(segments)
}

/// A section body of `segments`, each as encoded, in their order.
pub(super) fn segments_body<'a>(segments: impl ExactSizeIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut body = Vec::new();
    write_u32(&mut body, segments.len() as u32);
    for raw in segments {
        body.extend_from_slice(raw);
    }
    body
}

/// A data symbol the linking section defines: its name, and the bytes it names, as its segment's
/// index, the offset in that segment and the length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DataSymbol {
    pub(super) name: String,
    pub(super) segment: u32,
    pub(super) offset: u32,
    pub(super) size: u32,
}

/// A symbol's flags: undefined (no segment, no offset), named though undefined, and absolute (an
/// address the linker defines, such as `__heap_base`, which names no segment's bytes).
const UNDEFINED: u32 = 0x10;
const EXPLICIT_NAME: u32 = 0x40;
const ABSOLUTE: u32 = 0x200;

/// The data symbols the module's linking section defines in its data segments, in its symbol
/// table's order; an error when it has no linking section.
pub(super) fn data_symbols(module: &Module) -> Result<Vec<DataSymbol>> {
    let linking = module.custom("linking").ok_or(
        "it has no linking section: the program is linked with --emit-relocs, as dx links it for --wasm-split",
    )?;
    let mut reader = Reader::new(linking);
    let version = reader.u32()?;
    if version != 2 {
        return Err(format!(
            "its linking section is of version {version}, not 2"
        ));
    }
    let mut symbols = Vec::new();
    while !reader.done() {
        let kind = reader.byte()?;
        let length = reader.u32()? as usize;
        let payload = reader.take(length)?;
        if kind != 8 {
            continue;
        }
        let mut table = Reader::new(payload);
        for _ in 0..table.count()? {
            let kind = table.byte()?;
            let flags = table.u32()?;
            let defined = flags & UNDEFINED == 0;
            match kind {
                // A function, a global, a tag, a table: an index, and a name when it has one.
                0 | 2 | 4 | 5 => {
                    table.u32()?;
                    if defined || flags & EXPLICIT_NAME != 0 {
                        table.name()?;
                    }
                }
                1 => {
                    let name = table.name()?.to_string();
                    if defined {
                        let symbol = DataSymbol {
                            name,
                            segment: table.u32()?,
                            offset: table.u32()?,
                            size: table.u32()?,
                        };
                        if flags & ABSOLUTE == 0 {
                            symbols.push(symbol);
                        }
                    }
                }
                3 => {
                    table.u32()?;
                }
                kind => return Err(format!("a symbol of unknown kind {kind}")),
            }
        }
        if !table.done() {
            return Err("its symbol table holds more than its symbols".into());
        }
    }
    Ok(symbols)
}

/// Whether the module's name section names its element or data segments, which a new order of
/// them would leave naming others.
pub(super) fn names_segments(module: &Module) -> Result<bool> {
    let Some(names) = module.custom("name") else {
        return Ok(false);
    };
    let mut reader = Reader::new(names);
    while !reader.done() {
        let kind = reader.byte()?;
        let length = reader.u32()? as usize;
        reader.take(length)?;
        if kind == 8 || kind == 9 {
            return Ok(true);
        }
    }
    Ok(false)
}
