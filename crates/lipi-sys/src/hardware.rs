//! Hardware probe: CPU (including Apple performance/efficiency cores), memory, GPU and disk.

use serde::Serialize;
use std::path::Path;
use std::process::Command;
use sysinfo::{Disks, System};

/// GPU acceleration available to local models.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gpu {
    /// Apple silicon GPU (Metal), sharing unified memory with the CPU.
    AppleMetal {
        /// Chip name, e.g. `Apple M4`.
        chip: String,
    },
    /// NVIDIA GPU with CUDA.
    Nvidia {
        /// Device name.
        name: String,
        /// Device memory in MiB.
        memory_mib: u64,
    },
    /// No usable GPU found.
    None,
}

/// What the machine offers.
#[derive(Debug, Clone, Serialize)]
pub struct Hardware {
    /// Operating system and version.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// CPU brand string.
    pub cpu: String,
    /// Logical CPUs.
    pub logical_cores: usize,
    /// Physical cores.
    pub physical_cores: usize,
    /// Performance cores (Apple silicon and other hybrid CPUs where known).
    pub performance_cores: Option<usize>,
    /// Efficiency cores (where known).
    pub efficiency_cores: Option<usize>,
    /// Total memory in bytes.
    pub total_memory: u64,
    /// Available memory in bytes.
    pub available_memory: u64,
    /// GPU acceleration.
    pub gpu: Gpu,
    /// Free bytes on the disk that holds the lipi data directory.
    pub data_disk_free: Option<u64>,
}

fn sysctl(key: &str) -> Option<String> {
    let out = Command::new("sysctl").arg("-n").arg(key).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn sysctl_usize(key: &str) -> Option<usize> {
    sysctl(key)?.parse().ok()
}

fn nvidia() -> Option<Gpu> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).lines().next()?.to_string();
    let (name, mem) = line.split_once(',')?;
    Some(Gpu::Nvidia { name: name.trim().to_string(), memory_mib: mem.trim().parse().unwrap_or(0) })
}

fn disk_free(path: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    let target = std::fs::canonicalize(path)
        .or_else(|_| std::fs::canonicalize(path.parent().unwrap_or(path)))
        .unwrap_or_else(|_| path.to_path_buf());
    disks
        .list()
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

impl Hardware {
    /// Probe the running machine. Takes a few milliseconds.
    pub fn probe() -> Hardware {
        let mut sys = System::new();
        sys.refresh_memory();
        sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
        let logical = sys.cpus().len().max(1);
        let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        let physical = System::physical_core_count().unwrap_or(logical);
        let (perf, eff) = if cfg!(target_os = "macos") {
            (sysctl_usize("hw.perflevel0.physicalcpu"), sysctl_usize("hw.perflevel1.physicalcpu"))
        } else {
            (None, None)
        };
        let gpu = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Gpu::AppleMetal { chip: sysctl("machdep.cpu.brand_string").unwrap_or_else(|| cpu.clone()) }
        } else {
            nvidia().unwrap_or(Gpu::None)
        };
        let data = crate::paths::data_dir();
        Hardware {
            os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_string()),
            arch: std::env::consts::ARCH.to_string(),
            cpu,
            logical_cores: logical,
            physical_cores: physical,
            performance_cores: perf,
            efficiency_cores: eff,
            total_memory: sys.total_memory(),
            available_memory: sys.available_memory(),
            gpu,
            data_disk_free: disk_free(&data),
        }
    }

    /// Cores best suited to sustained OCR work (performance cores where known).
    pub fn fast_cores(&self) -> usize {
        self.performance_cores.unwrap_or(self.physical_cores).max(1)
    }

    /// Total memory in GiB.
    pub fn total_gib(&self) -> f64 {
        self.total_memory as f64 / (1u64 << 30) as f64
    }
}

/// Format a byte count for people.
pub fn human_bytes(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < units.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", units[i]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_sane_values() {
        let hw = Hardware::probe();
        assert!(hw.logical_cores >= 1 && hw.physical_cores >= 1);
        assert!(hw.total_memory > 0);
        assert!(hw.fast_cores() >= 1);
    }

    #[test]
    fn human_readable_sizes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MB");
    }
}
