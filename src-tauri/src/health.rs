//! Hardware health: temperatures, fans, GPU, battery, power adapter and
//! storage, from whatever each OS exposes without admin rights. Anything
//! that can't be read is reported as unavailable with the reason; values
//! are never guessed.
//!
//! Battery health is recorded once a day to the app data folder so the UI
//! can show a trend and an estimate of when it reaches 80%.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Clone, Debug)]
pub struct Sensor {
    pub label: String,
    pub celsius: f32,
}

#[derive(Serialize, Default, Clone)]
pub struct Temps {
    pub cpu: Vec<Sensor>,
    pub gpu: Vec<Sensor>,
    pub storage: Vec<Sensor>,
    pub battery: Vec<Sensor>,
    pub ram: Vec<Sensor>,
    pub other: Vec<Sensor>,
    /// Why no temperatures could be read, if none were.
    pub note: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Fan {
    pub index: u32,
    pub rpm: f32,
    pub min_rpm: Option<f32>,
    pub max_rpm: Option<f32>,
}

#[derive(Serialize, Default, Clone)]
pub struct Fans {
    pub fans: Vec<Fan>,
    pub note: Option<String>,
}

#[derive(Serialize, Default, Clone)]
pub struct Gpu {
    pub name: Option<String>,
    pub utilization: Option<f32>,
    pub note: Option<String>,
}

#[derive(Serialize, Default, Clone)]
pub struct Battery {
    pub percent: Option<f32>,
    pub charging: bool,
    pub plugged_in: bool,
    pub fully_charged: bool,
    pub cycles: Option<u32>,
    /// The cycle count the battery is rated for, when the OS reports it.
    pub design_cycles: Option<u32>,
    pub design_mah: Option<f32>,
    pub max_mah: Option<f32>,
    /// max / design capacity, in percent.
    pub health_pct: Option<f32>,
    pub condition: Option<String>,
    pub temp_c: Option<f32>,
    pub voltage_v: Option<f32>,
    /// Positive while charging, negative while discharging.
    pub current_a: Option<f32>,
    pub power_w: Option<f32>,
    pub minutes_to_empty: Option<u32>,
    pub minutes_to_full: Option<u32>,
}

#[derive(Serialize, Default, Clone)]
pub struct Power {
    pub adapter_connected: bool,
    pub adapter_name: Option<String>,
    pub adapter_watts: Option<f32>,
    pub adapter_voltage_v: Option<f32>,
    pub adapter_current_a: Option<f32>,
    /// What the whole computer is drawing right now.
    pub system_power_w: Option<f32>,
    pub note: Option<String>,
}

#[derive(Serialize, Default, Clone)]
pub struct Disk {
    pub name: String,
    /// "Verified", "Failing", "OK"… as the OS words it.
    pub smart_status: Option<String>,
    pub temp_c: Option<f32>,
    /// NVMe "percentage used" (wear).
    pub wear_pct: Option<f32>,
    pub data_written_bytes: Option<u64>,
    pub power_on_hours: Option<u64>,
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DailyRecord {
    /// Days since the Unix epoch.
    pub day: u64,
    pub health_pct: f32,
    pub max_mah: f32,
    pub design_mah: f32,
    pub cycles: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Projection {
    /// Estimated date (epoch seconds) health reaches 80%, from the trend.
    pub date_80: Option<u64>,
    /// Estimated cycle count at which health reaches 80%.
    pub cycles_80: Option<u32>,
    /// How the estimate was made, in plain words.
    pub basis: String,
}

#[derive(Serialize, Clone)]
pub struct DrainSample {
    pub at: u64,
    pub percent: f32,
    pub power_w: f32,
}

#[derive(Serialize, Default)]
pub struct Health {
    pub temps: Temps,
    pub fans: Fans,
    pub gpu: Gpu,
    pub battery: Option<Battery>,
    pub battery_note: Option<String>,
    pub power: Power,
    pub storage: Vec<Disk>,
    pub storage_note: Option<String>,
    pub ram_temp_note: Option<String>,
    pub history: Vec<DailyRecord>,
    pub projection: Option<Projection>,
    /// Recent battery level and draw this session, oldest first.
    pub drain: Vec<DrainSample>,
}

pub struct HealthState(pub Mutex<Inner>);

/// The app's one health reader (sensor handles and the drain history).
pub static STATE: std::sync::LazyLock<HealthState> = std::sync::LazyLock::new(HealthState::new);

pub struct Inner {
    comps: sysinfo::Components,
    storage: Option<(Instant, Vec<Disk>, Option<String>)>,
    drain: Vec<DrainSample>,
    history_file: Option<PathBuf>,
}

impl HealthState {
    pub fn new() -> Self {
        HealthState(Mutex::new(Inner {
            comps: sysinfo::Components::new_with_refreshed_list(),
            storage: None,
            drain: Vec::new(),
            history_file: None,
        }))
    }
    pub fn set_history_file(&self, p: PathBuf) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).history_file = Some(p);
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Sorts a sensor into CPU/GPU/storage/battery/RAM by its label. Labels
/// differ per OS and driver (Apple "PMU tdie3", Linux "Core 0", "edge",
/// "Composite"…), so this is keyword-based.
pub fn classify(label: &str) -> &'static str {
    let l = label.to_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| l.contains(k));
    if has(&["nand", "nvme", "composite", "ssd", "drive", "disk", "sata"]) {
        "storage"
    } else if has(&["battery", "gas gauge", "bat"]) {
        "battery"
    } else if has(&["dimm", "dram", "memory", " ram", "sodimm"]) || l.starts_with("ram") {
        "ram"
    } else if has(&["gpu", "edge", "junction", "amdgpu", "nouveau", "radeon"]) || is_apple(&l, 'g') {
        "gpu"
    } else if has(&["tdie", "tctl", "core", "package", "cpu", "k10temp", "coretemp", "soc"]) || is_apple(&l, 's') {
        "cpu"
    } else {
        "other"
    }
}

/// Apple Silicon "PMU TP<n><kind>" sensors: g = GPU, s = SoC/CPU cluster.
fn is_apple(l: &str, kind: char) -> bool {
    l.strip_prefix("pmu tp").is_some_and(|r| r.ends_with(kind) && r[..r.len() - 1].chars().all(|c| c.is_ascii_digit()))
}

fn read_temps(comps: &mut sysinfo::Components) -> Temps {
    comps.refresh(true);
    let mut t = Temps::default();
    for c in comps.list() {
        let Some(v) = c.temperature() else { continue };
        if !v.is_finite() || v <= 0.0 || v > 150.0 {
            continue;
        }
        let s = Sensor { label: c.label().to_string(), celsius: v };
        match classify(&s.label) {
            "cpu" => t.cpu.push(s),
            "gpu" => t.gpu.push(s),
            "storage" => t.storage.push(s),
            "battery" => t.battery.push(s),
            "ram" => t.ram.push(s),
            _ => t.other.push(s),
        }
    }
    for v in [&mut t.cpu, &mut t.gpu, &mut t.storage, &mut t.battery, &mut t.ram, &mut t.other] {
        v.sort_by(|a, b| natural(&a.label).cmp(&natural(&b.label)));
    }
    if t.cpu.is_empty() && t.gpu.is_empty() && t.storage.is_empty() && t.other.is_empty() {
        t.note = Some(if cfg!(windows) {
            "Windows only shares temperature sensors with administrator tools (and many PCs not at all).".into()
        } else {
            "No temperature sensors were readable on this system.".into()
        });
    }
    t
}

/// Sort key so "tdie10" comes after "tdie9".
fn natural(s: &str) -> (String, u64) {
    let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    (s[..s.len() - digits.len()].to_string(), digits.parse().unwrap_or(0))
}

// ---------------------------------------------------------------------------
// Battery history and projections

fn load_history(f: &Option<PathBuf>) -> Vec<DailyRecord> {
    f.as_ref()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Adds today's battery health if it isn't recorded yet.
fn record_day(f: &Option<PathBuf>, b: &Battery) -> Vec<DailyRecord> {
    let mut h = load_history(f);
    let (Some(health), Some(max), Some(design), Some(cycles)) = (b.health_pct, b.max_mah, b.design_mah, b.cycles) else {
        return h;
    };
    let day = now() / 86_400;
    if h.last().is_some_and(|r| r.day == day) {
        return h;
    }
    h.push(DailyRecord { day, health_pct: health, max_mah: max, design_mah: design, cycles });
    if let Some(f) = f {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, serde_json::to_vec(&h).unwrap_or_default());
    }
    h
}

/// Estimates when health reaches 80%: from the recorded trend when there's
/// at least two weeks of it, otherwise from wear per cycle so far.
pub fn project(history: &[DailyRecord], current: &Battery) -> Option<Projection> {
    let health = current.health_pct?;
    if health <= 80.0 {
        return Some(Projection { date_80: None, cycles_80: None, basis: "Health is already at or below 80%.".into() });
    }
    // Least-squares line of health over days.
    if let (Some(first), Some(last)) = (history.first(), history.last()) {
        if last.day >= first.day + 14 && history.len() >= 3 {
            let n = history.len() as f64;
            let mx = history.iter().map(|r| r.day as f64).sum::<f64>() / n;
            let my = history.iter().map(|r| r.health_pct as f64).sum::<f64>() / n;
            let sxy: f64 = history.iter().map(|r| (r.day as f64 - mx) * (r.health_pct as f64 - my)).sum();
            let sxx: f64 = history.iter().map(|r| (r.day as f64 - mx).powi(2)).sum();
            let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
            if slope < -1e-6 {
                let day_80 = mx + (80.0 - my) / slope;
                let cycles_per_day = (last.cycles.saturating_sub(first.cycles)) as f64 / (last.day - first.day) as f64;
                return Some(Projection {
                    date_80: Some((day_80.max(last.day as f64) * 86_400.0) as u64),
                    cycles_80: (cycles_per_day > 0.0)
                        .then(|| (last.cycles as f64 + (day_80 - last.day as f64).max(0.0) * cycles_per_day) as u32),
                    basis: format!("Trend of {} daily readings over {} days.", history.len(), last.day - first.day),
                });
            }
        }
    }
    let cycles = current.cycles? as f64;
    if cycles < 50.0 {
        return None;
    }
    let per_cycle = (100.0 - health as f64) / cycles;
    if per_cycle <= 0.0 {
        return None;
    }
    let cycles_80 = (20.0 / per_cycle) as u32;
    Some(Projection {
        date_80: None,
        cycles_80: Some(cycles_80),
        basis: format!(
            "Wear so far ({:.1}% per 100 cycles); a dated estimate appears after two weeks of daily readings.",
            per_cycle * 100.0
        ),
    })
}

/// Reads everything. Slow sources (storage) are cached for a minute.
pub fn read(state: &HealthState) -> Health {
    let mut g = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = Health { temps: read_temps(&mut g.comps), ..Default::default() };
    platform::fill(&mut h);
    let fresh = g.storage.as_ref().is_some_and(|(t, _, _)| t.elapsed().as_secs() < 60);
    if !fresh {
        let (disks, note) = platform::storage(&h.temps);
        g.storage = Some((Instant::now(), disks, note));
    }
    if let Some((_, d, n)) = &g.storage {
        h.storage = d.clone();
        h.storage_note = n.clone();
    }
    if h.temps.ram.is_empty() {
        h.ram_temp_note = Some("Memory temperature isn't exposed by this computer's sensors.".into());
    }
    if let Some(b) = &h.battery {
        h.history = record_day(&g.history_file, b);
        h.projection = project(&h.history, b);
        if let (Some(p), Some(w)) = (b.percent, b.power_w) {
            let at = now();
            if g.drain.last().is_none_or(|s| at >= s.at + 30) {
                g.drain.push(DrainSample { at, percent: p, power_w: w });
                // Two hours at one sample per 30 s.
                if g.drain.len() > 240 {
                    g.drain.remove(0);
                }
            }
        }
        h.drain = g.drain.clone();
    }
    h
}

// ---------------------------------------------------------------------------
// macOS

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use plist::{Dictionary, Value};

