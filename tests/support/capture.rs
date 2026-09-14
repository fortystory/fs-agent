use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// A `Write` sink that keeps everything written to it, so a test can assert on
/// what a renderer produced. Cloning shares the same buffer.
#[derive(Clone, Default)]
pub struct CaptureBuf {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl CaptureBuf {
    pub fn text(&self) -> String {
        let bytes = self.inner.lock().expect("capture buffer poisoned");
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Write for CaptureBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner
            .lock()
            .expect("capture buffer poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
