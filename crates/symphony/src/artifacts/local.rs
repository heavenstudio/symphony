use super::model::{hex, random};
use super::{ArtifactId, Failure, Generation, Provider, Scope};
use crate::{project::Project, uploads};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Private local disk adapter. An acknowledged write survives its process; it makes no promise
/// about losing this disk or container. Legacy `uploads::store`, paths and calls are untouched.
/// All cooperating writers use a permanent per-object OS lock (released on process exit), never
/// a shared-service lock. The configured artifact directory must be private to the host.
pub struct Local {
    root: PathBuf,
}

impl Local {
    /// Resolve through the project's configured artifacts folder. No directory is made until used.
    pub fn for_project(root: &Path) -> Self {
        Self {
            root: root.join(Project::of(root).artifact("durable")),
        }
    }

    fn folder(&self, scope: &Scope) -> PathBuf {
        self.root.join(scope.key())
    }

    fn locked(&self, scope: &Scope, id: &ArtifactId) -> Result<(File, PathBuf), Failure> {
        if !hex(id.as_str(), 64) {
            return Err(Failure::Forbidden);
        }
        let folder = self.folder(scope);
        fs::create_dir_all(&folder).map_err(io_failure)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(folder.join(format!("{}.lock", id.as_str())))
            .map_err(io_failure)?;
        lock.lock().map_err(io_failure)?;
        Ok((lock, folder.join(id.as_str())))
    }
}

fn io_failure(error: std::io::Error) -> Failure {
    match error.kind() {
        std::io::ErrorKind::NotFound => Failure::Missing,
        std::io::ErrorKind::PermissionDenied => Failure::Forbidden,
        _ => Failure::Unavailable,
    }
}

fn open(path: &Path) -> Result<File, Failure> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(io_failure)?;
    if !file.metadata().map_err(io_failure)?.is_file() {
        return Err(Failure::Integrity);
    }
    Ok(file)
}

fn header(file: &mut File) -> Result<Generation, Failure> {
    let mut header = [0; 65];
    file.read_exact(&mut header).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Failure::Integrity
        } else {
            io_failure(error)
        }
    })?;
    let text = std::str::from_utf8(&header[..64]).map_err(|_error| Failure::Integrity)?;
    if header[64] != b'\n' || !hex(text, 64) {
        return Err(Failure::Integrity);
    }
    Generation::new(text)
}

fn sync_parent(path: &Path) -> Result<(), Failure> {
    File::open(path.parent().ok_or(Failure::Unavailable)?)
        .and_then(|folder| folder.sync_all())
        .map_err(io_failure)
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Provider for Local {
    fn binding(&self) -> &str {
        "symphony-local-v1"
    }

    fn put(&self, scope: &Scope, id: &ArtifactId, bytes: &[u8]) -> Result<Generation, Failure> {
        if bytes.is_empty() || bytes.len() > uploads::MAX_BYTES {
            return Err(Failure::Integrity);
        }
        let (_lock, path) = self.locked(scope, id)?;
        match open(&path) {
            Ok(mut existing) => return header(&mut existing),
            Err(Failure::Missing) => {}
            Err(error) => return Err(error),
        }
        let generation = Generation::new(&random().map_err(|_error| Failure::Unavailable)?)?;
        let pending = Pending(path.with_extension(format!("{}.part", generation.as_str())));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&pending.0)
            .map_err(io_failure)?;
        file.write_all(generation.as_str().as_bytes())
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.write_all(bytes))
            .and_then(|()| file.sync_all())
            .map_err(io_failure)?;
        fs::hard_link(&pending.0, &path).map_err(io_failure)?;
        // If this sync's acknowledgement is lost, inspect/read reconcile the same object.
        sync_parent(&path)?;
        Ok(generation)
    }

    fn inspect(&self, scope: &Scope, id: &ArtifactId) -> Result<Generation, Failure> {
        let (_lock, path) = self.locked(scope, id)?;
        header(&mut open(&path)?)
    }

    fn read(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<Vec<u8>, Failure> {
        let (_lock, path) = self.locked(scope, id)?;
        let mut file = open(&path)?;
        if header(&mut file)? != *generation {
            return Err(Failure::GenerationMismatch);
        }
        let mut bytes = Vec::new();
        file.take(uploads::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_failure)?;
        if bytes.len() > uploads::MAX_BYTES {
            return Err(Failure::Integrity);
        }
        Ok(bytes)
    }

    fn delete(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<(), Failure> {
        let (_lock, path) = self.locked(scope, id)?;
        if header(&mut open(&path)?)? != *generation {
            return Err(Failure::GenerationMismatch);
        }
        fs::remove_file(&path).map_err(io_failure)?;
        sync_parent(&path)
    }
}