    fn ioreg(args: &[&str]) -> Option<Value> {
        let out = std::process::Command::new("/usr/sbin/ioreg").args(args).output().ok()?;
        Value::from_reader_xml(&out.stdout[..]).ok()
    }

    fn int(d: &Dictionary, k: &str) -> Option<i64> {
        match d.get(k)? {
            // ioreg prints negative values as huge unsigned numbers.
            Value::Integer(i) => i.as_signed().or_else(|| i.as_unsigned().map(|u| u as i64)),
            _ => None,
        }
    }
    fn boolean(d: &Dictionary, k: &str) -> bool {
        d.get(k).and_then(Value::as_boolean).unwrap_or(false)
    }

    pub fn fill(h: &mut Health) {
        battery(h);
        h.fans = smc::fans();
        gpu(h);
    }

    fn battery(h: &mut Health) {
        let Some(list) = ioreg(&["-rn", "AppleSmartBattery", "-a"]) else {
            h.battery_note = Some("Couldn't read battery information.".into());
            return;
        };
        let Some(d) = list.as_array().and_then(|a| a.first()).and_then(Value::as_dictionary) else {
            h.battery_note = Some("This Mac has no battery.".into());
            h.power.note = Some("No battery or adapter details on this Mac.".into());
            return;
        };
        let installed = boolean(d, "BatteryInstalled");
        let plugged = boolean(d, "ExternalConnected");
        let mv = int(d, "Voltage").map(|v| v as f32 / 1000.0);
        let ma = int(d, "InstantAmperage").or_else(|| int(d, "Amperage")).map(|a| a as f32 / 1000.0);
        let design = int(d, "DesignCapacity").map(|v| v as f32);
        let max = int(d, "AppleRawMaxCapacity").or_else(|| int(d, "NominalChargeCapacity")).map(|v| v as f32);
        let minutes = |k: &str| int(d, k).filter(|m| *m > 0 && *m < 65535).map(|m| m as u32);
        let charging = boolean(d, "IsCharging");
        if installed {
            let health = match (max, design) {
                (Some(m), Some(d)) if d > 0.0 => Some(m / d * 100.0),
                _ => None,
            };
            let condition = if int(d, "PermanentFailureStatus").unwrap_or(0) != 0 {
                "Service recommended (battery reports a permanent failure)".to_string()
            } else if health.is_some_and(|x| x < 80.0) {
                "Service recommended (below 80% of design capacity)".to_string()
            } else {
                "Normal".to_string()
            };
            h.battery = Some(Battery {
                percent: int(d, "CurrentCapacity").map(|v| v as f32),
                charging,
                plugged_in: plugged,
                fully_charged: boolean(d, "FullyCharged"),
                cycles: int(d, "CycleCount").map(|v| v as u32),
                design_cycles: int(d, "DesignCycleCount9C").map(|v| v as u32),
                design_mah: design,
                max_mah: max,
                health_pct: health,
                condition: Some(condition),
                temp_c: int(d, "Temperature").map(|t| t as f32 / 100.0),
                voltage_v: mv,
                current_a: ma,
                power_w: match (mv, ma) {
                    (Some(v), Some(a)) => Some(v * a),
                    _ => None,
                },
                minutes_to_empty: if !plugged { minutes("AvgTimeToEmpty").or(minutes("TimeRemaining")) } else { None },
                minutes_to_full: if charging { minutes("AvgTimeToFull").or(minutes("TimeRemaining")) } else { None },
            });
        } else {
            h.battery_note = Some("No battery is installed.".into());
        }
        let telemetry = d.get("PowerTelemetryData").and_then(Value::as_dictionary);
        let adapter = d.get("AdapterDetails").and_then(Value::as_dictionary);
        h.power = Power {
            adapter_connected: plugged,
            adapter_name: adapter.and_then(|a| a.get("Name")).and_then(Value::as_string).map(str::to_string),
            adapter_watts: adapter.and_then(|a| int(a, "Watts")).filter(|_| plugged).map(|w| w as f32),
            adapter_voltage_v: adapter.and_then(|a| int(a, "AdapterVoltage")).filter(|_| plugged).map(|v| v as f32 / 1000.0),
            adapter_current_a: adapter.and_then(|a| int(a, "Current")).filter(|_| plugged).map(|v| v as f32 / 1000.0),
            system_power_w: telemetry
                .and_then(|t| int(t, "SystemLoad").or_else(|| int(t, "SystemPowerIn")))
                .filter(|w| *w > 0)
                .map(|w| w as f32 / 1000.0)
                .or_else(|| h.battery.as_ref().and_then(|b| b.power_w).filter(|w| *w < 0.0).map(|w| -w)),
            note: None,
        };
    }

