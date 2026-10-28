//! Asking a UPnP Internet Gateway Device: found by an SSDP search on the local network, it takes
//! a lease of an hour, or a permanent mapping when it takes no other kind.

use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;

use api_types::state::PortMappingMethod;
use async_trait::async_trait;
use igd_next::aio::Gateway as Router;
use igd_next::aio::tokio::{Tokio, search_gateway};
use igd_next::{AddPortError, PortMappingProtocol, SearchOptions};
use tracing::debug;

use super::{DESCRIPTION, Gateway, Mapped, local_toward};
use crate::sync::lock;

/// Where SSDP searches go: every UPnP device on the local network listens there.
const DISCOVERY: &str = "239.255.255.250:1900";

/// How long the search waits for a router to answer.
const SEARCHING: Duration = Duration::from_secs(3);

/// The lease asked for, in seconds.
const LEASE: u32 = 3600;

/// A mapping made, as it is renewed and removed.
#[derive(Clone)]
struct Held {
  router: Router<Tokio>,
  local: SocketAddr,
  /// the lease the router took, 0 for a permanent mapping
  lease: u32,
  mapped: Mapped,
}

/// The UPnP router of the local network.
pub struct Upnp {
  discovery: SocketAddr,
  held: Mutex<Option<Held>>,
}

impl Default for Upnp {
  fn default() -> Self {
    Self::searching_at(DISCOVERY.parse().expect("the SSDP address is an address"))
  }
}

impl Upnp {
  /// The router that answers SSDP searches sent to `discovery`.
  pub fn searching_at(discovery: SocketAddr) -> Self {
    Self {
      discovery,
      held: Mutex::new(None),
    }
  }

  fn held(&self) -> std::sync::MutexGuard<'_, Option<Held>> {
    lock(&self.held)
  }

  /// Asks `router` to forward `local`'s port to it, for `lease` seconds, or for good when it
  /// takes only that; returns the lease taken.
  async fn add(router: &Router<Tokio>, local: SocketAddr, lease: u32) -> Result<u32, AddPortError> {
    let tcp = PortMappingProtocol::TCP;
    match router
      .add_port(tcp, local.port(), local, lease, DESCRIPTION)
      .await
    {
      Ok(()) => Ok(lease),
      Err(AddPortError::OnlyPermanentLeasesSupported) => router
        .add_port(tcp, local.port(), local, 0, DESCRIPTION)
        .await
        .map(|()| 0),
      Err(e) => Err(e),
    }
  }
}

fn refused(error: AddPortError) -> String {
  format!("the UPnP router refused: {error}")
}

fn lifetime(lease: u32) -> Option<Duration> {
  (lease > 0).then(|| Duration::from_secs(lease.into()))
}

#[async_trait]
impl Gateway for Upnp {
  async fn map(&self, ip: IpAddr, port: u16) -> Result<Mapped, String> {
    let mut options = SearchOptions::default();
    options.broadcast_address = self.discovery;
    options.timeout = Some(SEARCHING);
    let router = search_gateway(options)
      .await
      .map_err(|e| format!("no UPnP router found: {e}"))?;
    let local = SocketAddr::new(local_toward(router.addr.ip(), ip)?, port);
    let lease = Self::add(&router, local, LEASE).await.map_err(refused)?;
    let external_address = match router.get_external_ip().await {
      Ok(address) => Some(address),
      Err(e) => {
        debug!(error = %e, "the UPnP router did not say our external address");
        None
      }
    };
    let mapped = Mapped {
      method: PortMappingMethod::Upnp,
      external_address,
      external_port: port,
      lifetime: lifetime(lease),
    };
    *self.held() = Some(Held {
      router,
      local,
      lease,
      mapped: mapped.clone(),
    });
    Ok(mapped)
  }

  async fn renew(&self) -> Result<Mapped, String> {
    let held = self.held().clone().ok_or("nothing is mapped")?;
    let lease = match Self::add(&held.router, held.local, held.lease).await {
      // some routers will not extend a mapping they hold and call it a conflict; the port holds
      // our own mapping, made less than a lease ago, so it is removed and made again
      Err(AddPortError::PortInUse) => {
        debug!("the UPnP router would not extend the mapping; made again");
        let removed = held
          .router
          .remove_port(PortMappingProtocol::TCP, held.local.port())
          .await;
        if let Err(e) = removed {
          debug!(error = %e, "the UPnP router did not remove the mapping before it was made again");
        }
        Self::add(&held.router, held.local, held.lease).await
      }
      added => added,
    }
    .map_err(refused)?;
    let mapped = Mapped {
      lifetime: lifetime(lease),
      ..held.mapped.clone()
    };
    *self.held() = Some(Held {
      lease,
      mapped: mapped.clone(),
      ..held
    });
    Ok(mapped)
  }

  async fn unmap(&self) -> Result<(), String> {
    let held = self.held().take().ok_or("nothing is mapped")?;
    held
      .router
      .remove_port(PortMappingProtocol::TCP, held.local.port())
      .await
      .map_err(|e| format!("the UPnP router did not remove the mapping: {e}"))
  }
}
