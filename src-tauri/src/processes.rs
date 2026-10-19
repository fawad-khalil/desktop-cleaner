//! Process listing via sysinfo. The `System` is kept alive between calls so
//! CPU usage (which needs two samples) is meaningful.

use serde::Serialize;
use std::sync::Mutex;
use sysinfo::{ProcessesToUpdate, System};

pub struct ProcState(pub Mutex<System>);

impl ProcState {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        ProcState(Mutex::new(sys))
    }
}

#[derive(Serialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub exe: Option<String>,
    pub memory: u64,
    /// Percent of total CPU capacity (0-100), normalised across cores.
    pub cpu: f32,
    pub disk_read: u64,
    pub disk_written: u64,
    pub run_time: u64,
}

#[derive(Serialize)]
pub struct ProcSnapshot {
    pub processes: Vec<ProcInfo>,
    pub total_memory: u64,
    pub used_memory: u64,
    pub cpu: f32,
    pub cpu_count: usize,
}

pub fn snapshot(state: &ProcState) -> ProcSnapshot {
    let mut sys = state.0.lock().unwrap();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    let cpus = sys.cpus().len().max(1);
    let processes = sys
        .processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| {
            let du = p.disk_usage();
            ProcInfo {
                pid: p.pid().as_u32(),
                parent: p.parent().map(|pp| pp.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                exe: p.exe().map(|e| e.to_string_lossy().into_owned()),
                memory: p.memory(),
                cpu: p.cpu_usage() / cpus as f32,
                disk_read: du.read_bytes,
                disk_written: du.written_bytes,
                run_time: p.run_time(),
            }
        })
        .collect();
    ProcSnapshot {
        processes,
        total_memory: sys.total_memory(),
        used_memory: sys.used_memory(),
        cpu: sys.global_cpu_usage(),
        cpu_count: cpus,
    }
}
