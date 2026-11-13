//! Whether a Mach-O executable exports any symbol, read from its export trie, the names dyld
//! could look up in it: the trie its `LC_DYLD_INFO_ONLY` or its `LC_DYLD_EXPORTS_TRIE` points at.
//! The `links` tests of the shipped binaries' packages read it to hold each binary to the
//! `-no_exported_symbols` its package's build script links it with (docs/CHECKS.md, "Size budgets").

/// `LC_DYLD_INFO_ONLY`: the export trie's offset and size are the command's eleventh and twelfth
/// words.
const DYLD_INFO_ONLY: u32 = 0x8000_0022;
/// `LC_DYLD_EXPORTS_TRIE`: a linkedit data command, its offset and size the third and fourth words.
const DYLD_EXPORTS_TRIE: u32 = 0x8000_0033;
/// A 64-bit Mach-O's first word, little-endian.
const MAGIC_64: u32 = 0xfeed_facf;

/// Whether `binary` exports a symbol. A trie that names none is absent, or is its root alone: a
/// node with no terminal (its size 0) and no child (its count 0), padded, as a link with
/// `-no_exported_symbols` writes it.
pub(crate) fn exports_any(binary: &[u8]) -> bool {
    let word = |at: usize| u32::from_le_bytes(binary[at..at + 4].try_into().unwrap());
    let at_word = |at: usize| usize::try_from(word(at)).unwrap();
    assert_eq!(word(0), MAGIC_64, "not a 64-bit Mach-O");
    // The commands follow the 32-byte header, whose fifth word counts them.
    let mut command = 32;
    let mut trie = None;
    for _ in 0..word(16) {
        match word(command) {
            DYLD_INFO_ONLY => trie = Some((at_word(command + 40), at_word(command + 44))),
            DYLD_EXPORTS_TRIE => trie = Some((at_word(command + 8), at_word(command + 12))),
            _ => {}
        }
        command += at_word(command + 4);
    }
    let Some((offset, size)) = trie else {
        return false;
    };
    let root = &binary[offset..offset + size];
    !(root.is_empty() || root.starts_with(&[0, 0]))
}
