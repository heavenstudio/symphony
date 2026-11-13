//! Kernel observations of the recorded browser group. No process-name search or wall clock.

use std::collections::HashMap;
use std::io;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct Member {
    pub pid: u32,
    // Read by the diagnostics, which print a whole member, and by the tests.
    #[cfg_attr(not(test), expect(dead_code, reason = "printed by the diagnostics"))]
    pub parent: u32,
    #[cfg_attr(not(test), expect(dead_code, reason = "printed by the diagnostics"))]
    pub state: u32,
    pub cpu: Duration,
}

/// Charge only CPU observed in the owned processes, retaining work when helpers exit.
/// A sleeping or descheduled browser does not spend its wait's budget.
pub(super) struct Budget {
    readings: HashMap<u32, Duration>,
    used: Duration,
    limit: Duration,
}

impl Budget {
    pub fn new(limit: Duration, members: &[Member]) -> Self {
        Self {
            readings: members.iter().map(|m| (m.pid, m.cpu)).collect(),
            used: Duration::ZERO,
            limit,
        }
    }

    pub fn exhausted(&mut self, members: &[Member]) -> bool {
        for member in members {
            let before = self.readings.entry(member.pid).or_default();
            self.used += member.cpu.saturating_sub(*before);
            *before = (*before).max(member.cpu);
        }
        self.used >= self.limit
    }

    pub fn used(&self) -> Duration {
        self.used
    }
}

#[cfg(target_os = "macos")]
pub(super) fn members(group: u32) -> io::Result<Vec<Member>> {
    // sys/proc_info.h; libc exposes proc_listpids but not its group selector.
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = vec![0i32; 64];
    loop {
        // SAFETY: libproc writes at most the supplied buffer size, selecting only the
        // process group whose positive child PID was recorded at launch.
        let bytes = unsafe {
            libc::proc_listpids(
                PROC_PGRP_ONLY,
                group,
                pids.as_mut_ptr().cast(),
                i32::try_from(pids.len() * size_of::<i32>()).unwrap(),
            )
        };
        if bytes < 0 {
            return Err(io::Error::last_os_error());
        }
        let count = usize::try_from(bytes).unwrap() / size_of::<i32>();
        if count == pids.len() {
            pids.resize(pids.len() * 2, 0);
            continue;
        }
        pids.truncate(count);
        break;
    }
    let mut found = Vec::new();
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        // SAFETY: both zeroed structs are valid output buffers of their named libproc
        // flavours. A process exiting during the snapshot is omitted, never another PID.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let mut usage: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
        let read = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                std::ptr::from_mut(&mut info).cast(),
                i32::try_from(size_of::<libc::proc_bsdinfo>()).unwrap(),
            )
        };
        if read != i32::try_from(size_of::<libc::proc_bsdinfo>()).unwrap() || info.pbi_pgid != group
        {
            continue;
        }
        let read = unsafe {
            libc::proc_pid_rusage(
                pid,
                libc::RUSAGE_INFO_V0,
                std::ptr::from_mut(&mut usage).cast(),
            )
        };
        // Zombies have no CPU left to consume, and libproc may refuse their usage.
        if read != 0 && info.pbi_status != 5 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(error);
        }
        // XNU fill_task_rusage copies task_power_info's mach absolute CPU ticks.
        let nanos = cpu_nanos(usage.ri_user_time + usage.ri_system_time)?;
        found.push(Member {
            pid: info.pbi_pid,
            parent: info.pbi_ppid,
            state: info.pbi_status,
            cpu: Duration::from_nanos(nanos),
        });
    }
    found.sort_by_key(|m| m.pid);
    Ok(found)
}

#[cfg(target_os = "macos")]
#[expect(
    deprecated,
    reason = "libc's mach timebase bindings match the existing native CPU reader"
)]
fn cpu_nanos(ticks: u64) -> io::Result<u64> {
    let mut base = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: the call writes exactly the supplied timebase struct.
    let status = unsafe { libc::mach_timebase_info(&mut base) };
    if status != 0 || base.denom == 0 {
        return Err(io::Error::other(format!(
            "mach_timebase_info failed: {status}"
        )));
    }
    let nanos = u128::from(ticks) * u128::from(base.numer) / u128::from(base.denom);
    u64::try_from(nanos).map_err(io::Error::other)
}

