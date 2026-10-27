//! Asking the router to forward our peer port to this computer, so other users can connect to
//! us. While the option is on and we are logged in, each way of asking is tried in turn (UPnP,
//! then NAT-PMP or PCP at the default gateway) until one router forwards the port; the mapping
//! is renewed at half its lease, asked for again when the port or the listening address
//! changes, and removed when we log out, when the option is turned off and at an orderly stop.
//! Where it stands is `State.server.listen.portMapping`.

mod natpmp;
mod upnp;

use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use api_types::state::{PortMappingMethod, PortMappingState, PortMappingStatus};
use async_trait::async_trait;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

pub use natpmp::NatPmp;
pub use upnp::Upnp;

use super::reachability::Reachability;
use crate::state::StateOwner;

/// How long one request to a router may take before it counts as unanswered.
const ASKING: Duration = Duration::from_secs(15);

/// How long the stop waits for the mapping to be removed.
const STOPPING: Duration = Duration::from_secs(5);

/// The shortest wait before a renewal, whatever lease the router gave.
const SHORTEST_RENEWAL: Duration = Duration::from_secs(30);

/// What the mappings on the routers are named.
const DESCRIPTION: &str = "reseek";

/// A port the router forwards to us.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapped {
  /// how the router was asked
  pub method: PortMappingMethod,
  /// our address on the internet, when the router said
  pub external_address: Option<IpAddr>,
  /// the port the router forwards
  pub external_port: u16,
  /// how long the mapping holds unless renewed, `None` until removed
  pub lifetime: Option<Duration>,
}

/// One way of asking a router to forward a port. A gateway keeps the mapping it made, so it
/// can renew and remove it.
#[async_trait]
pub trait Gateway: Send + Sync {
  /// Asks for TCP `port` on the router's outside to reach `port` on this computer at `ip`, or
  /// at the address it reaches the router from when `ip` is unspecified. The error says why
  /// it did not.
  async fn map(&self, ip: IpAddr, port: u16) -> Result<Mapped, String>;

  /// Asks again for the mapping `map` made, for a new lease.
  async fn renew(&self) -> Result<Mapped, String>;

  /// Asks the router to forward the port no more.
  async fn unmap(&self) -> Result<(), String>;
}

/// What the mapping follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wanted {
  enabled: bool,
  logged_in: bool,
  ip: IpAddr,
  port: u16,
}

impl Wanted {
  /// The address and port to forward, when the port is to be forwarded at all.
  fn target(self) -> Option<(IpAddr, u16)> {
    (self.enabled && self.logged_in).then_some((self.ip, self.port))
  }
}

/// The mapping of our peer port on the router.
pub struct PortMapper {
  wanted: watch::Sender<Wanted>,
  /// the ways of asking, in the order they are tried, until the task takes them
  gateways: Mutex<Option<Vec<Box<dyn Gateway>>>>,
  stop: CancellationToken,
  task: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  // every change under these locks is a single take or put
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PortMapper {
  /// A mapper that asks through `gateways` in order, for `port` at `ip` while `enabled` and
  /// logged in; nothing is asked until [`PortMapper::start`].
  pub fn new(gateways: Vec<Box<dyn Gateway>>, enabled: bool, ip: IpAddr, port: u16) -> Self {
    Self {
      wanted: watch::Sender::new(Wanted {
        enabled,
        logged_in: false,
        ip,
        port,
      }),
      gateways: Mutex::new(Some(gateways)),
      stop: CancellationToken::new(),
      task: Mutex::new(None),
    }
  }

  /// Starts following what is wanted, writing where the mapping stands into `state` and
  /// starting `reachability`'s judgement over whenever the mapping changes.
  pub fn start(&self, state: StateOwner, reachability: Arc<Reachability>) {
    let Some(gateways) = lock(&self.gateways).take() else {
      return warn!("the port mapping was started already");
    };
    let task = tokio::spawn(
      Mapping {
        gateways,
        state,
        reachability,
        held: None,
        failed: None,
      }
      .run(self.wanted.subscribe(), self.stop.clone())
      .instrument(info_span!("port_mapping")),
    );
    *lock(&self.task) = Some(task);
  }

  /// We logged in, or the session ended.
  pub fn logged_in(&self, logged_in: bool) {
    self.wanted.send_if_modified(|wanted| {
      let changed = wanted.logged_in != logged_in;
      wanted.logged_in = logged_in;
      changed
    });
  }

  /// The options now ask for mapping when `enabled`, of `port` at `ip`.
  pub fn options(&self, enabled: bool, ip: IpAddr, port: u16) {
    self.wanted.send_if_modified(|wanted| {
      let next = Wanted {
        enabled,
        ip,
        port,
        ..*wanted
      };
      let changed = next != *wanted;
      *wanted = next;
      changed
    });
  }

  /// Removes the mapping at an orderly stop, waiting a few seconds at most for the router.
  pub async fn stop(&self) {
    self.stop.cancel();
    let task = lock(&self.task).take();
    let Some(task) = task else {
      return;
    };
    match tokio::time::timeout(STOPPING, task).await {
      Ok(Ok(())) => {}
      Ok(Err(e)) => warn!(error = %e, "the port mapping task ended badly"),
      Err(_) => {
        warn!("the port mapping was not removed in time; the router keeps it until its lease ends")
      }
    }
  }
}

/// The task's own state.
struct Mapping {
  gateways: Vec<Box<dyn Gateway>>,
  state: StateOwner,
  reachability: Arc<Reachability>,
  /// the gateway that holds the mapping, what it mapped, and the address and port it was for
  held: Option<(usize, Mapped, (IpAddr, u16))>,
  /// the reason the last attempt failed, so the same reason is not warned of again
  failed: Option<String>,
}

impl Mapping {
  async fn run(mut self, mut wanted: watch::Receiver<Wanted>, stop: CancellationToken) {
    loop {
      let target = wanted.borrow_and_update().target();
      if self
        .held
        .as_ref()
        .is_some_and(|(_, _, at)| Some(*at) != target)
      {
        self.unmap().await;
      }
      match target {
        Some(target) if self.held.is_none() => self.map(target).await,
        Some(_) => {}
        None => self.publish(off()),
      }
      let renewal = self.held.as_ref().and_then(|(_, mapped, _)| {
        let lifetime = mapped.lifetime?;
        Some((lifetime / 2).max(SHORTEST_RENEWAL))
      });
      tokio::select! {
        () = stop.cancelled() => {
          if self.held.is_some() {
            self.unmap().await;
          }
          return self.publish(off());
        }
        changed = wanted.changed() => if changed.is_err() { return },
        () = tokio::time::sleep(renewal.unwrap_or_default()), if renewal.is_some() => {
          self.renew().await;
        }
      }
    }
  }

