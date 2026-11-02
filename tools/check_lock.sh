#!/usr/bin/env bash
# Sourced by the content check and the staged docs check. Hold fd 9 through the build, then close
# it (never unlink the lock file). Cargo inherits it, so a stopped caller cannot free a live build's
# lock, and the kernel frees it when the last holder ends. A leftover file holds nothing by itself.
check_lock() {
	local folder="$1" label="$2" kind="$3" waiting=""
	local -a lock_command try_flags wait_command
	if command -v flock >/dev/null 2>&1; then
		lock_command=(flock)
		try_flags=(-n)
		wait_command=(flock 9)
	elif command -v lockf >/dev/null 2>&1; then
		lock_command=(lockf -s)
		try_flags=(-t 0)
		# macOS lockf always uses LOCK_NB on a descriptor, even while waiting: it spins.
		# File mode with -k blocks in the kernel without unlinking. It releases before
		# we take fd 9, so retry if another caller wins that gap; only fd 9 guards the build.
		wait_command=(lockf -s -k "$folder/lock" true)
	else
		printf "%s: neither flock nor lockf is installed to take the %s build folder's lock.\n" "$label" "$kind" >&2
		return 1
	fi
	mkdir -p "$folder" || return 1
	exec 9>>"$folder/lock" || return 1
	if [[ -n "${SF_TOOL_TEST_WAIT_HELPER:-}" ]]; then
		wait_command=("$SF_TOOL_TEST_WAIT_HELPER" --wait "lock $folder/lock, initial holder pid $(cat "$folder/lock" 2>/dev/null)" "${wait_command[@]}")
	fi
	while ! "${lock_command[@]}" "${try_flags[@]}" 9; do
		if [[ -z "$waiting" ]]; then
			printf '%s: waiting for the %s build folder %s, held by %s\n' \
				"$label" "$kind" "$folder" "$(cat "$folder/lock" 2>/dev/null)" >&2
			waiting=1
		fi
		if ! "${wait_command[@]}"; then
			exec 9>&-
			return 1
		fi
	done
	if ! printf '%s\n' "$$" >"$folder/lock"; then
		exec 9>&-
		return 1
	fi
}
