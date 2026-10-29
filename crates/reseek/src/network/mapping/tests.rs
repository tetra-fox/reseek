use std::net::Ipv4Addr;

use api_types::state::Reachable;

use super::*;
use crate::testing;

/// A router that answers as it is told and notes every request.
#[derive(Clone)]
struct Fake {
  name: &'static str,
  /// what mapping answers; `Err` refuses
  answer: Result<Mapped, String>,
  /// whether renewals are refused
  renewals_refused: bool,
  asked: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Gateway for Fake {
  async fn map(&self, _: IpAddr, port: u16) -> Result<Mapped, String> {
    lock(&self.asked).push(format!("{} map {port}", self.name));
    self.answer.clone().map(|mapped| Mapped {
      external_port: port,
      ..mapped
    })
  }

  async fn renew(&self) -> Result<Mapped, String> {
    lock(&self.asked).push(format!("{} renew", self.name));
    if self.renewals_refused {
      return Err("lease lost".into());
    }
    self.answer.clone()
  }

  async fn unmap(&self) -> Result<(), String> {
    lock(&self.asked).push(format!("{} unmap", self.name));
    Ok(())
  }
}

fn mapped(lifetime: u64) -> Mapped {
  Mapped {
    method: PortMappingMethod::Pcp,
    external_address: Some(Ipv4Addr::new(203, 0, 113, 7).into()),
    external_port: 50300,
    lifetime: Some(Duration::from_secs(lifetime)),
  }
}

/// A mapper over `routers` with port mapping on for port 50300, with its state, its
/// reachability judgment and the requests the routers saw.
struct Rig {
  mapper: PortMapper,
  state: StateOwner,
  reachability: Arc<Reachability>,
  requests: Arc<Mutex<Vec<String>>>,
  /// the count of peers reaching us, which the judgment follows while it is held
  _inbound: watch::Sender<u64>,
}

fn rig(routers: Vec<Fake>) -> Rig {
  let requests = Arc::new(Mutex::new(Vec::new()));
  let gateways = routers
    .into_iter()
    .map(|router| {
      Box::new(Fake {
        asked: requests.clone(),
        ..router
      }) as Box<dyn Gateway>
    })
    .collect();
  let state = testing::state();
  let (inbound, counted) = watch::channel(0);
  let reachability = Arc::new(Reachability::new(state.clone(), counted));
  let mapper = PortMapper::new(gateways, true, Ipv4Addr::UNSPECIFIED.into(), 50300);
  mapper.start(state.clone(), reachability.clone());
  Rig {
    mapper,
    state,
    reachability,
    requests,
    _inbound: inbound,
  }
}

fn router(name: &'static str, answer: Result<Mapped, String>) -> Fake {
  Fake {
    name,
    answer,
    renewals_refused: false,
    asked: Arc::default(),
  }
}

/// Lets the mapping task run until it waits again.
async fn settle() {
  for _ in 0..10 {
    tokio::task::yield_now().await;
  }
}

fn mapping(state: &StateOwner) -> PortMappingState {
  state.current().server.listen.port_mapping.clone()
}

fn asked(asked: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
  lock(asked).clone()
}

#[tokio::test(start_paused = true)]
async fn nothing_is_asked_until_logged_in_and_a_mapping_is_renewed_at_half_its_lease() {
  let Rig {
    mapper,
    state,
    requests,
    ..
  } = rig(vec![router("pcp", Ok(mapped(600)))]);
  settle().await;
  assert!(asked(&requests).is_empty());
  assert_eq!(mapping(&state).state, PortMappingStatus::Off);

  mapper.logged_in(true);
  settle().await;
  let held = mapping(&state);
  assert_eq!(held.state, PortMappingStatus::Mapped);
  assert_eq!(held.method, Some(PortMappingMethod::Pcp));
  assert_eq!(held.external_address.as_deref(), Some("203.0.113.7"));
  assert_eq!(held.external_port, Some(50300));
  assert_eq!(asked(&requests), ["pcp map 50300"]);

  tokio::time::sleep(Duration::from_secs(299)).await;
  assert_eq!(
    asked(&requests).len(),
    1,
    "not renewed before half the lease"
  );
  tokio::time::sleep(Duration::from_secs(2)).await;
  assert_eq!(asked(&requests), ["pcp map 50300", "pcp renew"]);
  tokio::time::sleep(Duration::from_secs(300)).await;
  assert_eq!(asked(&requests).len(), 3, "and again half a lease later");
  assert_eq!(mapping(&state).state, PortMappingStatus::Mapped);
}

#[tokio::test(start_paused = true)]
async fn a_new_port_is_mapped_in_place_of_the_old_and_an_orderly_stop_removes_it() {
  let Rig {
    mapper,
    state,
    requests,
    ..
  } = rig(vec![router("upnp", Ok(mapped(3600)))]);
  mapper.logged_in(true);
  settle().await;
  mapper.options(true, Ipv4Addr::UNSPECIFIED.into(), 50400);
  settle().await;
  assert_eq!(
    asked(&requests),
    ["upnp map 50300", "upnp unmap", "upnp map 50400"]
  );
  assert_eq!(mapping(&state).external_port, Some(50400));

  mapper.stop().await;
  assert_eq!(asked(&requests).last().unwrap(), "upnp unmap");
  assert_eq!(mapping(&state).state, PortMappingStatus::Off);
}

#[tokio::test(start_paused = true)]
async fn logging_out_or_turning_it_off_removes_the_mapping() {
  let Rig {
    mapper,
    state,
    requests,
    ..
  } = rig(vec![router("upnp", Ok(mapped(3600)))]);
  mapper.logged_in(true);
  settle().await;
  mapper.logged_in(false);
  settle().await;
  assert_eq!(mapping(&state).state, PortMappingStatus::Off);
  mapper.logged_in(true);
  settle().await;
  mapper.options(false, Ipv4Addr::UNSPECIFIED.into(), 50300);
  settle().await;
  assert_eq!(
    asked(&requests),
    [
      "upnp map 50300",
      "upnp unmap",
      "upnp map 50300",
      "upnp unmap"
    ]
  );
  assert_eq!(mapping(&state).state, PortMappingStatus::Off);
  mapper.stop().await;
  assert_eq!(asked(&requests).len(), 4, "nothing left to remove");
}

#[tokio::test(start_paused = true)]
async fn the_next_way_is_tried_and_when_none_works_the_reasons_are_kept() {
  let Rig {
    mapper,
    state,
    requests,
    ..
  } = rig(vec![
    router("upnp", Err("no UPnP router found".into())),
    router("pcp", Ok(mapped(3600))),
  ]);
  mapper.logged_in(true);
  settle().await;
  assert_eq!(asked(&requests), ["upnp map 50300", "pcp map 50300"]);
  assert_eq!(mapping(&state).method, Some(PortMappingMethod::Pcp));

  let failing = rig(vec![
    router("upnp", Err("no UPnP router found".into())),
    router("pcp", Err("the router refused: conflict".into())),
  ]);
  let (mapper, state) = (failing.mapper, failing.state);
  mapper.logged_in(true);
  settle().await;
  let failed = mapping(&state);
  assert_eq!(failed.state, PortMappingStatus::Failed);
  assert_eq!(
    failed.error.as_deref(),
    Some("no UPnP router found; the router refused: conflict")
  );
  assert_eq!((failed.method, failed.external_port), (None, None));
}

#[tokio::test(start_paused = true)]
async fn a_renewal_refused_asks_for_the_mapping_anew() {
  let Rig {
    mapper,
    state,
    requests,
    ..
  } = rig(vec![Fake {
    renewals_refused: true,
    ..router("pcp", Ok(mapped(600)))
  }]);
  mapper.logged_in(true);
  settle().await;
  tokio::time::sleep(Duration::from_secs(301)).await;
  assert_eq!(
    asked(&requests),
    ["pcp map 50300", "pcp renew", "pcp map 50300"]
  );
  assert_eq!(mapping(&state).state, PortMappingStatus::Mapped);
}

#[tokio::test(start_paused = true)]
async fn a_new_mapping_starts_the_reachability_judgment_over() {
  let Rig {
    mapper,
    state,
    reachability,
    _inbound,
    ..
  } = rig(vec![router("pcp", Ok(mapped(3600)))]);
  tokio::spawn(reachability.clone().follow(CancellationToken::new()));
  reachability.logged_in();
  for token in 0..5 {
    reachability.search_answered(token);
  }
  settle().await;
  assert_eq!(state.current().server.listen.reachable, Reachable::No);
  mapper.logged_in(true);
  settle().await;
  assert_eq!(
    state.current().server.listen.reachable,
    Reachable::Unknown,
    "the searches before the mapping no longer count"
  );
}
