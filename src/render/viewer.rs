//! 一屏**外来**的编辑器画面（`[ui] file_viewer = "nvim"`）。
//!
//! 文件页点开一个文件时，浮层里跑一个真的 nvim：在 pty 里 spawn 它、把它的输出解析成网格、
//! 把键盘与鼠标的字节喂回去。**heng 在这里自己就是那个终端** —— pty 的另一端没有终端，
//! 所以还得替它回答那些查询（DSR、DA1、DECRQM、Kitty 键盘协议、DECRQSS、XTGETTCAP），
//! 不答它就不画：实测第一版没有应答层时，屏幕上只有
//! `E1568: Terminal did not respond to DSR request` 加一句 `Press ENTER or type command to
//! continue`。那一串询问、以及每一处为什么这么做，记在 `prototype/embed-nvim` 分支的
//! README 里 —— 这个模块是那份原型的收编。
//!
//! 它是**接缝**：[`Viewer`] 是 TUI 唯一需要知道的东西，测试里换成假的。测试不该 spawn 一个
//! 编辑器，更不该假设这台机器上装了它。
//!
//! 这一层**不进事件流、不进模型上下文**：与内置的内容弹窗同一条口径（渲染器读盘给读的人看，
//! 权限门约束的是模型的读写）。它因此不过 heng 的沙箱 —— 与那个弹窗一样，做的是
//! 「人在自己的终端里点开自己的文件」。

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::render::Key;

/// 浮层里那块外来屏幕 —— TUI 只与它打交道。
pub trait Viewer: Send {
    /// 现在这一屏（调用方拿它画）。
    fn screen(&self) -> vt100::Screen;
    /// 喂一串字节进去：键、鼠标、粘贴在这一层都是字节。
    fn feed(&mut self, bytes: &[u8]);
    /// 跟着浮层变尺寸（pty 那一侧的 `TIOCSWINSZ`）。
    fn resize(&mut self, cols: u16, rows: u16);
    /// 它还活着吗。它自己退出（`:q`）时前端把浮层收掉。
    fn alive(&mut self) -> bool;
    /// 收掉它：杀掉子进程，等着读线程退。
    fn kill(&mut self);
}

// ------------------------------------------------------------------ 键与鼠标 → 字节

/// 一个 [`Key`] → 写进 nvim 的字节。
///
/// **`Ctrl-C` 与 `Ctrl-Z` 不在这里**：前者是前端的「关掉这个浮层」，后者是终端层的挂起手势
/// （`.scratch/suspend-gesture/spec.md` §1「任何视图都拦不住它」），两者都由
/// `TuiState::key` 在更前面接走，根本到不了这里。
///
/// 一个已知的缺口：`Key` 不带修饰信息（`map_key` 那一层就把 `Alt+x` 当裸字符收下了），
/// 所以带 `Alt` 的组合到不了 nvim。换来的是这一层不必再懂一遍 crossterm 的键模型。
pub fn key_bytes(key: &Key) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = match key {
        Key::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        // nvim 把 `\r` 与 `\n` 都当 Enter；发 `\r` 稳妥。
        Key::Enter | Key::Newline => vec![b'\r'],
        Key::Tab => vec![b'\t'],
        Key::BackTab => b"\x1b[Z".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Esc => vec![0x1b],
        // 方向键用传统 CSI 编码：nvim 进了应用光标键模式（`DECCKM`）也照收这一套，
        // 而外层终端交给我们的本来就是这几个键（`prototype/embed-nvim` 里实测过）。
        Key::Up => b"\x1b[A".to_vec(),
        Key::Down => b"\x1b[B".to_vec(),
        Key::Right => b"\x1b[C".to_vec(),
        Key::Left => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        // `Key` 里那批 Emacs 风格的行编辑键，在 nvim 里全都是正经的控制键 —— 一个一个
        // 对回去，别让 `Ctrl-U` 之类在浮层里失灵。
        Key::CtrlA => vec![0x01],
        Key::CtrlE => vec![0x05],
        Key::CtrlG => vec![0x07],
        Key::CtrlK => vec![0x0b],
        Key::CtrlN => vec![0x0e],
        Key::CtrlO => vec![0x0f],
        Key::CtrlP => vec![0x10],
        Key::CtrlU => vec![0x15],
        Key::CtrlW => vec![0x17],
        // `Ctrl-D` 在 nvim 里是「向下翻半屏」，比它在前端当「退出」更值钱。
        Key::CtrlD => vec![0x04],
        // nvim 的 transpose：它在浮层里独占键盘，所以选择器那个手势到不了这里，得照原样
        // 进去（`.scratch/nvim-file-viewer/spec.md` §4）。
        Key::CtrlT => vec![0x14],
        Key::CtrlC | Key::CtrlZ => return None,
    };
    Some(bytes)
}