#[cfg(target_os = "linux")]
pub(super) fn members(group: u32) -> io::Result<Vec<Member>> {
    // SAFETY: sysconf only reads the kernel's clock tick frequency.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let ticks = u64::try_from(ticks).map_err(io::Error::other)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let text = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if let Some(member) = stat_member(pid, &text, group, ticks) {
            found.push(member);
        }
    }
    found.sort_by_key(|m| m.pid);
    Ok(found)
}

#[cfg(any(target_os = "linux", test))]
fn stat_member(pid: u32, text: &str, group: u32, ticks: u64) -> Option<Member> {
    let (_, fields) = text.rsplit_once(')')?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    if fields.get(2)?.parse::<u32>().ok()? != group {
        return None;
    }
    let user: u64 = fields.get(11)?.parse().ok()?;
    let system: u64 = fields.get(12)?.parse().ok()?;
    Some(Member {
        pid,
        parent: fields.get(1)?.parse().ok()?,
        state: u32::from(*fields.first()?.as_bytes().first()?),
        cpu: Duration::from_secs_f64((user + system) as f64 / ticks as f64),
    })
}

#[test]
fn budget_counts_work_across_helpers_but_never_waiting_or_unrelated_cpu() {
    let member = |pid, millis| Member {
        pid,
        parent: 1,
        state: 1,
        cpu: Duration::from_millis(millis),
    };
    let mut budget = Budget::new(Duration::from_millis(50), &[member(10, 100)]);
    for _ in 0..100 {
        assert!(!budget.exhausted(&[member(10, 100)]));
    }
    assert!(!budget.exhausted(&[]));
    assert!(!budget.exhausted(&[member(10, 0)]));
    assert!(!budget.exhausted(&[member(10, 100)]));
    assert!(!budget.exhausted(&[member(10, 120), member(11, 10)]));
    assert!(!budget.exhausted(&[member(11, 20)]));
    assert!(budget.exhausted(&[member(11, 30)]));
    assert_eq!(budget.used(), Duration::from_millis(50));
}

#[test]
fn linux_snapshot_selects_the_owned_group_and_reads_past_parentheses() {
    let stat = "10 (chrome (helper)) Z 1 10 10 0 0 0 0 0 0 0 25 5 900 800";
    let member = stat_member(10, stat, 10, 100).unwrap();
    assert_eq!((member.pid, member.parent, member.state), (10, 1, 90));
    assert_eq!(member.cpu, Duration::from_millis(300));
    assert!(stat_member(10, stat, 11, 100).is_none());
    assert!(stat_member(10, "malformed", 10, 100).is_none());
}

#[test]
fn kernel_snapshot_tracks_only_the_owned_group_and_cpu_advances_twice() {
    use std::io::{BufRead, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut child = super::BrowserGroup {
        child: Command::new("/bin/sh")
            .args([
                "-c",
                r#"printf 'ready\n'
while read -r line; do
    i=0
    while test "$i" -lt 50000; do i=$((i+1)); done
    printf 'done\n'
done"#,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap(),
        done: false,
    };
    let group = child.child.id();
    let mut reader = std::io::BufReader::new(child.child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "ready\n");
    let sample = || {
        let observed = members(group).unwrap();
        assert_eq!(observed.len(), 1, "{observed:?}");
        assert_eq!(observed[0].pid, group);
        assert_eq!(observed[0].parent, std::process::id());
        observed[0].cpu
    };
    let mut previous = sample();
    for _ in 0..2 {
        writeln!(child.child.stdin.as_mut().unwrap(), "spin").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "done\n");
        let now = sample();
        assert!(
            now > previous,
            "kernel CPU did not advance: {previous:?} -> {now:?}"
        );
        eprintln!("owned group={group} CPU {previous:?} -> {now:?}");
        previous = now;
    }
    drop(child.child.stdin.take());
    assert!(child.reap().unwrap().success());
    assert!(members(group).unwrap().is_empty());
}
