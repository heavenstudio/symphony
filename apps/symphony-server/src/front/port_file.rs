//! The bound port's hint for local tools, written as soon as the listener is bound and cleared
//! only by the server that wrote it.

use std::path::Path;

/// The server's address for local tools that link to the app.
pub(crate) const PORT_FILE: &str = "artifacts/docs-app.port";

fn records(root: &Path, port: u16) -> bool {
    std::fs::read_to_string(root.join(PORT_FILE)).is_ok_and(|said| said.trim() == port.to_string())
}

pub(super) fn record_port(root: &Path, port: u16) {
    let path = root.join(PORT_FILE);
    if let Some(folder) = path.parent()
        && std::fs::create_dir_all(folder).is_ok()
    {
        let _ = std::fs::write(&path, format!("{port}\n"));
    }
}

/// Remove the port record, unless another app has written its own over it since.
pub(super) fn clear_port(root: &Path, port: u16) {
    if records(root, port) {
        let _ = std::fs::remove_file(root.join(PORT_FILE));
    }
}