/// 一次鼠标事件 → SGR 编码（`CSI < b ; x ; y M`），坐标相对 nvim 那块网格（1 起算）。
///
/// 落在网格外的返回 `None`：框外那一下归前端（关掉浮层），而框内落在留白上的那几格什么都
/// 不做 —— 两条都照详情覆盖层的老规矩。
pub fn mouse_bytes(event: MouseEvent, grid: Rect) -> Option<Vec<u8>> {
    if event.column < grid.x
        || event.row < grid.y
        || event.column >= grid.x + grid.width
        || event.row >= grid.y + grid.height
    {
        return None;
    }
    let x = event.column - grid.x + 1;
    let y = event.row - grid.y + 1;
    let (mut code, released) = match event.kind {
        MouseEventKind::Down(MouseButton::Left) => (0, false),
        MouseEventKind::Down(MouseButton::Middle) => (1, false),
        MouseEventKind::Down(MouseButton::Right) => (2, false),
        MouseEventKind::Up(_) => (0, true),
        MouseEventKind::Drag(MouseButton::Left) => (32, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return None,
    };
    // 修饰位：SGR 的按钮码里 shift 是 4、alt 是 8、ctrl 是 16。`nowrap` 之后长行要靠
    // 「shift + 滚轮」横向滚，而这几位一丢就到不了 nvim。
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }
    let tail = if released { 'm' } else { 'M' };
    Some(format!("\x1b[<{code};{x};{y}{tail}").into_bytes())
}

// ------------------------------------------------------------------ nvim 那一份实现

/// 跑在 pty 里的一个 nvim，加上我们对它屏幕的全部理解（一份 [`vt100`] 的网格）。
pub struct NvimViewer {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser>>,
    exited: bool,
}

impl NvimViewer {
    /// 起一个**只读**的 nvim 看 `path`（相对 `cwd`）。
    ///
    /// 三件事写死在这里，是配置层定了的：
    ///
    /// - **只读**：`-M` 是真锁（`nomodifiable`，`:wq!` 也写不出去），`-R` 让状态行显示
    ///   `[RO]` —— 「这是只读的」在屏幕上有话说。`-R` 单用是**软**只读（`W!` 写得穿），
    ///   两个一起才够（两档各自的脾气在 `prototype/embed-nvim` 的 README 里实测过）。
    /// - **不折行**：`nowrap` 走 `-c` 而**不是** `--cmd` —— `--cmd` 在 vimrc **之前**执行，
    ///   用户配置里的 `wrap` 会把它顶掉；`-c` 在 vimrc 与文件之后执行，才压得住。
    /// - **鼠标给 nvim**：`set mouse=a`。
    ///
    /// `wake` 是「有新画面了」的通知口：读线程每收到字节就叫一声，前端据此重绘（60 ms 的
    /// 脉冲太慢，打字会钝）。
    pub fn spawn(
        cwd: &Path,
        path: &str,
        cols: u16,
        rows: u16,
        wake: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    ) -> std::io::Result<NvimViewer> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(other)?;

        let mut cmd = CommandBuilder::new("nvim");
        cmd.arg("-M");
        cmd.arg("-R");
        cmd.arg("-c");
        cmd.arg("set nowrap");
        cmd.arg("--cmd");
        cmd.arg("set mouse=a");
        cmd.arg(path);
        cmd.cwd(cwd);
        // 终端能力写死成最常见的一档：应答层与键编码都是照着它写的，而外层终端的差异
        // （真彩、键盘协议）由 heng 自己在这一层吸收掉 —— `CSI ? u` 我们就答「没有」。
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // 环境**不清**：读的人的 `~/.config/nvim`（`XDG_CONFIG_HOME` / `NVIM_APPNAME`）与
        // 他的 LazyVim 那一套都该照常生效（`prototype/embed-nvim` 里用探针实测过）。

        let child = pair.slave.spawn_command(cmd).map_err(other)?;
        // 从端必须在父进程里关掉，否则 pty 永远不会 EOF（`:q` 之后读线程一直挂着）。
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(other)?;
        let writer = pair.master.take_writer().map_err(other)?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let writer = Arc::new(Mutex::new(writer));

        let sink = parser.clone();
        let responder = writer.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let cursor = {
                            let mut parser = sink.lock().unwrap();
                            parser.process(&buf[..n]);
                            parser.screen().cursor_position()
                        };
                        // 一边解析一边当终端回答它 —— 这一层不做，它就停在
                        // `E1568` 上不画东西。
                        let mut answers = Vec::new();
                        respond_to(&buf[..n], cursor, &mut answers);
                        if !answers.is_empty() {
                            let mut writer = responder.lock().unwrap();
                            let _ = writer.write_all(&answers);
                            let _ = writer.flush();
                        }
                        if let Some(wake) = &wake {
                            // 叫一声就够了：前端只是要重绘，不关心叫了几次。
                            let _ = wake.send(());
                        }
                    }
                }
            }
        });

        Ok(NvimViewer {
            master: pair.master,
            writer,
            child,
            parser,
            exited: false,
        })
    }
}

