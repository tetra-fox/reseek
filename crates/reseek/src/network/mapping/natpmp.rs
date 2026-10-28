//! Asking the default gateway by PCP, or by NAT-PMP when it speaks only the older protocol. The
//! router picks the lease and may offer another outside port; a port other than ours is no use,
//! since other users are told ours, so it is given back.

use std::net::IpAddr;
use std::num::NonZeroU16;
use std::sync::Mutex;
use std::time::Duration;

use api_types::state::PortMappingMethod;
use async_trait::async_trait;
use crab_nat::{
  InternetProtocol, PortMapping, PortMappingOptions, PortMappingType, TimeoutConfig, natpmp,
};
use tracing::debug;

use super::{Gateway, Mapped, local_toward};
use crate::sync::lock;

/// How long each request waits for the router, doubling on each of the retries: about four
/// seconds for each protocol in all.
const TIMEOUTS: TimeoutConfig = TimeoutConfig {
  initial_timeout: Duration::from_millis(250),
  max_retries: 3,
  max_retry_timeout: Some(Duration::from_secs(2)),
};

/// The default gateway.
#[derive(Default)]
pub struct NatPmp {
  held: Mutex<Option<(PortMapping, Mapped)>>,
}

/// The address of the default gateway.
async fn default_gateway() -> Result<IpAddr, String> {
  let found = tokio::task::spawn_blocking(netdev::get_default_gateway)
    .await
    .map_err(|e| format!("the default gateway was not looked up: {e}"))?
    .map_err(|e| format!("no default gateway: {e}"))?;
  found
    .ipv4
    .first()
    .copied()
    .map(IpAddr::V4)
    .or_else(|| found.ipv6.first().copied().map(IpAddr::V6))
    .ok_or_else(|| "the default gateway has no address".to_string())
}

/// What `mapping` gives, with our external address asked of `router` by NAT-PMP when the
/// mapping did not say it.
async fn mapped(mapping: &PortMapping, router: IpAddr) -> Mapped {
  let (method, external_address) = match mapping.mapping_type() {
    PortMappingType::Pcp { external_ip, .. } => (PortMappingMethod::Pcp, Some(external_ip)),
    PortMappingType::NatPmp => {
      let address = match natpmp::external_address(router.into(), Some(TIMEOUTS)).await {
        Ok(address) => Some(IpAddr::V4(address)),
        Err(e) => {
          debug!(error = %e, "the NAT-PMP router did not say our external address");
          None
        }
      };
      (PortMappingMethod::Natpmp, address)
    }
  };
  Mapped {
    method,
    external_address,
    external_port: mapping.external_port().get(),
    lifetime: Some(Duration::from_secs(mapping.lifetime().into())),
  }
}

#[async_trait]
impl Gateway for NatPmp {
  async fn map(&self, ip: IpAddr, port: u16) -> Result<Mapped, String> {
    let router = default_gateway().await?;
    let client = local_toward(router, ip)?;
    let internal = NonZeroU16::new(port).ok_or("port 0 cannot be forwarded")?;
    let options = PortMappingOptions {
      external_port: Some(internal),
      lifetime_seconds: None,
      timeout_config: Some(TIMEOUTS),
    };
    let mapping = PortMapping::new(
      router.into(),
      client,
      InternetProtocol::Tcp,
      internal,
      options,
    )
    .await
    .map_err(|e| format!("no PCP or NAT-PMP answer from {router}: {e}"))?;
    let offered = mapping.external_port().get();
    if offered != port {
      if let Err((e, _)) = mapping.try_drop().await {
        debug!(error = %e, offered, "the port the router offered was not given back");
      }
      return Err(format!(
        "the router at {router} offered port {offered}, not {port}"
      ));
    }
    let made = mapped(&mapping, router).await;
    *lock(&self.held) = Some((mapping, made.clone()));
    Ok(made)
  }

  async fn renew(&self) -> Result<Mapped, String> {
    let (mut mapping, made) = lock(&self.held).clone().ok_or("nothing is mapped")?;
    mapping
      .renew()
      .await
      .map_err(|e| format!("the router did not renew the mapping: {e}"))?;
    let renewed = Mapped {
      lifetime: Some(Duration::from_secs(mapping.lifetime().into())),
      ..made
    };
    *lock(&self.held) = Some((mapping, renewed.clone()));
    Ok(renewed)
  }

  async fn unmap(&self) -> Result<(), String> {
    let (mapping, _) = lock(&self.held).take().ok_or("nothing is mapped")?;
    mapping
      .try_drop()
      .await
      .map_err(|(e, _)| format!("the router did not remove the mapping: {e}"))
  }
}
