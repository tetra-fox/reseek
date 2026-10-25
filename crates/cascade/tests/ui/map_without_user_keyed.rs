use std::collections::BTreeMap;

#[derive(cascade::Cascade)]
struct Options {
  api_keys: BTreeMap<String, String>,
}

fn main() {}