  /// Asks each gateway in turn until one forwards `port` to `ip`.
  async fn map(&mut self, (ip, port): (IpAddr, u16)) {
    self.publish(PortMappingState {
      state: PortMappingStatus::Mapping,
      ..off()
    });
    let mut reasons = Vec::new();
    for (index, gateway) in self.gateways.iter().enumerate() {
      match asking(gateway.map(ip, port)).await {
        Ok(mapped) => {
          info!(
            method = ?mapped.method,
            external_address = ?mapped.external_address,
            external_port = mapped.external_port,
            lifetime = ?mapped.lifetime,
            "Port forwarded by the router"
          );
          self.failed = None;
          self.publish(mapped_state(&mapped));
          self.held = Some((index, mapped, (ip, port)));
          self.reachability.start_over("the port mapping changed");
          return;
        }
        Err(reason) => {
          debug!(reason, "the router did not forward the port this way");
          reasons.push(reason);
        }
      }
    }
    let reason = reasons.join("; ");
    if self.failed.as_deref() == Some(reason.as_str()) {
      debug!(port, reason, "Port not forwarded by the router");
    } else {
      warn!(port, reason, "Port not forwarded by the router");
    }
    self.publish(PortMappingState {
      state: PortMappingStatus::Failed,
      error: Some(reason.clone()),
      ..off()
    });
    self.failed = Some(reason);
  }

  /// Renews the mapping held; one the router will not renew is asked for anew.
  async fn renew(&mut self) {
    let Some((index, _, target)) = self.held.take() else {
      return;
    };
    match asking(self.gateways[index].renew()).await {
      Ok(mapped) => {
        debug!(lifetime = ?mapped.lifetime, "port mapping renewed");
        self.publish(mapped_state(&mapped));
        self.held = Some((index, mapped, target));
      }
      Err(reason) => {
        debug!(reason, "port mapping not renewed; asked for anew");
        self.reachability.start_over("the port mapping changed");
        self.map(target).await;
      }
    }
  }

  /// Removes the mapping held.
  async fn unmap(&mut self) {
    let Some((index, mapped, _)) = self.held.take() else {
      return;
    };
    match asking(self.gateways[index].unmap()).await {
      Ok(()) => info!(
        external_port = mapped.external_port,
        "Port no longer forwarded by the router"
      ),
      Err(reason) => warn!(
        external_port = mapped.external_port,
        reason, "port mapping not removed; the router keeps it until its lease ends"
      ),
    }
    self.reachability.start_over("the port mapping changed");
  }

  fn publish(&self, mapping: PortMappingState) {
    self
      .state
      .update(|state| state.server.listen.port_mapping = mapping);
  }
}

/// `request` with [`ASKING`] as its limit.
async fn asking<T>(request: impl Future<Output = Result<T, String>>) -> Result<T, String> {
  tokio::time::timeout(ASKING, request)
    .await
    .unwrap_or_else(|_| Err(format!("no answer within {} s", ASKING.as_secs())))
}

fn off() -> PortMappingState {
  PortMappingState {
    state: PortMappingStatus::Off,
    method: None,
    external_address: None,
    external_port: None,
    error: None,
  }
}

fn mapped_state(mapped: &Mapped) -> PortMappingState {
  PortMappingState {
    state: PortMappingStatus::Mapped,
    method: Some(mapped.method),
    external_address: mapped.external_address.map(|ip| ip.to_string()),
    external_port: Some(mapped.external_port),
    error: None,
  }
}

/// The address this computer reaches `router` from, or `ip` when it names one.
fn local_toward(router: IpAddr, ip: IpAddr) -> Result<IpAddr, String> {
  if !ip.is_unspecified() {
    return Ok(ip);
  }
  let unspecified: IpAddr = if router.is_ipv4() {
    std::net::Ipv4Addr::UNSPECIFIED.into()
  } else {
    std::net::Ipv6Addr::UNSPECIFIED.into()
  };
  // connecting a UDP socket sends nothing; it only picks the route and so the local address
  let socket = std::net::UdpSocket::bind((unspecified, 0))
    .and_then(|socket| socket.connect((router, 9)).map(|()| socket))
    .and_then(|socket| socket.local_addr())
    .map_err(|e| format!("no route to the router at {router}: {e}"))?;
  Ok(socket.ip())
}

#[cfg(test)]
mod tests;
