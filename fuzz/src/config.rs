//! A configuration with a field of every shape, for the configuration targets.

use std::collections::BTreeMap;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Options {
  #[cascade(env = "FUZZ_DEBUG", cli = "--debug", apply = "restart")]
  pub debug: bool,
  #[cascade(apply = "reconnect")]
  pub server: Server,
  #[cascade(env = "FUZZ_SHARED", cli = "--shared")]
  pub directories: Vec<String>,
  #[cascade(user_keyed)]
  pub keys: BTreeMap<String, Key>,
  pub hooks: Vec<Hook>,
}

#[derive(Clone, Default, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Server {
  #[cascade(env = "FUZZ_PORT")]
  pub port: u16,
  #[cascade(secret)]
  pub password: Option<String>,
  #[cascade(kind = "duration")]
  pub timeout: String,
  pub ratio: f64,
}

#[derive(Clone, Default, Serialize, Deserialize, Cascade)]
pub struct Key {
  #[cascade(secret)]
  pub key: String,
  #[serde(default)]
  pub role: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize, Cascade)]
pub struct Hook {
  pub url: String,
  #[cascade(secret)]
  pub token: Option<String>,
}
