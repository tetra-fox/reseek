//! The families read from the system each time the metrics are asked for: this process, the
//! host's memory, fixed filesystems and network devices, and the async runtime.

use std::sync::Mutex;

use sysinfo::{
  Disks, MemoryRefreshKind, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System,
};
use tokio::runtime::Handle;

use super::registry::{Family, Labels, Values};
use crate::sync::lock;

/// Filesystems that live in memory or are layered over others, which are left out.
const NOT_FIXED: &[&str] = &["tmpfs", "overlay", "devtmpfs", "ramfs", "squashfs"];

/// What the system families are read from, kept between reads so each read only refreshes.
pub struct SystemProbe {
  state: Mutex<Probed>,
  runtime: Option<Handle>,
}

struct Probed {
  system: System,
  disks: Disks,
  networks: Networks,
  pid: Option<Pid>,
}

impl Default for SystemProbe {
  /// A probe of this process, and of the async runtime it is made on when there is one.
  fn default() -> Self {
    Self {
      state: Mutex::new(Probed {
        system: System::new(),
        disks: Disks::new_with_refreshed_list(),
        networks: Networks::new_with_refreshed_list(),
        pid: sysinfo::get_current_pid().ok(),
      }),
      runtime: Handle::try_current().ok(),
    }
  }
}

impl SystemProbe {
  /// The system families as they are now. Reads `/proc` and the like, so it blocks.
  pub fn families(&self) -> Vec<Family> {
    let mut probed = lock(&self.state);
    let Probed {
      system,
      disks,
      networks,
      pid,
    } = &mut *probed;
    let mut families = Vec::new();
    if let Some(pid) = *pid {
      system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
      );
      if let Some(process) = system.process(pid) {
        families.push(counter(
          "process_cpu_seconds_total",
          "Total user and system CPU time spent in seconds.",
          process.accumulated_cpu_time() as f64 / 1000.0,
        ));
        families.push(gauge(
          "process_resident_memory_bytes",
          "Resident memory size in bytes.",
          process.memory() as f64,
        ));
        families.push(gauge(
          "process_virtual_memory_bytes",
          "Virtual memory size in bytes.",
          process.virtual_memory() as f64,
        ));
        families.push(gauge(
          "process_start_time_seconds",
          "Start time of the process since unix epoch in seconds.",
          process.start_time() as f64,
        ));
      }
    }
    if let Some(open) = open_fds() {
      families.push(gauge(
        "process_open_fds",
        "Number of open file descriptors.",
        open as f64,
      ));
    }
    system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
    for (field, value) in [
      ("MemTotal", system.total_memory()),
      ("MemAvailable", system.available_memory()),
      ("MemFree", system.free_memory()),
    ] {
      families.push(gauge(
        &format!("node_memory_{field}_bytes"),
        &format!("Memory information field {field}_bytes."),
        value as f64,
      ));
    }
    disks.refresh(true);
    let mut available = Vec::new();
    let mut size = Vec::new();
    for disk in disks.list() {
      let fstype = disk.file_system().to_string_lossy().into_owned();
      if disk.is_removable() || NOT_FIXED.contains(&fstype.as_str()) {
        continue;
      }
      let labels = Labels::from([
        (
          "device".to_string(),
          disk.name().to_string_lossy().into_owned(),
        ),
        ("fstype".to_string(), fstype),
        (
          "mountpoint".to_string(),
          disk.mount_point().display().to_string(),
        ),
      ]);
      available.push((labels.clone(), disk.available_space() as f64));
      size.push((labels, disk.total_space() as f64));
    }
    families.push(Family {
      name: "node_filesystem_avail_bytes".into(),
      help: "Filesystem space available to non-root users in bytes.".into(),
      values: Values::Gauge(available),
    });
    families.push(Family {
      name: "node_filesystem_size_bytes".into(),
      help: "Filesystem size in bytes.".into(),
      values: Values::Gauge(size),
    });
    networks.refresh(true);
    let mut received = Vec::new();
    let mut transmitted = Vec::new();
    for (device, data) in networks.list() {
      let labels = Labels::from([("device".to_string(), device.clone())]);
      received.push((labels.clone(), data.total_received() as f64));
      transmitted.push((labels, data.total_transmitted() as f64));
    }
    families.push(Family {
      name: "node_network_receive_bytes_total".into(),
      help: "Network device statistic receive_bytes.".into(),
      values: Values::Counter(received),
    });
    families.push(Family {
      name: "node_network_transmit_bytes_total".into(),
      help: "Network device statistic transmit_bytes.".into(),
      values: Values::Counter(transmitted),
    });
    if let Some(runtime) = &self.runtime {
      let metrics = runtime.metrics();
      families.push(gauge(
        "tokio_workers",
        "The number of worker threads of the async runtime.",
        metrics.num_workers() as f64,
      ));
      families.push(gauge(
        "tokio_alive_tasks",
        "The number of tasks alive in the async runtime.",
        metrics.num_alive_tasks() as f64,
      ));
      families.push(gauge(
        "tokio_global_queue_depth",
        "The number of tasks waiting in the async runtime's global queue.",
        metrics.global_queue_depth() as f64,
      ));
    }
    families
  }
}

fn counter(name: &str, help: &str, value: f64) -> Family {
  Family {
    name: name.into(),
    help: help.into(),
    values: Values::Counter(vec![(Labels::new(), value)]),
  }
}

fn gauge(name: &str, help: &str, value: f64) -> Family {
  Family {
    name: name.into(),
    help: help.into(),
    values: Values::Gauge(vec![(Labels::new(), value)]),
  }
}

/// How many files this process has open, where the system lists them.
fn open_fds() -> Option<usize> {
  if cfg!(target_os = "linux") {
    std::fs::read_dir("/proc/self/fd").ok().map(Iterator::count)
  } else {
    None
  }
}
