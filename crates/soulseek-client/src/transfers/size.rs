//! Settling the size of a download when the uploader names one: an absent size or the same size
//! keeps ours, the caller's resolver settles any other, and without one the 0 that old 32-bit
//! clients report for a file of 2 GiB or more keeps ours while anything else aborts.

use std::sync::Arc;

use super::options::SizeMismatchResolver;
use super::record::Transfer;
use crate::error::{ClientError, ErrorSource};

/// Below this size an old 32-bit client reports its true size; at or above it, such a client
/// reports 0 for a file of 2 GiB or more
const OVERFLOWED_SIZE: u64 = i32::MAX as u64;

/// The size to download when the peer reports `remote`. No size from the peer, or the same
/// size, keeps ours; a resolver settles any other; without one, the 0 an old client reports for
/// a file of 2 GiB or more keeps ours, and anything else aborts.
pub(super) fn resolve_size(
  transfer: &Transfer,
  remote: Option<u64>,
  resolver: Option<&SizeMismatchResolver>,
) -> Result<u64, ClientError> {
  let local = transfer.size;
  let Some(remote) = remote.filter(|remote| *remote != local) else {
    return Ok(local);
  };
  let mismatch = |source: Option<ClientError>| ClientError::SizeMismatch {
    local,
    remote,
    source: source.map(|source| Arc::new(source) as ErrorSource),
  };
  match resolver {
    Some(resolve) => match resolve(transfer, remote) {
      Ok(size) if size >= transfer.start_offset => Ok(size),
      Ok(_) => Err(mismatch(None)),
      Err(error) => Err(mismatch(Some(error))),
    },
    None if remote == 0 && local >= OVERFLOWED_SIZE => Ok(local),
    None => Err(mismatch(None)),
  }
}

#[cfg(test)]
mod tests {
  use soulseek_proto::TransferDirection;

  use super::*;
  use crate::transfers::record::Record;

  fn transfer(size: u64, start_offset: u64) -> Transfer {
    Record::new(
      TransferDirection::Download,
      "bob",
      &"a.flac".into(),
      1,
      size,
      start_offset,
    )
    .snapshot()
  }

  #[test]
  fn an_absent_or_equal_size_keeps_ours() {
    assert_eq!(resolve_size(&transfer(100, 0), None, None).unwrap(), 100);
    assert_eq!(
      resolve_size(&transfer(100, 0), Some(100), None).unwrap(),
      100
    );
  }

  #[test]
  fn zero_from_an_old_client_keeps_ours_only_from_two_gibibytes() {
    let at_limit = transfer(OVERFLOWED_SIZE, 0);
    assert_eq!(
      resolve_size(&at_limit, Some(0), None).unwrap(),
      OVERFLOWED_SIZE
    );
    let below = transfer(OVERFLOWED_SIZE - 1, 0);
    assert!(matches!(
      resolve_size(&below, Some(0), None),
      Err(ClientError::SizeMismatch { remote: 0, .. })
    ));
  }

  #[test]
  fn another_size_aborts_unless_the_resolver_settles_it() {
    assert!(matches!(
      resolve_size(&transfer(100, 0), Some(120), None),
      Err(ClientError::SizeMismatch {
        local: 100,
        remote: 120,
        ..
      })
    ));
    let take_remote: SizeMismatchResolver = Arc::new(|_, remote| Ok(remote));
    assert_eq!(
      resolve_size(&transfer(100, 0), Some(120), Some(&take_remote)).unwrap(),
      120
    );
    // a size below the start offset, or an error, still aborts
    let too_small: SizeMismatchResolver = Arc::new(|_, _| Ok(10));
    assert!(resolve_size(&transfer(100, 50), Some(120), Some(&too_small)).is_err());
    let refuse: SizeMismatchResolver = Arc::new(|_, _| Err(ClientError::transfer("no")));
    assert!(matches!(
      resolve_size(&transfer(100, 0), Some(120), Some(&refuse)),
      Err(ClientError::SizeMismatch {
        source: Some(_),
        ..
      })
    ));
  }
}
