//! A descriptor no spawn or fork copies. A lock file a process holds across its spawns is then
//! held by that process alone: its release is the release, and no child carries the lock through
//! the moments before its exec. macOS alone has the flag ([`O_CLOFORK`]), and only its `open` sets
//! it; elsewhere the options are returned as they are.

use std::fs::OpenOptions;

/// `O_CLOFORK`, which the libc crate does not name for Apple (`sys/fcntl.h`). Only `open` sets it;
/// macOS's `F_SETFD` and `F_DUPFD_CLOFORK` do not.
#[cfg(target_vendor = "apple")]
pub(crate) const O_CLOFORK: libc::c_int = 0x0800_0000;

/// `options` set to open a descriptor no spawn or fork copies.
#[cfg(target_vendor = "apple")]
pub fn close_on_fork(options: &mut OpenOptions) -> &mut OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(O_CLOFORK)
}

/// `options` as they are: nothing is close-on-fork here.
#[cfg(not(target_vendor = "apple"))]
pub fn close_on_fork(options: &mut OpenOptions) -> &mut OpenOptions {
    options
}

#[cfg(test)]
mod tests {
    use super::close_on_fork;
    use std::fs::OpenOptions;

    #[test]
    fn a_close_on_fork_descriptor_opens_reads_and_writes_as_its_options_say() {
        let dir = std::env::temp_dir().join(format!(
            "close-on-fork-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        let file = close_on_fork(&mut options).open(&path).unwrap();
        file.try_lock().unwrap();
        std::fs::write(&path, "held").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "held");
        drop(file);
        let mut options = OpenOptions::new();
        options.write(true);
        assert!(close_on_fork(&mut options).open(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
