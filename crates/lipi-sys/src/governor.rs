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
use std::time::{Duration, Instant};
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
pub struct Governor {
    profile: Profile,
    total_memory: u64,
    logical_cores: usize,
}

/// Why a job is being held back.
#[derive(Debug, Clone, PartialEq)]
pub enum Hold {
    /// Available memory is below the profile's reserve.
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

impl Governor {
    /// Governor for a profile on the probed hardware.
    pub fn new(profile: Profile, hw: &Hardware) -> Self {
        Governor { profile, total_memory: hw.total_memory, logical_cores: hw.logical_cores }
    }

    /// The profile in force.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Check once whether a new job may start.
    pub fn check(&self) -> Option<Hold> {
        let mut sys = System::new();
        sys.refresh_memory();
        let available = sys.available_memory();
        let required = ((self.total_memory as f64 * self.profile.memory_reserve()) as u64).max(512 << 20);
        if available < required {
            return Some(Hold::LowMemory { available, required });
        }
        let p = memory_pressure();
        let limit = if self.profile == Profile::Max { Pressure::Critical } else { Pressure::Warn };
        if p >= limit {
            return Some(Hold::Pressure(p));
        }
        if self.profile == Profile::Gentle {
            let load = System::load_average().one;
            if load > self.logical_cores as f64 * 1.5 {
                return Some(Hold::Load(load));
            }
        }
        None
    }

    /// Block until a new job may start. `on_hold` is called once per distinct hold.
    pub fn admit(&self, mut on_hold: impl FnMut(&Hold)) -> Duration {
        let start = Instant::now();
        let mut last: Option<std::mem::Discriminant<Hold>> = None;
        while let Some(h) = self.check() {
            let d = std::mem::discriminant(&h);
            if last != Some(d) {
                on_hold(&h);
                last = Some(d);
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
        start.elapsed()
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

    #[test]
    fn pressure_is_ordered() {
        assert!(Pressure::Critical > Pressure::Warn && Pressure::Warn > Pressure::Normal);
    }
}
