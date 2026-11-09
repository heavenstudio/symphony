//! `symphony publish-client STAGE PUBLIC`: a fully assembled client put in place of the one served,
//! as a reload publishes its build: one rename, or one exchange of the folders over a client there.

use std::io::Write;
use std::path::Path;

/// Publish a fully assembled client with one rename, including over a nonempty directory.
pub(super) fn publish_client_main(args: &[String], err: &mut dyn Write) -> i32 {
    let [stage, public] = args else {
        let _ = writeln!(err, "usage: symphony publish-client STAGE PUBLIC");
        return 2;
    };
    match publish_client(Path::new(stage), Path::new(public)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(err, "symphony publish-client: {stage} -> {public}: {error}");
            1
        }
    }
}

pub(crate) fn publish_client(stage: &Path, public: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    if !public.try_exists()? {
        return std::fs::rename(stage, public);
    }
    let stage = CString::new(stage.as_os_str().as_bytes())?;
    let public = CString::new(public.as_os_str().as_bytes())?;
    // SAFETY: both paths are live, NUL-terminated strings; the kernel atomically exchanges their
    // directory entries. No file descriptors or pointers outlive this call.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(stage.as_ptr(), public.as_ptr(), libc::RENAME_SWAP) };
    // SAFETY: both paths are live, NUL-terminated strings, relative to the current directory for
    // AT_FDCWD. RENAME_EXCHANGE atomically swaps the entries without exposing a missing directory.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            stage.as_ptr(),
            libc::AT_FDCWD,
            public.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic client directory exchange requires macOS or Linux",
    ));
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
