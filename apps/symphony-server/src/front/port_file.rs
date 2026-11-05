//! The bound port's hint for local tools, written as soon as the listener is bound and cleared
//! only by the server that wrote it.

use std::path::{Path, PathBuf};

/// The server's address for local tools that link to the app, in the project's artifacts folder.
const PORT_FILE: &str = "docs-app.port";

/// The port record of the project at `root`.
pub(crate) fn port_file(root: &Path) -> PathBuf {
    root.join(symphony::project::Project::of(root).artifact(PORT_FILE))
}

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
