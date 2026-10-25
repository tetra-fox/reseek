//! The daemon without a web UI: the API only.

fn main() -> std::process::ExitCode {
  reseek::app::main(reseek::ui::Bundle::None)
}