    fn gpu(h: &mut Health) {
        let Some(list) = ioreg(&["-r", "-c", "IOAccelerator", "-d", "1", "-a"]) else {
            h.gpu.note = Some("Couldn't read GPU statistics.".into());
            return;
        };
        let Some(d) = list.as_array().and_then(|a| a.first()).and_then(Value::as_dictionary) else {
            h.gpu.note = Some("No GPU statistics on this Mac.".into());
            return;
        };
        h.gpu.name = d.get("model").and_then(|m| match m {
            Value::String(s) => Some(s.clone()),
            Value::Data(b) => Some(String::from_utf8_lossy(b).trim_end_matches('\0').to_string()),
            _ => None,
        });
        h.gpu.utilization = d
            .get("PerformanceStatistics")
            .and_then(Value::as_dictionary)
            .and_then(|p| int(p, "Device Utilization %"))
            .map(|v| v as f32);
        if h.gpu.utilization.is_none() {
            h.gpu.note = Some("This GPU doesn't report its utilization.".into());
        }
    }

    pub fn storage(temps: &Temps) -> (Vec<Disk>, Option<String>) {
        let out = std::process::Command::new("/usr/sbin/diskutil").args(["info", "-plist", "disk0"]).output();
        let Some(d) = out.ok().and_then(|o| Value::from_reader_xml(&o.stdout[..]).ok()).and_then(|v| v.into_dictionary()) else {
            return (Vec::new(), Some("Couldn't read the internal disk's details.".into()));
        };
        let s = |k: &str| d.get(k).and_then(Value::as_string).filter(|s| !s.is_empty()).map(str::to_string);
        let mut disk = Disk {
            name: s("MediaName").or_else(|| s("IORegistryEntryName")).unwrap_or_else(|| "Internal disk".into()),
            smart_status: s("SMARTStatus"),
            temp_c: temps.storage.iter().map(|t| t.celsius).reduce(f32::max),
            ..Default::default()
        };
        if !super::smartctl(&mut disk, "disk0") {
            disk.note = Some(
                "Wear and data written live in the NVMe SMART log, which macOS only hands to tools like smartctl (brew install smartmontools); install it and they'll appear here.".into(),
            );
        }
        (vec![disk], None)
    }

