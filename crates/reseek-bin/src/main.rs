//! The daemon, with the web UI built in.

mod allocator;

fn main() -> std::process::ExitCode {
  reseek::app::main(reseek_ui_assets::get)
}
