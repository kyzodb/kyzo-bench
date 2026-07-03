use serde::{Deserialize, Serialize};
use std::fs;

/// The machine a run happened on, captured from the machine itself.
///
/// There is no constructor that takes strings: [`Hardware::capture`] reads
/// `/proc` and `uname`, so a record can never carry a hand-typed (or
/// hand-flattered) spec. Two records are comparable only if this struct says
/// they ran on the same class of box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hardware {
    pub cpu_model: String,
    pub logical_cpus: usize,
    pub mem_total_kib: u64,
    pub arch: String,
    pub kernel: String,
}

impl Hardware {
    pub fn capture() -> Hardware {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let cpu_model = cpuinfo
            .lines()
            .find_map(|l| l.strip_prefix("model name"))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|| "unknown".to_owned());
        let logical_cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
        let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let mem_total_kib = meminfo
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|l| l.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let uname = |flag: &str| {
            std::process::Command::new("uname")
                .arg(flag)
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_else(|| "unknown".to_owned())
        };
        Hardware {
            cpu_model,
            logical_cpus,
            mem_total_kib,
            arch: uname("-m"),
            kernel: uname("-r"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_reads_a_real_machine() {
        let hw = Hardware::capture();
        assert!(hw.logical_cpus > 0);
        assert!(hw.mem_total_kib > 0);
        assert_ne!(hw.cpu_model, "unknown");
        assert_ne!(hw.arch, "unknown");
    }
}
