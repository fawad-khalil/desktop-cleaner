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
use sysinfo::{Networks, Pid, Process, ProcessesToUpdate, System};

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
    /// Friendly name of the app this process belongs to, e.g. every
    /// "Google Chrome Helper (Renderer)" is part of "Google Chrome".
    pub app: String,
    pub kind: Kind,
    /// Runs as the current user (only those can be quit).
    pub mine: bool,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Something the user opened.
    App,
    /// User-level helpers, agents and tools without a window of their own.
    Background,
    /// Part of the operating system, or run by another account.
    System,
}

struct Class {
    app: String,
    kind: Kind,
    /// Identified on its own (an app bundle, or launched from the desktop
    /// shell), so its child processes can be attributed to it.
    anchor: bool,
}

/// Well-known executables whose file names are not their product names.
/// (macOS names come from app bundles instead.)
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn friendly(stem: &str) -> String {
    let known = [
        ("chrome", "Google Chrome"), ("msedge", "Microsoft Edge"), ("firefox", "Firefox"),
        ("code", "Visual Studio Code"), ("explorer", "File Explorer"), ("ms-teams", "Microsoft Teams"),
        ("teams", "Microsoft Teams"), ("winword", "Microsoft Word"), ("excel", "Microsoft Excel"),
        ("powerpnt", "Microsoft PowerPoint"), ("outlook", "Microsoft Outlook"), ("olk", "Microsoft Outlook"),
        ("onedrive", "OneDrive"), ("slack", "Slack"), ("discord", "Discord"), ("spotify", "Spotify"),
        ("zoom", "Zoom"), ("steam", "Steam"), ("thunderbird", "Thunderbird"), ("nautilus", "Files"),
        ("gnome-shell", "GNOME Shell"), ("plasmashell", "KDE Plasma"), ("msedgewebview2", "Microsoft Edge WebView"),
    ];
    let lower = stem.to_lowercase();
    if let Some((_, n)) = known.iter().find(|(k, _)| *k == lower) {
        return n.to_string();
    }
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn starts_with_any(p: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|x| p.starts_with(x))
}

/// Classifies one process on its own; `classify_all` then lets helpers
/// inherit the app of the process that started them.
fn classify(p: &Process, mine: bool, parent_name: Option<&str>) -> Class {
    let name = p.name().to_string_lossy().into_owned();
    let exe = p.exe().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name.clone());
    let _ = parent_name;

    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
        // The outermost bundle names the app: ".../Google Chrome.app/.../Google Chrome Helper.app/..."
        if let Some(i) = exe.find(".app/") {
            let bundle = &exe[..i];
            let app = bundle.rsplit('/').next().unwrap_or(bundle).to_string();
            let user_apps = ["/Applications/", "/System/Applications/", &format!("{home}/Applications/")];
            // Only the bundle's main executable means the app is open;
            // widgets and extensions inside it run on their own.
            let main = exe[i + 5..].strip_prefix("Contents/MacOS/").is_some_and(|r| !r.contains('/'));
            let kind = if !mine || (exe.starts_with("/System/") && !exe.starts_with("/System/Applications/")) {
                Kind::System
            } else if main && starts_with_any(&exe, &user_apps) {
                Kind::App
            } else {
                Kind::Background
            };
            return Class { app, kind, anchor: kind == Kind::App };
        }
        let system = exe.is_empty()
            || !mine
            || starts_with_any(&exe, &["/System/", "/usr/", "/bin/", "/sbin/", "/Library/Apple/", "/private/", "/Library/Developer/CommandLineTools/"]);
        Class { app: name, kind: if system { Kind::System } else { Kind::Background }, anchor: false }
    }

    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
        let lower = exe.to_lowercase();
        let system = !mine || exe.is_empty() || lower.starts_with(&root);
        let from_shell = parent_name.is_some_and(|n| n.eq_ignore_ascii_case("explorer.exe"));
        let kind = if system { Kind::System } else if from_shell { Kind::App } else { Kind::Background };
        Class { app: friendly(&stem), kind, anchor: kind == Kind::App }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // No executable means a kernel thread.
        let system = !mine || exe.is_empty() || starts_with_any(&exe, &["/usr/sbin/", "/sbin/"]);
        let helper = starts_with_any(&exe, &["/usr/lib", "/usr/libexec/", "/lib/"])
            || ["systemd", "dbus-daemon", "dbus-broker", "pipewire", "pipewire-pulse", "wireplumber", "pulseaudio", "xdg-desktop-portal", "at-spi-bus-launcher", "gvfsd", "ssh-agent", "gpg-agent", "bash", "zsh", "sh", "fish"]
                .iter()
                .any(|n| *n == stem);
        let kind = if system { Kind::System } else if helper { Kind::Background } else { Kind::App };
        Class { app: friendly(&stem), kind, anchor: kind == Kind::App }
    }
}

