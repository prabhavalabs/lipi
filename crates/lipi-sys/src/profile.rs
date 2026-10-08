//! Performance profiles and capability checks.

use crate::hardware::{Gpu, Hardware, human_bytes};
use serde::Serialize;
use std::str::FromStr;

/// How hard lipi may work the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Background work while the machine is in use: few workers, lowest priority
    /// (efficiency cores on Apple silicon), strict memory admission.
    Gentle,
    /// Default: one worker per performance core minus one, reduced priority.
    #[default]
    Balanced,
    /// Dedicated machine: one worker per physical core, normal priority.
    Max,
}

impl FromStr for Profile {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "gentle" | "low" | "background" => Ok(Profile::Gentle),
            "balanced" | "default" | "normal" => Ok(Profile::Balanced),
            "max" | "full" | "fast" => Ok(Profile::Max),
            other => Err(format!("unknown profile `{other}` (use gentle, balanced or max)")),
        }
    }
}

impl Profile {
    /// Name of the profile.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Gentle => "gentle",
            Profile::Balanced => "balanced",
            Profile::Max => "max",
        }
    }

    /// Number of concurrent OCR workers. Each worker runs one single-threaded OCR process.
    pub fn workers(self, hw: &Hardware) -> usize {
        let n = match self {
            Profile::Gentle => (hw.physical_cores / 4).clamp(1, 2),
            Profile::Balanced => hw.fast_cores().saturating_sub(1).max(1),
            Profile::Max => hw.physical_cores,
        };
        // Leave at least ~600 MB of memory per worker.
        let by_memory = (hw.total_memory / (600 << 20)).max(1) as usize;
        n.min(by_memory).max(1)
    }

    /// Process niceness applied on Unix.
    pub fn niceness(self) -> i32 {
        match self {
            Profile::Gentle => 19,
            Profile::Balanced => 10,
            Profile::Max => 0,
        }
    }

    /// Fraction of total memory that must stay available before a new job starts.
    pub fn memory_reserve(self) -> f64 {
        match self {
            Profile::Gentle => 0.25,
            Profile::Balanced => 0.15,
            Profile::Max => 0.05,
        }
    }
}

/// Severity of a capability finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Informational.
    Ok,
    /// Works, with a limitation.
    Warn,
    /// Will not work.
    Error,
}

/// One capability finding.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Severity.
    pub level: Level,
    /// What was checked.
    pub subject: String,
    /// Result and advice.
    pub message: String,
}

fn finding(level: Level, subject: &str, message: String) -> Finding {
    Finding { level, subject: subject.to_string(), message }
}

/// Assess what this machine can run.
pub fn capabilities(hw: &Hardware) -> Vec<Finding> {
    let gib = hw.total_gib();
    let mut f = Vec::new();
    f.push(if gib < 4.0 {
        finding(
            Level::Warn,
            "memory",
            format!("{gib:.1} GiB total: OCR will run with one worker; large scans may be slow"),
        )
    } else {
        finding(
            Level::Ok,
            "memory",
            format!("{gib:.1} GiB total, {} available", human_bytes(hw.available_memory)),
        )
    });
    f.push(if hw.physical_cores < 2 {
        finding(Level::Warn, "cpu", format!("{} core: OCR of long documents will be slow", hw.physical_cores))
    } else {
        let split = match (hw.performance_cores, hw.efficiency_cores) {
            (Some(p), Some(e)) => format!(" ({p} performance + {e} efficiency)"),
            _ => String::new(),
        };
        finding(Level::Ok, "cpu", format!("{} physical cores{split}", hw.physical_cores))
    });
    match hw.data_disk_free {
        Some(free) if free < (500 << 20) => f.push(finding(
            Level::Error,
            "disk",
            format!(
                "only {} free for models (about 45 MB needed); set LIPI_HOME to another disk",
                human_bytes(free)
            ),
        )),
        Some(free) => f.push(finding(Level::Ok, "disk", format!("{} free for models", human_bytes(free)))),
        None => {}
    }
    let vlm = match &hw.gpu {
        Gpu::AppleMetal { chip } if gib >= 16.0 => finding(
            Level::Ok,
            "model engines",
            format!("{chip} with {gib:.0} GiB unified memory can run 1B-parameter OCR models (planned)"),
        ),
        Gpu::Nvidia { name, memory_mib } if *memory_mib >= 6000 => finding(
            Level::Ok,
            "model engines",
            format!("{name} ({memory_mib} MiB) can run 1B-parameter OCR models (planned)"),
        ),
        Gpu::None => finding(
            Level::Warn,
            "model engines",
            "no GPU found: model-based OCR engines (planned) will be unavailable; Tesseract is used".into(),
        ),
        _ => finding(
            Level::Warn,
            "model engines",
            "GPU memory below the ~16 GiB unified / 6 GiB dedicated needed for model-based OCR (planned)"
                .to_string(),
        ),
    };
    f.push(vlm);
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(physical: usize, perf: Option<usize>, gib: u64) -> Hardware {
        Hardware {
            os: "test".into(),
            arch: "x".into(),
            cpu: "cpu".into(),
            logical_cores: physical,
            physical_cores: physical,
            performance_cores: perf,
            efficiency_cores: None,
            total_memory: gib << 30,
            available_memory: gib << 29,
            gpu: Gpu::None,
            data_disk_free: Some(10 << 30),
        }
    }

    #[test]
    fn worker_counts() {
        let m4 = hw(10, Some(4), 24);
        assert_eq!(Profile::Gentle.workers(&m4), 2);
        assert_eq!(Profile::Balanced.workers(&m4), 3);
        assert_eq!(Profile::Max.workers(&m4), 10);
        let tiny = hw(2, None, 1);
        assert_eq!(Profile::Max.workers(&tiny), 1);
    }

    #[test]
    fn low_memory_warns() {
        let f = capabilities(&hw(2, None, 2));
        assert!(f.iter().any(|x| x.subject == "memory" && x.level == Level::Warn));
    }
}
