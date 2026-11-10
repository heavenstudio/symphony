//! Private receipt record framing.
pub(super) fn fields<const N: usize>(raw: &[u8]) -> Result<[&[u8]; N], &'static str> {
    let mut split = raw.split(|b| *b == 0);
    let mut fields = [&[][..]; N];
    for field in &mut fields {
        *field = split.next().ok_or("partial framing")?;
    }
    if split.next() != Some(&[][..]) || split.next().is_some() {
        return Err("trailing or duplicate record");
    }
    Ok(fields)
}

fn text(raw: &[u8]) -> Result<&str, &'static str> {
    std::str::from_utf8(raw).map_err(|_| "unsupported input encoding")
}

pub(super) fn object(raw: &[u8]) -> bool {
    matches!(raw.len(), 40 | 64)
        && raw
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

pub(super) fn hash_record(
    raw: &[u8],
    op: &str,
    phase: usize,
    id: &str,
) -> Result<String, &'static str> {
    let f = fields::<10>(raw)?;
    if f[0] != b"IR1"
        || f[1] != b"H"
        || f[2] != op.as_bytes()
        || f[3] != phase.to_string().as_bytes()
        || f[4] != id.as_bytes()
        || f[9] != b"END"
        || !object(f[8])
    {
        return Err("hash association");
    }
    let prefix = text(f[5])?;
    if prefix.len() > 4096
        || !prefix.starts_with('/')
        || !prefix.ends_with('/')
        || prefix.contains(['\n', '\r'])
    {
        return Err("prefix framing");
    }
    let sources = text(f[6])?;
    let blobs = text(f[7])?;
    if sources.is_empty() || sources.ends_with('\n') || blobs.ends_with('\n') {
        return Err("source framing");
    }
    let mut blobs = blobs.split('\n');
    let mut previous: Option<&str> = None;
    for path in sources.split('\n') {
        if previous.is_some_and(|old| old.as_bytes() >= path.as_bytes()) {
            return Err("duplicate or unsorted source");
        }
        previous = Some(path);
        if !path.starts_with('/') || path.contains('\r') {
            return Err("unsupported source framing");
        }
        let blob = blobs.next().ok_or("missing blob")?;
        if !object(blob.as_bytes()) {
            return Err("blob framing");
        }
    }
    if blobs.next().is_some() {
        return Err("extra blob");
    }
    // Exact newline strings are retained, not claimed to encode filesystem names losslessly.
    Ok(text(&f[8][..16])?.to_owned())
}

pub(super) fn final_record(
    raw: &[u8],
    op: &str,
    id: &str,
) -> Result<(String, String), &'static str> {
    let f = fields::<7>(raw)?;
    if f[0] != b"IR1"
        || f[1] != b"F"
        || f[2] != op.as_bytes()
        || f[3] != id.as_bytes()
        || f[6] != b"END"
    {
        return Err("final association");
    }
    let key = text(f[4])?;
    let path = text(f[5])?;
    if path.len() > 4096 {
        return Err("final path limit");
    }
    let framed = format!("{path}\n");
    let (_, selected) =
        super::super::selected_path(framed.as_bytes()).map_err(|_| "public selector framing")?;
    if selected != key {
        return Err("public selector key");
    }
    Ok((selected, path.to_owned()))
}
