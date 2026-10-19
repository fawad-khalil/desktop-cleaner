//! Process listing via sysinfo. The `System` is kept alive between calls so
//! CPU usage (which needs two samples) is meaningful.
//!
//! Per-process network *bytes* aren't exposed by any OS without admin rights
//! or packet capture, so per process we report open connections (via
//! netstat2) and system-wide throughput separately.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Networks, ProcessesToUpdate, System};

pub struct Inner {
    sys: System,
    nets: Networks,
    last: Instant,
}

pub struct ProcState(pub Mutex<Inner>);

impl ProcState {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        ProcState(Mutex::new(Inner {
            sys,
            nets: Networks::new_with_refreshed_list(),
            last: Instant::now(),
        }))
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
    /// Disk bytes read/written since the previous snapshot.
    pub disk_read: u64,
    pub disk_written: u64,
    pub run_time: u64,
    /// Established TCP connections.
    pub connections: u32,
    /// Listening TCP sockets plus bound UDP sockets.
    pub listening: u32,
}

#[derive(Serialize)]
pub struct ProcSnapshot {
    pub processes: Vec<ProcInfo>,
    pub total_memory: u64,
    pub used_memory: u64,
    pub cpu: f32,
    pub cpu_count: usize,
    /// System-wide network throughput in bytes per second.
    pub net_rx_rate: f64,
    pub net_tx_rate: f64,
    pub net_available: bool,
}

/// (established, listening) per pid.
fn socket_counts() -> Option<HashMap<u32, (u32, u32)>> {
    use netstat2::*;
    let socks = get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP | ProtocolFlags::UDP,
    )
    .ok()?;
    let mut m: HashMap<u32, (u32, u32)> = HashMap::new();
    for s in socks {
        let (est, listen) = match &s.protocol_socket_info {
            ProtocolSocketInfo::Tcp(t) if t.state == TcpState::Established => (1, 0),
            ProtocolSocketInfo::Tcp(t) if t.state == TcpState::Listen => (0, 1),
            ProtocolSocketInfo::Tcp(_) => (0, 0),
            ProtocolSocketInfo::Udp(_) => (0, 1),
        };
        for pid in s.associated_pids {
            let e = m.entry(pid).or_default();
            e.0 += est;
            e.1 += listen;
        }
    }
    Some(m)
}

pub fn snapshot(state: &ProcState) -> ProcSnapshot {
    let sockets = socket_counts();
    let mut g = state.0.lock().unwrap();
    let Inner { sys, nets, last } = &mut *g;

    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    nets.refresh(true);
    let secs = last.elapsed().as_secs_f64().max(0.001);
    *last = Instant::now();
    let (rx, tx) = nets
        .list()
        .values()
        .fold((0u64, 0u64), |(r, t), n| (r + n.received(), t + n.transmitted()));

    let cpus = sys.cpus().len().max(1);
    let processes = sys
        .processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| {
            let du = p.disk_usage();
            let pid = p.pid().as_u32();
            let (connections, listening) = sockets
                .as_ref()
                .and_then(|m| m.get(&pid).copied())
                .unwrap_or_default();
            ProcInfo {
                pid,
                parent: p.parent().map(|pp| pp.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                exe: p.exe().map(|e| e.to_string_lossy().into_owned()),
                memory: p.memory(),
                cpu: p.cpu_usage() / cpus as f32,
                disk_read: du.read_bytes,
                disk_written: du.written_bytes,
                run_time: p.run_time(),
                connections,
                listening,
            }
        })
        .collect();
    ProcSnapshot {
        processes,
        total_memory: sys.total_memory(),
        used_memory: sys.used_memory(),
        cpu: sys.global_cpu_usage(),
        cpu_count: cpus,
        net_rx_rate: rx as f64 / secs,
        net_tx_rate: tx as f64 / secs,
        net_available: sockets.is_some(),
    }
}