impl Viewer for NvimViewer {
    fn screen(&self) -> vt100::Screen {
        self.parser.lock().unwrap().screen().clone()
    }

    fn feed(&mut self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
    }

    fn alive(&mut self) -> bool {
        if !self.exited {
            if let Ok(Some(_)) = self.child.try_wait() {
                self.exited = true;
            }
        }
        !self.exited
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.exited = true;
    }
}

// ------------------------------------------------------------------ 当终端

/// 回答 nvim 在**输出流里**向终端提的那些问题。
///
/// 嵌入方案的隐藏工作量就在这个函数里：pty 的另一端没有终端，heng 自己就是那个终端。
/// 不回，nvim 会等到超时（`E1568`），并且按一份错的终端画像去选渲染与按键路径 —— 比如它
/// 以为我们支持 Kitty 键盘协议，于是按那个协议发键，而我们发进去的键是传统编码。
///
/// 这一版只管 `--clean` 启动时实际会问的那几条（`prototype/embed-nvim` 的 `--trace`
/// 看得到全集）：
///
/// - `CSI 5 n`（设备状态）→ `CSI 0 n`
/// - `CSI 6 n`（光标位置）→ `CSI row;col R`
/// - `CSI c`（DA1）→ `CSI ? 62;22 c`：VT220 + ANSI 色，一个保守的画像
/// - `CSI ? Ps $ p`（DECRQM）→ `CSI ? Ps;Pv $ y`：鼠标与光标那几条报「已设」，其余报「不识别」
/// - `CSI ? u`（Kitty 键盘协议）→ `CSI ? 0 u`：明确说没有，于是它走传统键编码
/// - `DCS $ q m`（DECRQSS）与 `DCS + q <能力>`（XTGETTCAP）→ 一律礼貌地回「不支持」
///
/// 光标位置从**解析后**的屏幕取：`CSI 6 n` 问的是「你现在在哪」，答案得是它刚画完的那一帧。
fn respond_to(bytes: &[u8], cursor: (u16, u16), out: &mut Vec<u8>) {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'P') {
            // DCS（`ESC P` … `ST`）：XTGETTCAP（终端能力表）与 DECRQSS（当前设置）。
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == 0x1b && bytes[j + 1] == b'\\') {
                j += 1;
            }
            let body = &bytes[start..j.min(bytes.len())];
            if body.ends_with(b"$qm") {
                out.extend_from_slice(b"\x1bP0$r\x1b\\");
            } else if let Some(name) = body.strip_prefix(b"+q") {
                out.extend_from_slice(b"\x1bP0+r");
                out.extend_from_slice(name);
                out.extend_from_slice(b"\x1b\\");
            }
            i = (j + 2).min(bytes.len());
            continue;
        }
        if bytes[i] != 0x1b || bytes.get(i + 1) != Some(&b'[') {
            i += 1;
            continue;
        }
        let start = i + 2;
        let mut j = start;
        while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
            j += 1;
        }
        let Some(&final_byte) = bytes.get(j) else {
            return;
        };
        let params = &bytes[start..j];
        match final_byte {
            b'n' if params == b"5" => out.extend_from_slice(b"\x1b[0n"),
            b'n' if params == b"6" => {
                out.extend_from_slice(
                    format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1).as_bytes(),
                );
            }
            b'c' => out.extend_from_slice(b"\x1b[?62;22c"),
            b'p' if params.first() == Some(&b'?') && params.last() == Some(&b'$') => {
                let mode = &params[1..params.len() - 1];
                // 鼠标上报（1002 / 1006）、光标可见（25）、备用屏幕（1049）我们确实是那样；
                // 剩下的（2026 同步输出、2027、2031、2048、69…）老实报「不识别」。
                let set = [b"1002".as_slice(), b"1006", b"25", b"1049"].contains(&mode);
                let value = if set { '1' } else { '2' };
                out.extend_from_slice(
                    format!("\x1b[?{};{}$y", String::from_utf8_lossy(mode), value).as_bytes(),
                );
            }
            b'u' if params == b"?" => out.extend_from_slice(b"\x1b[?0u"),
            _ => {}
        }
        i = j + 1;
    }
}

// ------------------------------------------------------------------ 画

