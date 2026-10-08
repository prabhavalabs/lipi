//! Resource governor: process priority and admission control.
//!
//! lipi never lets OCR starve the machine:
//!
//! * the process (and the OCR processes it starts) runs at reduced priority for the profile;
//!   on macOS the `gentle` profile also asks for background QoS, which keeps work on the
//!   efficiency cores;
//! * every OCR process is single-threaded;
//! * before each job the governor waits until enough memory is available and the operating
//!   system does not report memory pressure.

use crate::hardware::Hardware;
use crate::profile::Profile;
use std::process::Command;
use std::time::Duration;
use sysinfo::System;

/// Apply the profile's process priority to the current process. Child processes inherit it.
pub fn apply_priority(profile: Profile) {
    #[cfg(unix)]
    unsafe {
        let nice = profile.niceness();
        if nice > 0 {
            libc::setpriority(libc::PRIO_PROCESS, 0, nice);
        }
        #[cfg(target_os = "macos")]
        if profile == Profile::Gentle {
            // PRIO_DARWIN_PROCESS (4) with PRIO_DARWIN_BG (0x1000): background QoS.
            libc::setpriority(4, 0, 0x1000);
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, IDLE_PRIORITY_CLASS, SetPriorityClass,
        };
        let class = match profile {
            Profile::Gentle => IDLE_PRIORITY_CLASS,
            Profile::Balanced => BELOW_NORMAL_PRIORITY_CLASS,
            Profile::Max => return,
        };
        SetPriorityClass(GetCurrentProcess(), class);
    }
}

/// Configure a command for an OCR worker process: single-threaded, background QoS on macOS
/// when the profile is `gentle`.
pub fn worker_command(program: &std::path::Path, profile: Profile) -> Command {
    let mut cmd = if cfg!(target_os = "macos")
        && profile == Profile::Gentle
        && std::path::Path::new("/usr/sbin/taskpolicy").exists()
    {
        let mut c = Command::new("/usr/sbin/taskpolicy");
        c.arg("-b").arg(program);
        c
    } else {
        Command::new(program)
    };
    cmd.env("OMP_THREAD_LIMIT", "1").env("OMP_NUM_THREADS", "1");
    cmd
}

/// Operating-system memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    /// No pressure.
    Normal,
    /// The system is compressing or swapping.
    Warn,
    /// The system is close to running out of memory.
    Critical,
}

/// Read the operating system's memory-pressure signal where one exists.
pub fn memory_pressure() -> Pressure {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("sysctl").args(["-n", "kern.memorystatus_vm_pressure_level"]).output() {
            return match String::from_utf8_lossy(&out.stdout).trim() {
                "4" => Pressure::Critical,
                "2" => Pressure::Warn,
                _ => Pressure::Normal,
            };
        }
    }
    #[cfg(target_os = "linux")]
    {
        // PSI: "some avg10=1.23 avg60=... " — share of time tasks stalled on memory.
        if let Ok(s) = std::fs::read_to_string("/proc/pressure/memory") {
            let avg10 = s
                .lines()
                .find(|l| l.starts_with("some"))
                .and_then(|l| l.split_whitespace().find(|t| t.starts_with("avg10=")))
                .and_then(|t| t[6..].parse::<f32>().ok())
                .unwrap_or(0.0);
            return if avg10 > 40.0 {
                Pressure::Critical
            } else if avg10 > 10.0 {
                Pressure::Warn
            } else {
                Pressure::Normal
            };
        }
    }
    Pressure::Normal
}

/// Admission controller shared by the worker pool.
///
/// Conditions are graded rather than all-or-nothing, because macOS routinely reports the
/// "warn" memory-pressure level on a busy machine and holding work until it clears could wait
/// forever:
///
/// * **hard limit** (critical pressure, or available memory below the floor of 5% / 512 MB):
///   no new OCR job starts;
/// * **soft limit** (available memory below the profile's reserve, "warn" pressure for the
///   gentle and balanced profiles, or a high load average for gentle): jobs run one at a time;
/// * otherwise jobs start freely, up to the pool size.
pub struct Governor {
    profile: Profile,
    total_memory: u64,
    logical_cores: usize,
    active: std::sync::Mutex<usize>,
    /// Last reason reported to the caller, so a condition is reported once per run, not per job.
    reported: std::sync::Mutex<Option<(bool, std::mem::Discriminant<Hold>)>>,
}

/// Why a job is being held back or serialised.
#[derive(Debug, Clone, PartialEq)]
pub enum Hold {
    /// Available memory is below the profile's reserve (soft) or the floor (hard).
    LowMemory {
        /// Available bytes.
        available: u64,
        /// Required bytes.
        required: u64,
    },
    /// The OS reports memory pressure.
    Pressure(Pressure),
    /// The machine is heavily loaded (gentle profile only).
    Load(f64),
}

/// Outcome of one admission check.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Start the job.
    Run,
    /// Start the job only if no other job is running.
    Serialize(Hold),
    /// Do not start the job.
    Wait(Hold),
}

