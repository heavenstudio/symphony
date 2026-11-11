//! seed-clone SRC DST: DST, which must not exist, made a copy-on-write clone of folder SRC by one
//! clonefile(2) call, which APFS makes whole however many files SRC holds (0.6 s for the main
//! checkout's 12,000-file `.target/debug` at load 100, where `cp -c -R`, a clone call a file, took
//! 6.8 s). tools/seed-target.sh compiles it with rustc and runs it under the main checkout's build
//! lock, so that lock is held for a moment rather than for every file's clone. Links are cloned as
//! links, never followed. On failure it says why and exits 1, and the seed copies by `cp -c` instead.
//! macOS only.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::process::exit;

unsafe extern "C" {
    fn clonefile(src: *const c_char, dst: *const c_char, flags: u32) -> c_int;
}

/// clonefile's `CLONE_NOFOLLOW`: a link is cloned as itself.
const CLONE_NOFOLLOW: u32 = 1;

fn main() {
    let args: Vec<CString> = std::env::args_os()
        .skip(1)
        .filter_map(|arg| CString::new(arg.as_bytes()).ok())
        .collect();
    let [src, dst] = &args[..] else {
        eprintln!("usage: seed-clone SRC DST");
        exit(2);
    };
    // SAFETY: both are NUL-terminated paths that outlive the call.
    if unsafe { clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) } != 0 {
        eprintln!("seed-clone: {}", std::io::Error::last_os_error());
        exit(1);
    }
}
