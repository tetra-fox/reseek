//! What the tests of this crate and of the crates using it share.

/// What `f` logged on this thread while it ran, one line per event as the text sink writes it,
/// without color; for tests whose behavior is a log line.
pub fn logged<T>(f: impl FnOnce() -> T) -> (T, String) {
  #[derive(Clone, Default)]
  struct Lines(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
  impl std::io::Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
      crate::lock(&self.0).extend_from_slice(bytes);
      Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
      Ok(())
    }
  }
  let lines = Lines::default();
  let writer = lines.clone();
  let subscriber = tracing_subscriber::fmt()
    .with_writer(move || writer.clone())
    .with_ansi(false)
    .with_max_level(tracing::Level::TRACE)
    .finish();
  let result = tracing::subscriber::with_default(subscriber, f);
  let text = String::from_utf8_lossy(&crate::lock(&lines.0)).into_owned();
  (result, text)
}