    /// Fan speeds from the System Management Controller (no admin needed).
    mod smc {
        use super::super::{Fan, Fans};
        use std::ffi::{CString, c_char, c_void};

        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct Vers {
            major: u8,
            minor: u8,
            build: u8,
            reserved: u8,
            release: u16,
        }
        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct PLimit {
            version: u16,
            length: u16,
            cpu: u32,
            gpu: u32,
            mem: u32,
        }
        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct KeyInfo {
            data_size: u32,
            data_type: u32,
            data_attributes: u8,
        }
        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct KeyData {
            key: u32,
            vers: Vers,
            p_limit: PLimit,
            key_info: KeyInfo,
            result: u8,
            status: u8,
            data8: u8,
            data32: u32,
            bytes: [u8; 32],
        }

        const KERNEL_INDEX_SMC: u32 = 2;
        const READ_BYTES: u8 = 5;
        const READ_KEYINFO: u8 = 9;

        #[link(name = "IOKit", kind = "framework")]
        unsafe extern "C" {
            fn IOServiceMatching(name: *const c_char) -> *mut c_void;
            fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
            fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connect: *mut u32) -> i32;
            fn IOServiceClose(connect: u32) -> i32;
            fn IOObjectRelease(obj: u32) -> i32;
            fn IOConnectCallStructMethod(
                connection: u32,
                selector: u32,
                input: *const c_void,
                input_size: usize,
                output: *mut c_void,
                output_size: *mut usize,
            ) -> i32;
        }

