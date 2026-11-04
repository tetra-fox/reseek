//! The daemon, with the web UI built in.

fn main() -> std::process::ExitCode {
  reseek::app::main(reseek_ui_assets::get)
}
