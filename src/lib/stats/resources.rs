use std::sync::Mutex;

use anyhow::Result;
use lazy_static::lazy_static;
use serde::Serialize;
use sysinfo::{get_current_pid, ProcessExt, System, SystemExt};

lazy_static! {
    static ref SYSTEM: Mutex<System> = Mutex::new(System::new_all());
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
    sys.refresh_cpu();
    sys.refresh_memory();
    sys.refresh_processes();

    let pid = get_current_pid().expect("Failed getting current process' PID");
    let process = sys.process(pid).expect("Failed getting proccess from PID");

    return Ok(ResourceUsage {
        run_time: process.run_time(),
        cpu_usage: process.cpu_usage() / sys.cpus().len() as f32,
        memory_usage_bytes: process.memory(),
        total_memory_bytes: sys.total_memory(),
    });
}
