//! The bound port's hint for local tools, written as soon as the listener is bound and cleared
//! only by the server that wrote it.

use std::path::Path;

/// The port record of the project at `root`, in its artifacts folder.
pub(crate) use symphony::triage::port_file;

fn records(root: &Path, port: u16) -> bool {
    std::fs::read_to_string(port_file(root)).is_ok_and(|said| said.trim() == port.to_string())
}

pub(super) fn record_port(root: &Path, port: u16) {
    let path = port_file(root);
    if let Some(folder) = path.parent()
        && std::fs::create_dir_all(folder).is_ok()
    {
        let _ = std::fs::write(&path, format!("{port}\n"));
    }
}

/// Remove the port record, unless another app has written its own over it since.
pub(super) fn clear_port(root: &Path, port: u16) {
    if records(root, port) {
        let _ = std::fs::remove_file(port_file(root));
    }
}
