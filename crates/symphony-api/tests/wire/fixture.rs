//! The repository-path boundary used by wire tests and the verifier's preselection traversal.
use repository_read::Reader;
use std::io;
use std::path::Path;

pub(crate) fn read(reader: &Reader, root: &Path, name: &str) -> io::Result<String> {
    reader.read_to_string(
        &root
            .join("symphony/crates/symphony-api/fixtures")
            .join(name),
    )
}