/// Grade a snapshot of the machine for a profile. Pure function, so the policy can be tested.
pub fn decide(
    profile: Profile,
    total_memory: u64,
    available: u64,
    pressure: Pressure,
    load: f64,
    logical_cores: usize,
) -> Decision {
    let floor = ((total_memory as f64 * 0.05) as u64).max(512 << 20);
    if available < floor {
        return Decision::Wait(Hold::LowMemory { available, required: floor });
    }
    if pressure == Pressure::Critical {
        return Decision::Wait(Hold::Pressure(pressure));
    }
    let reserve = ((total_memory as f64 * profile.memory_reserve()) as u64).max(floor);
    if available < reserve {
        return Decision::Serialize(Hold::LowMemory { available, required: reserve });
    }
    if pressure == Pressure::Warn && profile != Profile::Max {
        return Decision::Serialize(Hold::Pressure(pressure));
    }
    if profile == Profile::Gentle && load > logical_cores as f64 * 1.5 {
        return Decision::Serialize(Hold::Load(load));
    }
    Decision::Run
}

/// A running job's place in the pool. Dropping it frees the place.
pub struct Slot<'a> {
    governor: &'a Governor,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut n = self.governor.active.lock().unwrap_or_else(|e| e.into_inner());
        *n = n.saturating_sub(1);
    }
}

impl Governor {
    /// Governor for a profile on the probed hardware.
    pub fn new(profile: Profile, hw: &Hardware) -> Self {
        Governor {
            profile,
            total_memory: hw.total_memory,
            logical_cores: hw.logical_cores,
            active: std::sync::Mutex::new(0),
            reported: std::sync::Mutex::new(None),
        }
    }

    /// The profile in force.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Grade the machine's current state.
    pub fn check(&self) -> Decision {
        let mut sys = System::new();
        sys.refresh_memory();
        decide(
            self.profile,
            self.total_memory,
            sys.available_memory(),
            memory_pressure(),
            System::load_average().one,
            self.logical_cores,
        )
    }

    /// Report a reason once until the condition changes or clears.
    fn report(
        &self,
        key: Option<(bool, std::mem::Discriminant<Hold>)>,
        h: Option<&Hold>,
        on_hold: &mut impl FnMut(&Hold),
    ) {
        let mut last = self.reported.lock().unwrap_or_else(|e| e.into_inner());
        if *last != key {
            if let Some(h) = h {
                on_hold(h);
            }
            *last = key;
        }
    }

    /// Block until a new job may start and return its slot. `on_hold` is called when the reason
    /// for waiting or for running one job at a time first appears or changes.
    pub fn admit(&self, mut on_hold: impl FnMut(&Hold)) -> Slot<'_> {
        loop {
            let decision = self.check();
            let key = match &decision {
                Decision::Run => None,
                Decision::Serialize(h) => Some((true, std::mem::discriminant(h))),
                Decision::Wait(h) => Some((false, std::mem::discriminant(h))),
            };
            let hold = match &decision {
                Decision::Run => None,
                Decision::Serialize(h) | Decision::Wait(h) => Some(h),
            };
            self.report(key, hold, &mut on_hold);
            {
                let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
                let start = match &decision {
                    Decision::Run => true,
                    Decision::Serialize(_) => *active == 0,
                    Decision::Wait(_) => false,
                };
                if start {
                    *active += 1;
                    return Slot { governor: self };
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn worker_processes_are_single_threaded() {
        let cmd = worker_command(std::path::Path::new("tesseract"), Profile::Balanced);
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.contains(&(OsStr::new("OMP_THREAD_LIMIT"), Some(OsStr::new("1")))));
    }

    const GIB: u64 = 1 << 30;

    #[test]
    fn warn_pressure_serialises_instead_of_waiting_forever() {
        let d = decide(Profile::Gentle, 24 * GIB, 12 * GIB, Pressure::Warn, 2.0, 10);
        assert_eq!(d, Decision::Serialize(Hold::Pressure(Pressure::Warn)));
        let d = decide(Profile::Max, 24 * GIB, 12 * GIB, Pressure::Warn, 2.0, 10);
        assert_eq!(d, Decision::Run);
    }

    #[test]
    fn hard_limits_wait() {
        assert!(matches!(
            decide(Profile::Max, 24 * GIB, 12 * GIB, Pressure::Critical, 0.0, 10),
            Decision::Wait(_)
        ));
        assert!(matches!(
            decide(Profile::Max, 24 * GIB, 256 << 20, Pressure::Normal, 0.0, 10),
            Decision::Wait(_)
        ));
    }

    #[test]
    fn below_reserve_serialises() {
        // gentle reserves 25%: 5 GiB available of 24 GiB is below it but above the 5% floor.
        let d = decide(Profile::Gentle, 24 * GIB, 5 * GIB, Pressure::Normal, 0.0, 10);
        assert!(matches!(d, Decision::Serialize(Hold::LowMemory { .. })));
        assert_eq!(decide(Profile::Balanced, 24 * GIB, 12 * GIB, Pressure::Normal, 50.0, 10), Decision::Run);
        assert!(matches!(
            decide(Profile::Gentle, 24 * GIB, 12 * GIB, Pressure::Normal, 50.0, 10),
            Decision::Serialize(Hold::Load(_))
        ));
    }

    #[test]
    fn slots_are_released() {
        let hw = Hardware::probe();
        let g = Governor::new(Profile::Max, &hw);
        if g.check() == Decision::Run {
            let a = g.admit(|_| {});
            let b = g.admit(|_| {});
            assert_eq!(*g.active.lock().unwrap(), 2);
            drop((a, b));
            assert_eq!(*g.active.lock().unwrap(), 0);
        }
    }

    #[test]
    fn pressure_is_ordered() {
        assert!(Pressure::Critical > Pressure::Warn && Pressure::Warn > Pressure::Normal);
    }
}