/// Classifies every process; helpers that aren't identifiable on their own
/// (a shell started by Terminal, a renderer started by a browser) take the
/// app of their nearest identifiable ancestor.
fn classify_all(sys: &System) -> HashMap<Pid, (Class, bool)> {
    let me = sysinfo::get_current_pid().ok().and_then(|p| sys.process(p)).and_then(|p| p.user_id().cloned());
    let mut out: HashMap<Pid, (Class, bool)> = sys
        .processes()
        .iter()
        .map(|(pid, p)| {
            let mine = me.is_none() || p.user_id() == me.as_ref();
            let parent = p.parent().and_then(|pp| sys.process(pp)).map(|pp| pp.name().to_string_lossy().into_owned());
            (*pid, (classify(p, mine, parent.as_deref()), mine))
        })
        .collect();
    let pids: Vec<Pid> = out.keys().copied().collect();
    for pid in pids {
        let (c, mine) = &out[&pid];
        if c.anchor || !mine {
            continue;
        }
        let mut cur = sys.process(pid).and_then(|p| p.parent());
        for _ in 0..32 {
            let Some(pp) = cur else { break };
            if pp == pid {
                break;
            }
            if let Some((a, _)) = out.get(&pp) {
                if a.anchor {
                    let app = a.app.clone();
                    let e = &mut out.get_mut(&pid).unwrap().0;
                    e.app = app;
                    e.kind = Kind::App;
                    break;
                }
            }
            cur = sys.process(pp).and_then(|p| p.parent());
        }
    }
    // Once an app is open, its other processes (extensions, agents) count
    // as part of it.
    let open: std::collections::HashSet<String> =
        out.values().filter(|(c, _)| c.kind == Kind::App).map(|(c, _)| c.app.clone()).collect();
    for (c, mine) in out.values_mut() {
        if *mine && c.kind == Kind::Background && open.contains(&c.app) {
            c.kind = Kind::App;
        }
    }
    out
}

/// Asks processes to quit (SIGTERM / a normal close on Windows), so apps
/// can shut down cleanly. Refuses system processes, other users'
/// processes, and this app or anything it runs under.
pub fn quit(state: &ProcState, pids: &[u32]) -> anyhow::Result<u32> {
    let mut g = state.0.lock().unwrap();
    let sys = &mut g.sys;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let classes = classify_all(sys);

    let mut protected = std::collections::HashSet::new();
    let mut cur = sysinfo::get_current_pid().ok();
    while let Some(p) = cur {
        if !protected.insert(p) {
            break;
        }
        cur = sys.process(p).and_then(|x| x.parent());
    }

    let mut done = 0;
    for &raw in pids {
        let pid = Pid::from_u32(raw);
        let (Some(p), Some((c, mine))) = (sys.process(pid), classes.get(&pid)) else { continue };
        if protected.contains(&pid) || !*mine || c.kind == Kind::System || raw <= 1 {
            anyhow::bail!("{} can't be quit from here", p.name().to_string_lossy());
        }
        #[cfg(unix)]
        let ok = p.kill_with(sysinfo::Signal::Term).unwrap_or(false);
        #[cfg(windows)]
        let ok = {
            use std::os::windows::process::CommandExt;
            // Without /F, taskkill asks the app to close like clicking ✕.
            std::process::Command::new("taskkill")
                .args(["/PID", &raw.to_string()])
                .creation_flags(0x0800_0000)
                .status()
                .is_ok_and(|s| s.success())
        };
        if ok {
            done += 1;
        }
    }
    Ok(done)
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
    let classes = classify_all(sys);
    let processes = sys
        .processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| {
            let du = p.disk_usage();
            let (class, mine) = &classes[&p.pid()];
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
                app: class.app.clone(),
                kind: class.kind,
                mine: *mine,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_running_processes() {
        let st = ProcState::new();
        let snap = snapshot(&st);
        let me = snap.processes.iter().find(|p| p.pid == std::process::id()).expect("own process listed");
        assert!(me.mine);
        assert_ne!(me.kind, Kind::System);
        // pid 1 (launchd / systemd / init) belongs to the OS.
        #[cfg(unix)]
        if let Some(init) = snap.processes.iter().find(|p| p.pid == 1) {
            assert_eq!(init.kind, Kind::System);
        }
        if std::env::var_os("SHOW_PROCS").is_some() {
            let mut v: Vec<_> = snap.processes.iter().map(|p| (format!("{:?}", p.kind), p.app.clone(), p.name.clone())).collect();
            v.sort();
            v.dedup();
            for x in v {
                eprintln!("{x:?}");
            }
        }
    }

    #[test]
    fn refuses_to_quit_itself_or_the_os() {
        let st = ProcState::new();
        assert!(quit(&st, &[std::process::id()]).is_err());
        #[cfg(unix)]
        assert!(quit(&st, &[1]).is_err());
    }
}