        struct Smc(u32);
        impl Drop for Smc {
            fn drop(&mut self) {
                unsafe { IOServiceClose(self.0) };
            }
        }

        fn fourcc(s: &str) -> u32 {
            s.bytes().fold(0, |a, b| (a << 8) | b as u32)
        }

        impl Smc {
            fn open() -> Option<Smc> {
                let name = CString::new("AppleSMC").ok()?;
                unsafe {
                    let service = IOServiceGetMatchingService(0, IOServiceMatching(name.as_ptr()));
                    if service == 0 {
                        return None;
                    }
                    let mut conn = 0u32;
                    #[allow(deprecated)]
                    let task = libc::mach_task_self();
                    let r = IOServiceOpen(service, task, 0, &mut conn);
                    IOObjectRelease(service);
                    (r == 0).then_some(Smc(conn))
                }
            }

            fn call(&self, input: &KeyData) -> Option<KeyData> {
                let mut out = KeyData::default();
                let mut size = std::mem::size_of::<KeyData>();
                let r = unsafe {
                    IOConnectCallStructMethod(
                        self.0,
                        KERNEL_INDEX_SMC,
                        input as *const _ as *const c_void,
                        std::mem::size_of::<KeyData>(),
                        &mut out as *mut _ as *mut c_void,
                        &mut size,
                    )
                };
                (r == 0 && out.result == 0).then_some(out)
            }

            /// Reads a key as a number, whatever its SMC type.
            fn read(&self, key: &str) -> Option<f32> {
                let info = self.call(&KeyData { key: fourcc(key), data8: READ_KEYINFO, ..Default::default() })?.key_info;
                let out = self.call(&KeyData { key: fourcc(key), key_info: info, data8: READ_BYTES, ..Default::default() })?;
                let b = &out.bytes;
                let ty = info.data_type.to_be_bytes();
                match &ty {
                    b"flt " => Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                    b"fpe2" => Some(u16::from_be_bytes([b[0], b[1]]) as f32 / 4.0),
                    b"ui8 " => Some(b[0] as f32),
                    b"ui16" => Some(u16::from_be_bytes([b[0], b[1]]) as f32),
                    b"ui32" => Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32),
                    _ => None,
                }
            }
        }

        pub fn fans() -> Fans {
            debug_assert_eq!(std::mem::size_of::<KeyData>(), 80);
            let Some(smc) = Smc::open() else {
                return Fans { fans: Vec::new(), note: Some("Couldn't open the System Management Controller.".into()) };
            };
            let n = smc.read("FNum").unwrap_or(0.0) as u32;
            if n == 0 {
                return Fans { fans: Vec::new(), note: Some("This Mac has no fans (it's cooled passively).".into()) };
            }
            let fans = (0..n)
                .filter_map(|i| {
                    Some(Fan {
                        index: i,
                        rpm: smc.read(&format!("F{i}Ac"))?,
                        min_rpm: smc.read(&format!("F{i}Mn")),
                        max_rpm: smc.read(&format!("F{i}Mx")),
                    })
                })
                .collect();
            Fans { fans, note: None }
        }
    }
}

