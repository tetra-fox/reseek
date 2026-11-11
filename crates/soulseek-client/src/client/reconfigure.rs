//! Changing the options while the client runs. The new options replace the old in one step; the
//! listener moves last, and if the new address cannot be bound the listener options go back to
//! what they were and the old listener keeps running.

use std::sync::Arc;

use super::Client;
use crate::error::ClientError;
use crate::options::ClientOptions;
use crate::patch::{self, ClientOptionsPatch};

impl Client {
  /// Applies `patch`. Returns whether some of the change only takes effect after reconnecting,
  /// which is never the case while disconnected. While connected the server is told the listen
  /// port, invitation setting and distributed status again. A listener that cannot be moved
  /// fails with a listen error and keeps the old listener options; the rest of the patch stays
  /// applied.
  pub async fn reconfigure_options(&self, patch: ClientOptionsPatch) -> Result<bool, ClientError> {
    self.inner.check_not_disposed()?;
    let _reconfiguring = self.inner.state_lock.lock().await;
    let old = self.inner.options();
    let new = Arc::new(patch.apply(&old)?);
    let connected = self.state().is_logged_in();
    let reconnect_required = connected && patch::reconnect_required(&old, &new);
    self.inner.set_options(new.clone());
    self
      .responder()
      .resize(new.limits.search_response_cache_size);
    self
      .inner
      .endpoints
      .resize(new.limits.user_endpoint_cache_size);
    self.transfers().reconfigure(&new);
    self.searches().resize(new.maximum_concurrent_searches);
    // the child limit and whether we take children may have changed
    self.distributed_command(crate::distributed::Command::Changed);

    let listener = if patch::listener_changed(&old, &new) {
      self.move_listener(&old, &new).await
    } else {
      Ok(())
    };

    if connected {
      self.send_configuration_messages().await.map_err(|error| {
        tracing::warn!(%error, "failed to send the reconfigured options to the server");
        ClientError::client(
          format!(
            "Failed to reconfigure options: {error}. Any successful reconfiguration has not been rolled back; retry with the same patch until successful or consider this as a fatal Exception"
          ),
          Some(error),
        )
      })?;
      if reconnect_required {
        tracing::warn!("some of the changed options take effect only after reconnecting");
      }
    }
    listener?;
    tracing::info!(reconnect_required, "options reconfigured");
    Ok(reconnect_required)
  }

  /// Stops or rebinds a running listener for the new options; a stopped listener is left to the
  /// next connect.
  async fn move_listener(
    &self,
    old: &ClientOptions,
    new: &Arc<ClientOptions>,
  ) -> Result<(), ClientError> {
    let running = self.inner.session().stage.listener().and_then(Option::take);
    let Some(running) = running else {
      return Ok(());
    };
    if !new.enable_listener {
      running.stop().await;
      return Ok(());
    }
    // bound next to the old one when the port differs; on the same port the old one must
    // close first, and comes back if the new one still cannot bind
    let (moved, error) = match self.start_listener(new) {
      Ok(listener) => {
        running.stop().await;
        (listener, None)
      }
      Err(_) if old.listen_port == new.listen_port => {
        running.stop().await;
        match self.start_listener(new) {
          Ok(listener) => (listener, None),
          Err(error) => match self.start_listener(old) {
            Ok(listener) => (listener, Some(error)),
            Err(restore) => {
              tracing::error!(%error, %restore, "the listener could not be moved or restored");
              self.revert_listener_options(old, new);
              return Err(error);
            }
          },
        }
      }
      Err(error) => (running, Some(error)),
    };
    if let Some(error) = &error {
      tracing::warn!(%error, "listener not moved; listener options reverted");
      self.revert_listener_options(old, new);
    }
    // a disconnect while the listener moved took the session; the listener goes with it
    let left = match self.inner.session().stage.listener() {
      Some(place) => place.replace(moved),
      None => Some(moved),
    };
    if let Some(left) = left {
      left.stop().await;
    }
    error.map_or(Ok(()), Err)
  }

  fn revert_listener_options(&self, old: &ClientOptions, new: &ClientOptions) {
    self.inner.set_options(Arc::new(ClientOptions {
      enable_listener: old.enable_listener,
      listen_ip_address: old.listen_ip_address,
      listen_port: old.listen_port,
      listen_backlog: old.listen_backlog,
      ..new.clone()
    }));
  }
}
