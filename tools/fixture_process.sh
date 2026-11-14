# Sourced by test fixtures before they start daemons. Call after this file has returned: the
# interpreter may have this source open above fd 2 while reading it. The Chrome driver runs a copy,
# crates/chrome-devtools/src/fixture_process.sh, which crates/symphony/tests/packages.rs holds equal.
fixture_close_fds() {
    # Enumerate this shell's open descriptors, not its descriptor limit. macOS exposes its
    # fdesc inventory at /dev/fd; Linux also provides /proc/self/fd. Snapshot the names before
    # closing them, including inherited descriptors above the shell's usual low numbers.
    fixture_fd_dir=/dev/fd
    if [ -d /proc/self/fd ]; then
        fixture_fd_dir=/proc/self/fd
    fi
    for fixture_fd in "$fixture_fd_dir"/*; do
        fixture_fd=${fixture_fd##*/}
        case "$fixture_fd" in
            0|1|2|*[!0-9]*) continue ;;
        esac
        eval "exec $fixture_fd>&-"
    done
}

# Read lifetime input in the watchdog process.
fixture_wait_eof() {
    while IFS= read -r fixture_line; do :; done
}