// ---------------------------------------------------------------------------
// Linux

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn read(p: &Path) -> Option<String> {
        fs::read_to_string(p).ok().map(|s| s.trim().to_string())
    }
    fn num(p: &Path) -> Option<f64> {
        read(p)?.parse().ok()
    }

    pub fn fill(h: &mut Health) {
        power_supply(h);
        fans(h);
        gpu(h);
    }

    fn power_supply(h: &mut Health) {
        let Ok(rd) = fs::read_dir("/sys/class/power_supply") else {
            h.battery_note = Some("No power supply information (/sys/class/power_supply) on this system.".into());
            return;
        };
        let mut found_battery = false;
        for e in rd.flatten() {
            let p = e.path();
            let ty = read(&p.join("type")).unwrap_or_default();
            if ty == "Mains" || ty == "USB" {
                if read(&p.join("online")).as_deref() == Some("1") {
                    h.power.adapter_connected = true;
                    h.power.adapter_name = Some(e.file_name().to_string_lossy().into_owned());
                    h.power.adapter_voltage_v = num(&p.join("voltage_now")).map(|v| (v / 1e6) as f32);
                    h.power.adapter_current_a = num(&p.join("current_now")).map(|v| (v / 1e6) as f32);
                }
            } else if ty == "Battery" && !found_battery {
                found_battery = true;
                let status = read(&p.join("status")).unwrap_or_default();
                let v = num(&p.join("voltage_now")).map(|v| v / 1e6);
                // Energy (µWh) or charge (µAh) depending on the driver.
                let (full, design, unit_mah) = match (num(&p.join("charge_full")), num(&p.join("charge_full_design"))) {
                    (Some(f), Some(d)) => (Some(f / 1000.0), Some(d / 1000.0), true),
                    _ => (num(&p.join("energy_full")), num(&p.join("energy_full_design")), false),
                };
                let to_mah = |x: Option<f64>| {
                    if unit_mah { x } else { x.zip(num(&p.join("voltage_min_design")).map(|v| v / 1e6)).map(|(e, v)| e / 1000.0 / v) }
                };
                let power = num(&p.join("power_now")).map(|w| w / 1e6).or_else(|| num(&p.join("current_now")).zip(v).map(|(a, v)| a / 1e6 * v));
                let discharging = status == "Discharging";
                let signed_power = power.map(|w| if discharging { -w.abs() } else { w.abs() });
                let health = match (full, design) {
                    (Some(f), Some(d)) if d > 0.0 => Some((f / d * 100.0) as f32),
                    _ => None,
                };
                h.battery = Some(Battery {
                    percent: num(&p.join("capacity")).map(|c| c as f32),
                    charging: status == "Charging",
                    plugged_in: !discharging,
                    fully_charged: status == "Full",
                    cycles: num(&p.join("cycle_count")).filter(|c| *c > 0.0).map(|c| c as u32),
                    design_cycles: None,
                    design_mah: to_mah(design).map(|x| x as f32),
                    max_mah: to_mah(full).map(|x| x as f32),
                    health_pct: health,
                    condition: health.map(|x| if x < 80.0 { "Service recommended (below 80% of design capacity)".into() } else { "Normal".into() }),
                    temp_c: num(&p.join("temp")).map(|t| (t / 10.0) as f32),
                    voltage_v: v.map(|x| x as f32),
                    current_a: num(&p.join("current_now")).map(|a| ((a / 1e6) * if discharging { -1.0 } else { 1.0 }) as f32),
                    power_w: signed_power.map(|w| w as f32),
                    minutes_to_empty: None,
                    minutes_to_full: None,
                });
                if discharging {
                    h.power.system_power_w = power.map(|w| w as f32);
                }
            }
        }
        if !found_battery {
            h.battery_note = Some("No battery found.".into());
        }
        if !h.power.adapter_connected && h.battery.is_none() {
            h.power.note = Some("No adapter details are exposed on this system.".into());
        }
    }

    fn fans(h: &mut Health) {
        let mut fans = Vec::new();
        if let Ok(rd) = fs::read_dir("/sys/class/hwmon") {
            for e in rd.flatten() {
                for i in 1..=8 {
                    if let Some(rpm) = num(&e.path().join(format!("fan{i}_input"))) {
                        fans.push(Fan {
                            index: fans.len() as u32,
                            rpm: rpm as f32,
                            min_rpm: num(&e.path().join(format!("fan{i}_min"))).map(|x| x as f32),
                            max_rpm: num(&e.path().join(format!("fan{i}_max"))).map(|x| x as f32),
                        });
                    }
                }
            }
        }
        h.fans = if fans.is_empty() {
            Fans { fans, note: Some("No fan sensors are exposed (hwmon) on this system.".into()) }
        } else {
            Fans { fans, note: None }
        };
    }

    fn gpu(h: &mut Health) {
        if let Ok(rd) = fs::read_dir("/sys/class/drm") {
            for e in rd.flatten() {
                if let Some(u) = num(&e.path().join("device/gpu_busy_percent")) {
                    h.gpu.utilization = Some(u as f32);
                    h.gpu.name = Some(e.file_name().to_string_lossy().into_owned());
                    return;
                }
            }
        }
        h.gpu.note = Some("This GPU driver doesn't expose utilization without vendor tools.".into());
    }

    pub fn storage(temps: &Temps) -> (Vec<Disk>, Option<String>) {
        let mut disks = Vec::new();
        if let Ok(rd) = fs::read_dir("/sys/block") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") || name.starts_with("dm-") {
                    continue;
                }
                let model = read(&e.path().join("device/model")).unwrap_or_else(|| name.clone());
                let mut d = Disk { name: format!("{model} ({name})"), ..Default::default() };
                if name.starts_with("nvme") {
                    d.temp_c = temps.storage.iter().map(|t| t.celsius).reduce(f32::max);
                }
                if !super::smartctl(&mut d, &format!("/dev/{name}")) {
                    d.note = Some("SMART health needs root (sudo smartctl -a); not read.".into());
                }
                disks.push(d);
            }
        }
        (disks, None)
    }
}

// ---------------------------------------------------------------------------
// Windows

#[cfg(windows)]
mod platform {
    use super::*;
    use serde_json::Value;

    /// Runs a PowerShell snippet that prints JSON.
    fn ps(script: &str) -> Option<Value> {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        serde_json::from_slice(&out.stdout).ok()
    }

    fn f(v: &Value, k: &str) -> Option<f64> {
        v.get(k)?.as_f64()
    }

