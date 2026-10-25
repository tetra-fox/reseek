//! The configuration as the API shows it: the settings page's view and the options document,
//! with secrets and URL credentials hidden.

use api_types::options as api;
use cascade::Loaded;
use serde_json::Value;

use super::{Options, redact};

/// The view of `loaded` for the settings page.
pub fn config_view(loaded: &Loaded<Options>) -> api::ConfigView {
  let view = cascade::ConfigView::new(loaded);
  let hidden = |mut value: Value| {
    redact::urls(&mut value);
    value
  };
  api::ConfigView {
    values: redact::values(loaded),
    fields: view
      .fields
      .into_iter()
      .map(|(key, field)| {
        let field = api::FieldView {
          source: source(field.source),
          file: field.file,
          env_var: field.env_var,
          cli_flag: field.cli_flag,
          kind: option_kind(field.kind),
          doc: field.doc,
          apply: apply(field.apply),
          secret: field.secret,
          not_applied: field.not_applied.map(|n| api::NotApplied {
            reason: n.reason,
            value: hidden(n.value),
          }),
        };
        (key, field)
      })
      .collect(),
    files: view
      .files
      .into_iter()
      .map(|file| api::FileView {
        name: file.name,
        path: file.path,
        revision: file.revision,
        writable: file.writable,
        read_only_reason: file.read_only_reason,
      })
      .collect(),
    pending_restart: view.pending_restart,
    pending_reconnect: view.pending_reconnect,
    managed_by: view.managed_by,
    warnings: view.warnings,
    load_error: view.load_error,
    overrides: view
      .overrides
      .into_iter()
      .map(|(key, o)| {
        let o = api::OverrideView {
          value: hidden(o.value),
          expires_at: o.expires_at,
          set_by: o.set_by,
          set_at: o.set_at,
        };
        (key, o)
      })
      .collect(),
  }
}

/// Every option's effective value with the layer that set it, one `key=value (source)` line per
/// key in key order, secrets and URL credentials hidden as in every other view.
pub fn debug_view(loaded: &Loaded<Options>) -> String {
  let values = redact::values(loaded);
  let mut text = String::new();
  for (key, source) in &loaded.sources {
    let value = key
      .split('.')
      .try_fold(&values, |value, part| value.get(part))
      .map_or_else(|| "null".to_string(), Value::to_string);
    let source = match source {
      cascade::Source::Default => "default".to_string(),
      cascade::Source::File { file } => format!("file {file}"),
      cascade::Source::Env { var } => format!("environment {var}"),
      cascade::Source::Cli { flag } => format!("command line {flag}"),
      cascade::Source::Runtime { set_by, .. } => format!("runtime override by {set_by}"),
    };
    text.push_str(&format!("{key}={value} ({source})\n"));
  }
  text
}

/// The layer a value came from, as the API writes it.
pub fn source(source: cascade::Source) -> api::OptionSource {
  match source {
    cascade::Source::Default => api::OptionSource::Default,
    cascade::Source::File { file } => api::OptionSource::File { file },
    cascade::Source::Env { var } => api::OptionSource::Env { var },
    cascade::Source::Cli { flag } => api::OptionSource::Cli { flag },
    cascade::Source::Runtime {
      expires_at,
      set_by,
      set_at,
    } => api::OptionSource::Runtime {
      expires_at,
      set_by,
      set_at,
    },
  }
}

fn option_kind(kind: cascade::Kind) -> api::OptionKind {
  match kind {
    cascade::Kind::Bool => api::OptionKind::Bool,
    cascade::Kind::Integer => api::OptionKind::Integer,
    cascade::Kind::Float => api::OptionKind::Float,
    cascade::Kind::String => api::OptionKind::String,
    cascade::Kind::Duration => api::OptionKind::Duration,
    cascade::Kind::Size => api::OptionKind::Size,
    cascade::Kind::List(item) => api::OptionKind::List(Box::new(option_kind(*item))),
    cascade::Kind::Map => api::OptionKind::Map,
    cascade::Kind::Table => api::OptionKind::Table,
  }
}

/// When a change takes effect, as the API writes it.
pub fn apply(apply: cascade::Apply) -> api::Apply {
  match apply {
    cascade::Apply::Live => api::Apply::Live,
    cascade::Apply::Reconnect => api::Apply::Reconnect,
    cascade::Apply::Restart => api::Apply::Restart,
  }
}

/// What a change did, by when each changed key takes effect.
pub fn changed(change: Option<&cascade::Change<Options>>) -> api::OptionsChanged {
  match change {
    Some(change) => api::OptionsChanged {
      applied: change.by_apply.live.clone(),
      reconnect_required: change.by_apply.reconnect.clone(),
      restart_required: change.by_apply.restart.clone(),
    },
    None => api::OptionsChanged::default(),
  }
}
