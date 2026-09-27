use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// 一个 `Write` sink：把写进它的一切都留着，于是测试能断言某个
/// 渲染器产出了什么。克隆共享同一块缓冲区。
#[derive(Clone, Default)]
pub struct CaptureBuf {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl CaptureBuf {
    pub fn text(&self) -> String {
        let bytes = self.inner.lock().expect("捕获缓冲区已中毒");
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Write for CaptureBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner
            .lock()
            .expect("捕获缓冲区已中毒")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
