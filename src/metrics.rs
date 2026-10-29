use serde_derive::Serialize;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Serialize)]
pub struct ProcessMetrics {
    process_id: u32,
    cpu_seconds_total: f64,
    memory_rss_bytes: u64,
    sampled_at_unix_seconds: f64,
}

pub fn collect() -> Result<ProcessMetrics, Box<dyn std::error::Error>> {
    let process_id = std::process::id();
    let pid = Pid::from_u32(process_id);
    let mut system = System::new()?;
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .without_tasks(),
    );
    let process = system
        .process(pid)
        .ok_or_else(|| io::Error::other("Unable to read API process metrics"))?;

    Ok(ProcessMetrics {
        process_id,
        cpu_seconds_total: process.accumulated_cpu_time() as f64 / 1000.0,
        memory_rss_bytes: process.memory(),
        sampled_at_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn metrics_measure_current_process_and_accumulate_cpu_time() {
        let before = collect().unwrap();
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(100) {
            std::hint::black_box(started.elapsed());
        }
        let after = collect().unwrap();

        assert_eq!(after.process_id, std::process::id());
        assert!(after.memory_rss_bytes > 0);
        assert!(after.cpu_seconds_total > before.cpu_seconds_total);
        assert!(after.sampled_at_unix_seconds > before.sampled_at_unix_seconds);

        let json = serde_json::to_value(&after).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 4);
        assert_eq!(json["process_id"], std::process::id());
        assert_eq!(json["cpu_seconds_total"], after.cpu_seconds_total);
        assert_eq!(json["memory_rss_bytes"], after.memory_rss_bytes);
        assert_eq!(
            json["sampled_at_unix_seconds"],
            after.sampled_at_unix_seconds
        );
    }
}
