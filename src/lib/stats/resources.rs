use std::sync::Mutex;

use anyhow::Result;
use lazy_static::lazy_static;
use serde::Serialize;
use sysinfo::{
    MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, get_current_pid,
};

lazy_static! {
    static ref SYSTEM: Mutex<System> = Mutex::new(System::new_with_specifics(
        RefreshKind::nothing()
            // Processes time, CPU, and memory:
            .with_processes(ProcessRefreshKind::nothing().with_cpu().with_memory())
            // System memory:
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    ));
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct ResourceUsage {
    pub run_time: u64,
    pub cpu_usage: f32,
    pub memory_usage_bytes: u64,
    pub total_memory_bytes: u64,
}

pub fn usage() -> Result<ResourceUsage> {
    let mut sys = SYSTEM.lock().unwrap();

    let pid = get_current_pid().expect("Failed getting current process' PID");

    // Refresh our process time, CPU, and memory:
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );

    // Refresh system memory:
    sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());

    let process = sys.process(pid).expect("Failed getting proccess from PID");

    Ok(ResourceUsage {
        run_time: process.run_time(),
        cpu_usage: process.cpu_usage() / sys.cpus().len() as f32,
        memory_usage_bytes: process.memory(),
        total_memory_bytes: sys.total_memory(),
    })
}
