#[derive(cascade::Cascade)]
struct Options {
  #[cascade(apply = "sometimes")]
  port: u16,
  #[cascade(cli = "debug")]
  debug: bool,
  #[cascade(env = "NOT-A-VAR")]
  name: String,
  #[cascade(kind = "color")]
  color: String,
}

fn main() {}