    pub fn fill(h: &mut Health) {
        let script = r#"
$b = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
$st = Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus -ErrorAction SilentlyContinue | Select-Object -First 1
$full = Get-CimInstance -Namespace root\wmi -ClassName BatteryFullChargedCapacity -ErrorAction SilentlyContinue | Select-Object -First 1
$static = Get-CimInstance -Namespace root\wmi -ClassName BatteryStaticData -ErrorAction SilentlyContinue | Select-Object -First 1
$cyc = Get-CimInstance -Namespace root\wmi -ClassName BatteryCycleCount -ErrorAction SilentlyContinue | Select-Object -First 1
$gpu = $null
try { $gpu = ((Get-Counter '\GPU Engine(*engtype_3D)\Utilization Percentage' -ErrorAction Stop).CounterSamples | Measure-Object CookedValue -Sum).Sum } catch {}
$gpuName = (Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | Select-Object -First 1).Name
@{ present = [bool]$b; percent = $b.EstimatedChargeRemaining; runtime = $b.EstimatedRunTime;
   online = $st.PowerOnline; charging = $st.Charging; voltage = $st.Voltage; chargeRate = $st.ChargeRate; dischargeRate = $st.DischargeRate;
   full = $full.FullChargedCapacity; design = $static.DesignedCapacity; cycles = $cyc.CycleCount;
   gpu = $gpu; gpuName = $gpuName } | ConvertTo-Json -Compress
"#;
        let Some(v) = ps(script) else {
            h.battery_note = Some("Couldn't query Windows for battery details.".into());
            return;
        };
        h.gpu.name = v.get("gpuName").and_then(Value::as_str).map(str::to_string);
        h.gpu.utilization = f(&v, "gpu").map(|x| x.min(100.0) as f32);
        if h.gpu.utilization.is_none() {
            h.gpu.note = Some("GPU performance counters aren't available.".into());
        }
        h.fans = Fans { fans: Vec::new(), note: Some("Windows doesn't expose fan speeds without vendor tools.".into()) };
        let online = v.get("online").and_then(Value::as_bool).unwrap_or(false);
        h.power.adapter_connected = online;
        if !v.get("present").and_then(Value::as_bool).unwrap_or(false) {
            h.battery_note = Some("No battery found.".into());
            h.power.note = Some("Adapter details aren't exposed on this PC.".into());
            return;
        }
        // root\wmi capacities are in mWh; voltage in mV; rates in mW.
        let voltage = f(&v, "voltage").filter(|x| *x > 0.0).map(|x| x / 1000.0);
        let full = f(&v, "full").filter(|x| *x > 0.0);
        let design = f(&v, "design").filter(|x| *x > 0.0);
        let to_mah = |mwh: Option<f64>| mwh.zip(voltage).map(|(e, v)| e / v);
        let charge = f(&v, "chargeRate").filter(|x| *x > 0.0);
        let discharge = f(&v, "dischargeRate").filter(|x| *x > 0.0);
        let power = charge.map(|c| c / 1000.0).or(discharge.map(|d| -d / 1000.0));
        let health = full.zip(design).map(|(f, d)| (f / d * 100.0) as f32);
        h.battery = Some(Battery {
            percent: f(&v, "percent").map(|x| x as f32),
            charging: v.get("charging").and_then(Value::as_bool).unwrap_or(false),
            plugged_in: online,
            fully_charged: f(&v, "percent").is_some_and(|p| p >= 100.0),
            cycles: f(&v, "cycles").filter(|c| *c > 0.0).map(|c| c as u32),
            design_cycles: None,
            design_mah: to_mah(design).map(|x| x as f32),
            max_mah: to_mah(full).map(|x| x as f32),
            health_pct: health,
            condition: health.map(|x| if x < 80.0 { "Service recommended (below 80% of design capacity)".into() } else { "Normal".into() }),
            temp_c: None,
            voltage_v: voltage.map(|x| x as f32),
            current_a: power.zip(voltage).map(|(w, v)| (w / v) as f32),
            power_w: power.map(|x| x as f32),
            // 71582788 means "unknown / plugged in".
            minutes_to_empty: f(&v, "runtime").filter(|m| *m > 0.0 && *m < 71_582_788.0 && !online).map(|m| m as u32),
            minutes_to_full: None,
        });
        if !online {
            h.power.system_power_w = discharge.map(|d| (d / 1000.0) as f32);
        }
        h.power.note = Some("Windows doesn't report adapter wattage.".into());
    }

    pub fn storage(_temps: &Temps) -> (Vec<Disk>, Option<String>) {
        let Some(v) = ps("Get-PhysicalDisk | Select-Object FriendlyName, HealthStatus, MediaType | ConvertTo-Json -Compress") else {
            return (Vec::new(), Some("Couldn't query disks.".into()));
        };
        let list = match v {
            Value::Array(a) => a,
            other => vec![other],
        };
        let disks = list
            .iter()
            .map(|d| Disk {
                name: d.get("FriendlyName").and_then(Value::as_str).unwrap_or("Disk").to_string(),
                smart_status: d.get("HealthStatus").map(|s| match s {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => match n.as_u64() {
                        Some(0) => "Healthy".into(),
                        Some(1) => "Warning".into(),
                        Some(2) => "Unhealthy".into(),
                        _ => "Unknown".into(),
                    },
                    _ => "Unknown".into(),
                }),
                note: Some("Temperature and wear need administrator rights (Get-StorageReliabilityCounter).".into()),
                ..Default::default()
            })
            .collect();
        (disks, None)
    }
}

