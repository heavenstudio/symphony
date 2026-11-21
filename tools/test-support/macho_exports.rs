//! The symbols a Mach-O executable exports, read from its export trie, the names dyld
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

/// The image's own header, which a link with `-no_exported_symbols` by a linker before macOS 26's
/// still names, alone: no symbol of the program's.
const HEADER: &str = "__mh_execute_header";

/// The symbols `binary` exports beyond its own header ([`HEADER`]), in the trie's order. A link
/// with `-no_exported_symbols` leaves none: no trie, its root alone, or the header's one name.
pub(crate) fn exports(binary: &[u8]) -> Vec<String> {
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
        return Vec::new();
    };
    let mut names = names(&binary[offset..offset + size]);
    names.retain(|name| name != HEADER);
    names
}

/// What a failed test says of `exports`: how many, and the first of them.
pub(crate) fn said(exports: &[String]) -> String {
    let first: Vec<&str> = exports.iter().take(8).map(String::as_str).collect();
    format!("{} exported, the first {first:?}", exports.len())
}

/// Every name an export trie holds. A node is the size of what it says of a symbol (ULEB128; not
/// 0 where a name ends there), that much of it, a count of children in one byte, and for each
/// child the bytes its name adds, a NUL and the child's offset in the trie (ULEB128).
fn names(trie: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    // Each node once: a well-formed trie is a tree, and a damaged one must still end.
    let mut seen = vec![false; trie.len()];
    let mut nodes = vec![(0, Vec::new())];
    while let Some((mut at, name)) = nodes.pop() {
        if at >= trie.len() || std::mem::replace(&mut seen[at], true) {
            continue;
        }
        let terminal = uleb(trie, &mut at);
        if terminal != 0 {
            found.push(String::from_utf8_lossy(&name).into_owned());
        }
        at = at.saturating_add(terminal);
        let Some(&children) = trie.get(at) else {
            continue;
        };
        at += 1;
        let mut below = Vec::new();
        for _ in 0..children {
            let Some(end) = trie
                .get(at..)
                .and_then(|rest| rest.iter().position(|&b| b == 0))
            else {
                break;
            };
            let mut child = name.clone();
            child.extend_from_slice(&trie[at..at + end]);
            at += end + 1;
            below.push((uleb(trie, &mut at), child));
        }
        // Popped last in, first out: reversed, the children are read in the trie's order.
        nodes.extend(below.into_iter().rev());
    }
    found
}

/// The ULEB128 number at `at`, which is moved past it; what is cut short reads as far as it goes.
fn uleb(bytes: &[u8], at: &mut usize) -> usize {
    let (mut value, mut shift) = (0usize, 0u32);
    while let Some(&byte) = bytes.get(*at) {
        *at += 1;
        value |= usize::from(byte & 0x7f).checked_shl(shift).unwrap_or(0);
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
    }
    value
}

#[test]
fn a_tries_names_are_read_whole_and_the_header_alone_is_no_export() {
    // No trie's bytes, and a root with nothing at it and no child, padded.
    assert_eq!(names(&[]), Vec::<String>::new());
    assert_eq!(names(&[0; 8]), Vec::<String>::new());
    // `_`, then `_mh_execute_header` (a symbol at offset 0) and `main` (one at 0x328, a two-byte
    // number), as a link writes the two.
    let mut trie = vec![0, 1, b'_', 0, 5];
    trie.extend([0, 2]);
    trie.extend(b"_mh_execute_header\0");
    let header_at = trie.len();
    trie.extend([0]);
    trie.extend(b"main\0");
    let main_at = trie.len();
    trie.extend([0]);
    let header = trie.len();
    trie.extend([2, 0, 0, 0]);
    let main = trie.len();
    trie.extend([3, 0, 0xa8, 0x06, 0]);
    trie[header_at] = u8::try_from(header).unwrap();
    trie[main_at] = u8::try_from(main).unwrap();
    assert_eq!(names(&trie), ["__mh_execute_header", "_main"]);
    // A child that points back at the root ends, as does one past the end and a name cut short.
    assert_eq!(names(&[0, 1, b'a', 0, 0]), Vec::<String>::new());
    assert_eq!(names(&[0, 1, b'a', 0, 9]), Vec::<String>::new());
    assert_eq!(names(&[0, 2, b'a', 0, 7, b'b', b'c', 1, 0, 0]), ["a"]);
    let (mut at, long) = (0, [0xa8, 0x06, 0x7f]);
    assert_eq!((uleb(&long, &mut at), at), (0x328, 2));
    assert_eq!(
        said(&["_a".into(), "_b".into()]),
        r#"2 exported, the first ["_a", "_b"]"#
    );
}