/// 一屏外来画面画进 ratatui 的 Buffer。
///
/// 这一层就是 `tui-term` 替人做的事，几十行：每个格子连颜色与属性一起搬过去 ——
/// 宽字符的第二格在 `vt100` 里是空串，**跳过**它，覆盖会把那个字擦掉一半。
///
/// 而**没有文字**的格子照样要搬：nvim 铺底色靠的正是「设好背景、再擦掉」，擦出来的格子
/// `contents()` 是空的、`bgcolor()` 却是它的底色。只画有字的那几格，屏幕上就只剩文字处
/// 有底色、空白处透出底下的转录 —— 而「没有框线，nvim 自己那块底色就是边界」
/// （`.scratch/nvim-file-viewer/spec.md` §3）正是靠铺满成立。
pub struct ScreenWidget<'a> {
    pub screen: &'a vt100::Screen,
}

impl Widget for ScreenWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            for col in 0..area.width {
                let Some(cell) = self.screen.cell(row, col) else {
                    continue;
                };
                // 宽字的第二格：它没有内容，覆盖会把那个字擦掉一半。
                if cell.is_wide_continuation() {
                    continue;
                }
                let Some(target) = buf.cell_mut(Position::new(area.x + col, area.y + row)) else {
                    continue;
                };
                // **没有文字也要写一个空格**：这块外来屏幕盖住的每一格都由它自己交代成空。
                // 只设样式不写符号的话，同一帧里先画的左栏（标记、页签条、读数）会原样留在
                // 缓冲里 —— 屏幕上就是它们从浮层的空白处透出来（2026-10-07 维护者报的）。
                // 空内容的格子与「本来就该是空格」的格子在 `vt100` 里是同一件事。
                let contents = cell.contents();
                target.set_symbol(if contents.is_empty() { " " } else { contents });
                target.set_style(style_of(cell));
            }
        }
    }
}

fn style_of(cell: &vt100::Cell) -> Style {
    let style = Style::default()
        .fg(color_of(cell.fgcolor()))
        .bg(color_of(cell.bgcolor()));
    let mut modifier = Modifier::empty();
    if cell.bold() {
        modifier |= Modifier::BOLD;
    }
    if cell.dim() {
        modifier |= Modifier::DIM;
    }
    if cell.italic() {
        modifier |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifier |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        modifier |= Modifier::REVERSED;
    }
    style.add_modifier(modifier)
}

fn color_of(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn other<E: std::fmt::Display>(error: E) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

// ------------------------------------------------------------------ 测试用的假屏幕

/// 测试用的假屏幕：**不起任何进程**。
///
/// 测试不该 spawn 一个编辑器，也不该假设这台机器上装了它 —— 所以 [`Viewer`] 这个接缝在
/// 测试里换成它。观测数据放在一个共享句柄里（[`FakeViewer::watch`]），因为塞进状态机的那个
/// 是 `Box<dyn Viewer>`，拿不回来。
#[cfg(test)]
#[derive(Clone)]
pub struct FakeViewer {
    log: Arc<Mutex<FakeLog>>,
}

/// 假屏幕记下来的东西。
#[cfg(test)]
pub struct FakeLog {
    /// 喂进来的字节，按顺序。
    pub fed: Vec<u8>,
    pub kills: usize,
    pub resizes: Vec<(u16, u16)>,
    pub alive: bool,
    parser: vt100::Parser,
}

#[cfg(test)]
impl Default for FakeViewer {
    fn default() -> Self {
        Self {
            log: Arc::new(Mutex::new(FakeLog {
                fed: Vec::new(),
                kills: 0,
                resizes: Vec::new(),
                alive: true,
                parser: vt100::Parser::new(4, 4, 0),
            })),
        }
    }
}

#[cfg(test)]
impl FakeViewer {
    /// 一份看得见它的观测口。
    pub fn watch(&self) -> FakeWatcher {
        FakeWatcher(self.log.clone())
    }
}

/// 从外面看那个假屏幕。
#[cfg(test)]
#[derive(Clone)]
pub struct FakeWatcher(Arc<Mutex<FakeLog>>);

#[cfg(test)]
impl FakeWatcher {
    pub fn fed(&self) -> Vec<u8> {
        self.0.lock().unwrap().fed.clone()
    }

    pub fn kills(&self) -> usize {
        self.0.lock().unwrap().kills
    }

    pub fn resizes(&self) -> Vec<(u16, u16)> {
        self.0.lock().unwrap().resizes.clone()
    }
}

#[cfg(test)]
impl Viewer for FakeViewer {
    fn screen(&self) -> vt100::Screen {
        self.log.lock().unwrap().parser.screen().clone()
    }

    fn feed(&mut self, bytes: &[u8]) {
        let mut log = self.log.lock().unwrap();
        log.fed.extend_from_slice(bytes);
        log.parser.process(bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let mut log = self.log.lock().unwrap();
        log.resizes.push((cols, rows));
        log.parser.screen_mut().set_size(rows, cols);
    }

    fn alive(&mut self) -> bool {
        self.log.lock().unwrap().alive
    }

    fn kill(&mut self) {
        let mut log = self.log.lock().unwrap();
        log.kills += 1;
        log.alive = false;
    }
}