/// Fills temperature, wear and data written from smartctl's JSON output,
/// when smartctl is installed and allowed to read the disk.
#[cfg_attr(windows, allow(dead_code))]
fn smartctl(d: &mut Disk, dev: &str) -> bool {
    let candidates = ["smartctl", "/opt/homebrew/bin/smartctl", "/usr/local/bin/smartctl", "/usr/sbin/smartctl"];
    let Some(out) = candidates.iter().find_map(|c| std::process::Command::new(c).args(["-a", "-j", dev]).output().ok()) else {
        return false;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return false };
    let log = &v["nvme_smart_health_information_log"];
    if log.is_null() && v["smart_status"].is_null() {
        return false;
    }
    if let Some(p) = v["smart_status"]["passed"].as_bool() {
        d.smart_status.get_or_insert_with(|| if p { "Passed".into() } else { "Failing".into() });
    }
    d.temp_c = v["temperature"]["current"].as_f64().map(|t| t as f32).or(d.temp_c);
    d.wear_pct = log["percentage_used"].as_f64().map(|x| x as f32);
    // NVMe data units are 1000 × 512 bytes.
    d.data_written_bytes = log["data_units_written"].as_u64().map(|u| u * 512_000);
    d.power_on_hours = v["power_on_time"]["hours"].as_u64();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_sensor_labels_across_platforms() {
        for (l, k) in [
            ("PMU tdie3", "cpu"), ("PMU TP2g", "gpu"), ("PMU TP1s", "cpu"), ("NAND CH0 temp", "storage"),
            ("gas gauge battery", "battery"), ("Core 0", "cpu"), ("Package id 0", "cpu"), ("Tctl", "cpu"),
            ("edge", "gpu"), ("Composite", "storage"), ("PMU tcal", "other"), ("SODIMM", "ram"),
        ] {
            assert_eq!(classify(l), k, "{l}");
        }
    }

    fn batt(health: f32, cycles: u32) -> Battery {
        Battery { health_pct: Some(health), cycles: Some(cycles), max_mah: Some(health * 50.0), design_mah: Some(5000.0), ..Default::default() }
    }

    #[test]
    fn projects_from_cycles_then_from_trend() {
        // 81% after 820 cycles: 2.3% per 100 cycles, so 80% at ~863 cycles.
        let p = project(&[], &batt(81.2, 820)).unwrap();
        assert_eq!(p.date_80, None);
        assert!((860..=880).contains(&p.cycles_80.unwrap()), "{p:?}");
        // A month of readings losing 0.1% a day from 90%: 80% about 100 days after the first.
        let h: Vec<DailyRecord> = (0..30)
            .map(|i| DailyRecord { day: 20_000 + i, health_pct: 90.0 - 0.1 * i as f32, max_mah: 0.0, design_mah: 5000.0, cycles: 300 + i as u32 })
            .collect();
        let p = project(&h, &batt(87.1, 329)).unwrap();
        let day = p.date_80.unwrap() / 86_400;
        assert!((20_098..=20_102).contains(&day), "{p:?}");
        assert!(project(&[], &batt(79.0, 900)).unwrap().basis.contains("already"));
        assert!(project(&[], &batt(99.0, 10)).is_none());
    }

    #[test]
    fn reads_without_panicking_and_never_invents_values() {
        let st = HealthState::new();
        let h = read(&st);
        // Whatever this machine has, every missing section explains itself.
        assert!(h.battery.is_some() || h.battery_note.is_some());
        assert!(!h.fans.fans.is_empty() || h.fans.note.is_some());
        assert!(h.gpu.utilization.is_some() || h.gpu.note.is_some());
        if let Some(b) = &h.battery {
            if let Some(x) = b.health_pct {
                assert!((0.0..=150.0).contains(&x));
            }
        }
        if std::env::var_os("SHOW_HEALTH").is_some() {
            eprintln!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "temps": h.temps.cpu.len() + h.temps.gpu.len(), "fans": h.fans.fans.iter().map(|f| f.rpm).collect::<Vec<_>>(),
                "fans_note": h.fans.note, "gpu": h.gpu.utilization, "gpu_name": h.gpu.name,
                "battery": h.battery.as_ref().map(|b| (b.percent, b.health_pct, b.cycles, b.temp_c, b.power_w, b.condition.clone())),
                "power": (h.power.adapter_name.clone(), h.power.adapter_watts, h.power.system_power_w),
                "storage": h.storage.iter().map(|d| (d.name.clone(), d.smart_status.clone(), d.temp_c, d.note.clone())).collect::<Vec<_>>(),
                "projection": h.projection,
            })).unwrap());
        }
    }
}
