//! 全屏外壳的几何（`.scratch/tui-sidebar/spec.md` §1–§2）。
//!
//! 纯算术：进去的是终端尺寸与输入区的行数，出来的是那个尺寸下存在的那些矩形。把它与绘制
//! 拆开，正是让降级阶梯恰好只有一个家的原因 —— 渲染器里没有 —— 也让人能一口气把那些
//! 阈值读完，而不是从四个调用点往回重建。
//!
//! 外壳是**一圈外框、一条全高左栏、一条主列**：外框是终端的边框，左栏放标记、页签条与
//! 会话读数，主列自上而下堆着转录、状态行、输入区与提示行。左栏的去留由**宽度与用户意愿
//! 相乘**决定：宽度档在这里算，意愿由调用方作为 `wanted` 传进来 —— 两者不同层，谁也不能替
//! 谁说话（`.scratch/sidebar-toggle/spec.md` §2）。

use ratatui::layout::Rect;

use crate::render::editor::Placed;

/// 画外壳的最小终端。再小一列或一行，屏幕上就只剩 [`crate::render::wording::too_small`]。
pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 10;

/// 外壳花在既不是转录也不是输入区的那些构件上的行：主列的两条分隔线、状态行与提示行。
/// `转录行 = h − CHROME − 输入行数`，这就是纵向算术的全部（spec §1、§2）。
///
/// 它从 7 一路减到这里，两笔账都是 `tui-chrome` 的：外框的上下两行随外框一起离开（§1），
/// 状态行上方那条线也离开、它占的那一行还给了转录（§2）。
const CHROME: u16 = 4;

/// **一个带框浮层**花掉的行列：它的内容矩形 = 四边各内缩一格。
///
/// 外壳不再用它 —— 外框已经离开（spec §1），内容区就是终端本身。它现在只服务那三块自己
/// 带框的东西：问题覆盖层、`/` 菜单、详情覆盖层。
const BORDER_COLUMNS: u16 = 2;

/// 页签条花掉的行：一条分隔线、标签、一条分隔线（spec §3）。
const TAB_ROWS: u16 = 3;

/// 页签条顶端那条分隔线与它的标签之间的行：就是那条分隔线本身。写在 [`TAB_ROWS`] 旁边，
/// 好让同一条页签条的两笔账不会漂移。
const TAB_RULE_ROWS: u16 = 1;

/// 转录在右缘永远留着的那几列：滚动条的列与回合条的列。画不画都留着，这样文字不会因为
/// 转录长高了而重新折行（spec §1）。
const TRAILING_COLUMNS: u16 = 2;

/// 左栏的两个内容宽度（spec §2）。两者之间刻意没有档：prototype 量过中间那一档，它换回来
/// 的 `（6%）` 已经在状态行里了。
const SIDEBAR_WIDE: u16 = 40;
const SIDEBAR_NARROW: u16 = 28;

/// 左栏每一档开始的宽度。
const SIDEBAR_WIDE_FROM: u16 = 120;

/// 低于这个宽度，左栏整栏隐藏，主列拿走一切。
const SIDEBAR_NARROW_FROM: u16 = 80;

/// 左栏顶上留的那一行空行：身份（标记或文字身份）从它下面一行才开始
/// （2026-10-01 真机反馈 —— 紧贴屏幕顶上太挤）。
///
/// 它只花左栏自己的行（主列与转录不受影响），并且计入左栏的内容高度：留白是**花掉的**，
/// 不是白得的，所以高度阶梯照旧由「内容行够不够」决定。
const SIDEBAR_TOP_GAP: u16 = 1;

/// 标记自己的宽度，与画家共用，好让两者不会脱节。
pub const LOGO_WIDTH: u16 = 38;

/// 标记画的行数，以及文字身份占的那一行。
const LOGO_ROWS: u16 = 5;
const IDENTITY_ROWS: u16 = 1;

/// 什么都不挤时左栏调用量页持有的字段，以及它最少保留的几个：上下文 / token / 回合
/// （spec §2）。
const SIDEBAR_FIELDS: u16 = 6;
const SIDEBAR_MIN_FIELDS: u16 = 3;

/// 输入区最多持有的输入行数（spec §2）。
const MAX_INPUT_ROWS: u16 = 10;

/// 无论草稿折成几行，输入区最少持有的行数（`.scratch/tui-input-pulse/spec.md` §1）。
///
/// 它是**输入区**的地板，不是草稿的：空草稿照样让编辑器答一行，它下面的行就是空白。有一个
/// 写三行的地方、而它不会在第三行到来的那一刻长高，这就是全部意义 —— 以前这个框会跟着草稿
/// 在光标下面跳。
const MIN_INPUT_ROWS: u16 = 3;

/// 一帧画左栏三种身份里的哪一种。
///
/// 阶梯在这里、在 [`sidebar_content`] 里定，所以画家问这里，而不是从左栏的高度自己重推
/// —— 一个自己比高度的画家就握着半条阶梯，而两半会漂移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarKind {
    /// 宽档上的标记（spec §2）。
    Mark,
    /// 一行文字身份，窄档放得下的就是这个。
    Text,
    /// 两者都不是：高度阶梯让出了身份，好让读数留下来。
    Hidden,
}

impl SidebarKind {
    /// 这个身份在左栏顶上花掉的行。
    pub fn rows(self) -> u16 {
        match self {
            SidebarKind::Mark => LOGO_ROWS,
            SidebarKind::Text => IDENTITY_ROWS,
            SidebarKind::Hidden => 0,
        }
    }
}

/// 一帧的那些区域，用终端坐标表示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    /// 整屏。外壳如今不再内缩，所以它就是内容区（spec §1）。详情覆盖层在**它**里面居中
    /// （§4）——而问题覆盖层仍然在主列里，两者故意不同基准。
    pub screen: Rect,
    /// 主列：分隔线右侧的一切。问题覆盖层与 `/` 菜单在它里面定位。
    pub main: Rect,
    /// 转录的那些行：文字、滚动条与回合条合在一起。
    pub transcript: Rect,
    /// 回合条的那一列，在转录的右缘。会话有没有单位放进去都画。
    pub rail: Rect,
    /// 状态行的内容行：`模型 … │ 模式 … │ 上下文 …%`。永远都画（spec §2、§5）。
    pub status: Rect,
    /// 主列里的那些输入行。
    pub input: Rect,
    /// 提示行，在输入区下面。
    pub hints: Rect,
    /// 左栏的内容，在它到底画了的时候。分隔线那一列**不算**它的一部分。
    pub sidebar: Option<Rect>,
    /// 页签条的标签行，在左栏画了的时候。
    pub tabs: Option<Rect>,
    /// 左栏的页行 —— 高度阶梯每留下一个字段就一行，所以这个矩形自己的高度就是那个数
    /// （spec §2、§3）。
    pub sidebar_page: Option<Rect>,
    /// 左栏与主列共有的那一列。
    pub divide: Option<u16>,
    /// 这一帧最后出来的是哪种左栏身份。
    pub sidebar_kind: SidebarKind,
}

/// 终端是不是小到只放得下那句告知（[`crate::render::wording::too_small`]）。
pub fn below_minimum(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

impl Regions {
    /// 转录的文字区：它的内容减去右缘永远留着的那两列 —— 滚动条的与回合条的。
    pub fn transcript_text(&self) -> Rect {
        Rect::new(
            self.transcript.x,
            self.transcript.y,
            self.transcript.width.saturating_sub(TRAILING_COLUMNS),
            self.transcript.height,
        )
    }

    /// 滚动条那一列，在回合条那一列的左边一格。
    pub fn scrollbar(&self) -> Rect {
        Rect::new(
            self.transcript.right().saturating_sub(TRAILING_COLUMNS),
            self.transcript.y,
            TRAILING_COLUMNS - 1,
            self.transcript.height,
        )
    }

    /// 主列里问题覆盖层的宽度。
    pub fn modal_width(&self) -> u16 {
        self.main
            .width
            .saturating_sub(MODAL_MARGIN)
            .min(MODAL_MAX_WIDTH)
    }

    /// 详情覆盖层的宽度：屏幕减去两侧边距，封顶是它自己的上限。一段工具输出是正文，不是
    /// 一句话，所以允许它有更多余地，免得眼睛来回跑（票 03 §Answer）。
    ///
    /// 基准是**整屏**而不是主列（spec §4）：覆盖层居中在屏幕上，宽度也该对着屏幕量。120 列
    /// 下封住它的仍然是边距（116 < 135），与改动前同宽；80 列下它明显变宽（76 而不是
    /// `主列宽 − 4`）—— 那是「屏幕居中」的应有之义。
    pub fn detail_width(&self) -> u16 {
        self.screen
            .width
            .saturating_sub(MODAL_MARGIN)
            .min(DETAIL_MAX_WIDTH)
    }

    /// 详情覆盖层去哪儿：**在屏幕上居中**（spec §4），比上下各短一行，好让转录在上下各留
    /// 一条边。
    ///
    /// 它刻意与 [`Regions::modal`] 不同基准：问题覆盖层留在主列里（问答期间压掉左栏会盖住
    /// 会话读数），而详情是「读一页正文」，居中在屏幕上看起来才是正的。两者不同不是笔误。
    ///
    /// 终端小到显示不出有用的正文时是 `None` —— 与 [`Regions::modal`] 给的是同一个诚实
    /// 答案，而那时详情视图干脆不打开，而不是打开成两行边框。
    pub fn detail(&self) -> Option<Rect> {
        let width = self.detail_width();
        if width <= BORDER_COLUMNS || self.screen.height <= BORDER_COLUMNS + DETAIL_MIN_ROWS {
            return None;
        }
        let height = self
            .screen
            .height
            .saturating_sub(BORDER_COLUMNS + DETAIL_MARGIN_ROWS);
        Some(Rect::new(
            self.screen.x + (self.screen.width.saturating_sub(width)) / 2,
            self.screen.y + (self.screen.height.saturating_sub(height)) / 2,
            width,
            height,
        ))
    }

    /// 一个 `rows` 个显示行高的问题去哪儿：在主列里居中，在那里画不清楚时哪儿都不去。
    pub fn modal(&self, rows: u16) -> Option<Rect> {
        let width = self.modal_width();
        let height = rows.saturating_add(BORDER_COLUMNS);
        if width <= BORDER_COLUMNS || height > self.main.height {
            return None;
        }
        Some(Rect::new(
            self.main.x + (self.main.width - width) / 2,
            self.main.y + (self.main.height - height) / 2,
            width,
            height,
        ))
    }

    /// 一个锚在 `anchor` 的浮动菜单在它要开的地方能拿多少内容行：光标那一行之上的全部，
    /// 上面没地方时则是它下面的余地。
    ///
    /// 调用方在要矩形之前先把匹配结果裁到这个数，于是菜单是滚动而不是被拒。
    pub fn menu_room(&self, anchor: Placed) -> u16 {
        let cursor_y = self.input.y.saturating_add(anchor.row);
        let above = cursor_y.saturating_sub(self.main.y);
        if above > 1 {
            return above - 1;
        }
        self.main.bottom().saturating_sub(cursor_y + 1)
    }

    /// 浮动的 `/` 菜单去哪儿：一个 `width` 列宽、`rows` 个内容行高的带框盒子，锚在光标上，
    /// 好跟着正在打的东西走（spec §6）。
    ///
    /// 它**向上**开 —— 输入区在屏幕脚下，所以有地方的是那一侧 —— 只有在上面什么都放不下时
    /// 才落到光标下面。两边都放不下时是 `None`，那是在那么小的终端上诚实的答案。
    pub fn menu(&self, anchor: Placed, width: u16, rows: u16) -> Option<Rect> {
        if rows == 0 || width < MENU_MIN_WIDTH {
            return None;
        }
        let height = rows.saturating_add(BORDER_COLUMNS);
        let cursor_y = self.input.y.saturating_add(anchor.row);
        let top = self.main.y;
        let y = if cursor_y.saturating_sub(top) >= height {
            cursor_y - height
        } else if self.main.bottom().saturating_sub(cursor_y + 1) >= height {
            cursor_y + 1
        } else {
            return None;
        };
        let left = self.main.x;
        let right = self.main.right();
        let width = width.min(right.saturating_sub(left));
        let x = self
            .input
            .x
            .saturating_add(anchor.column)
            .min(right.saturating_sub(width))
            .max(left);
        Some(Rect::new(x, y, width, height))
    }
}

/// 一个输入行有多少宽度留给文字：主列的内容减去提示符。在 [`plan`] 跑之前就知道，因为
/// plan 需要的正是草稿自己的高度。
pub fn input_text_width(area: Rect, sidebar_wanted: bool) -> u16 {
    main_width(area.width, sidebar_wanted).saturating_sub(crate::render::editor::prompt_columns())
}

/// 主列的内容宽度。
///
/// 问卷在 [`plan`] 跑之前就需要这个 —— 它想要几行决定输入区多高 —— 而它必须与 `plan`
/// 交回来的 `input` 矩形一致，否则画出来的行与请求的高度就对不上。
pub fn content_width(area: Rect, sidebar_wanted: bool) -> u16 {
    main_width(area.width, sidebar_wanted)
}

/// 排出一帧的版面。`draft_rows` 是输入的草稿折成的行数，`sidebar_wanted` 是用户想不想看见
/// 左栏（`Ctrl-O` 的那一位，`.scratch/sidebar-toggle/spec.md` §2）。
///
/// 这里的顺序**就是**降级阶梯：终端变窄时左栏先变窄、再隐藏；草稿长高时输入区往转录的行里
/// 长；左栏自己的高度决定它的哪些部分活下来（spec §2）。**意愿与宽度相乘**：`wanted` 为假
/// 时走的是与「宽度不够」完全相同的那一支 —— 左栏整栏让位，主列拿走一切。
///
/// 输入区自己的阶梯是 [`MIN_INPUT_ROWS`] … [`MAX_INPUT_ROWS`]，而地板被余地夹住：两者相
/// 撞处**转录的最后一行优先**。外框与状态行上方那条线相继离开之后（spec §1–§2），40×10
/// 下那份余地足够让输入区拿满三行，同时转录还留三行 —— 不再是「两行输入区、一行转录」。
pub fn plan(area: Rect, draft_rows: u16, sidebar_wanted: bool) -> Regions {
    // 内容区就是终端：外框已经离开（spec §1），没有哪一圈要内缩。
    let tier = sidebar_tier(area.width, sidebar_wanted);
    // 左栏顶上先让出一行空行，再算它的身份与字段 —— 留白是花掉的行，所以阶梯看到的是
    // 减掉它之后的高度。
    let sidebar_rows = area.height.saturating_sub(SIDEBAR_TOP_GAP);
    let (sidebar_kind, fields) = sidebar_content(area.width, sidebar_rows, sidebar_wanted);
    let input_rows = draft_rows
        .max(MIN_INPUT_ROWS)
        .min(max_input_rows(area.height));
    let transcript_rows = area.height.saturating_sub(CHROME + input_rows);

    let sidebar = tier.map(|tier| Rect::new(area.x, area.y + SIDEBAR_TOP_GAP, tier, sidebar_rows));
    let divide = tier.map(|tier| area.x + tier);
    let main_x = divide.map_or(area.x, |divide| divide + 1);
    let main = Rect::new(
        main_x,
        area.y,
        main_width(area.width, sidebar_wanted),
        area.height,
    );

    // 状态行**紧贴**转录的最后一行：它上方那条线已经不画了，省下的一行整行还给了转录
    // （spec §2）。它下面两条线各占一行 —— 而 `bottom()` 是排他的，所以那条线正好落在
    // `status.bottom()` / `input.bottom()` 自己那一行上，内容从下一行起。
    let transcript = Rect::new(main.x, main.y, main.width, transcript_rows);
    let status = Rect::new(main.x, transcript.bottom(), main.width, 1);
    let input = Rect::new(main.x, status.bottom() + 1, main.width, input_rows);
    let hints = Rect::new(main.x, input.bottom() + 1, main.width, 1);
    let rail = Rect::new(
        transcript.right().saturating_sub(1),
        transcript.y,
        1,
        transcript_rows,
    );

    Regions {
        screen: area,
        main,
        transcript,
        rail,
        status,
        input,
        hints,
        sidebar,
        tabs: sidebar.map(|sidebar| {
            Rect::new(
                sidebar.x,
                sidebar.y + sidebar_kind.rows() + TAB_RULE_ROWS,
                sidebar.width,
                1,
            )
        }),
        sidebar_page: sidebar.map(|sidebar| {
            Rect::new(
                sidebar.x,
                sidebar.y + sidebar_kind.rows() + TAB_ROWS,
                sidebar.width,
                fields,
            )
        }),
        divide,
        sidebar_kind,
    }
}

/// 终端宽 `width`、用户意愿 `sidebar_wanted` 时左栏的内容宽度，没得画时是 `None`。
///
/// 两个判据**相乘**：意愿说偏好，宽度说可行性。`sidebar_wanted` 为假与宽度不够走的是同一支
/// —— 左栏是它自己的一列，所以它的高度不是转录可以花掉的（spec §2）。宽度那三档一个字不改
/// （`.scratch/sidebar-toggle/spec.md` §2）。
fn sidebar_tier(width: u16, sidebar_wanted: bool) -> Option<u16> {
    if !sidebar_wanted {
        return None;
    }
    if width >= SIDEBAR_WIDE_FROM {
        Some(SIDEBAR_WIDE)
    } else if width >= SIDEBAR_NARROW_FROM {
        Some(SIDEBAR_NARROW)
    } else {
        None
    }
}

/// 主列拿到的那些列：整屏减去左栏（它画出来时连同分隔线那一列）。
///
/// 外框已经不占列了（spec §1），所以这里只剩左栏这一笔账。意愿为假时左栏那两笔一起让回来，
/// 主列因此拿到整屏宽（`.scratch/sidebar-toggle/spec.md` §2）。
fn main_width(width: u16, sidebar_wanted: bool) -> u16 {
    let sidebar = sidebar_tier(width, sidebar_wanted).map_or(0, |tier| tier + 1);
    width.saturating_sub(sidebar)
}

/// 给定左栏自己的内容高度，它的身份以及它能显示几个用量字段。
///
/// 阶梯是定死的：**标记**先走（先退成文字身份，再退成没有），然后从尾部丢字段 —— 先是
/// 缓存，再是输出，再是输入。地板是页签条加上上下文 / token / 回合，所以回答「还剩多少」的
/// 那三个读数最后走（spec §2）。意愿为假时连门都不进：整栏让位
/// （`.scratch/sidebar-toggle/spec.md` §2）。
fn sidebar_content(width: u16, content_rows: u16, sidebar_wanted: bool) -> (SidebarKind, u16) {
    let Some(tier) = sidebar_tier(width, sidebar_wanted) else {
        return (SidebarKind::Hidden, 0);
    };
    let mut kind = if tier >= LOGO_WIDTH {
        SidebarKind::Mark
    } else {
        SidebarKind::Text
    };
    let mut fields = SIDEBAR_FIELDS;
    loop {
        if kind.rows() + TAB_ROWS + fields <= content_rows {
            break;
        }
        match kind {
            SidebarKind::Mark => kind = SidebarKind::Text,
            SidebarKind::Text => kind = SidebarKind::Hidden,
            SidebarKind::Hidden if fields > SIDEBAR_MIN_FIELDS => fields -= 1,
            // 地板：页签条与那三个读数。连这些也放不下的终端在 [`MIN_HEIGHT`] 以下，永远
            // 到不了这里。
            SidebarKind::Hidden => break,
        }
    }
    (kind, fields)
}

/// 一个带框区域的内容矩形：四边各内缩一格。
///
/// 它现在只服务**自己带框**的浮层 —— 问题覆盖层、`/` 菜单、详情覆盖层。外壳不再内缩
/// （spec §1），所以 [`plan`] 与 [`main_width`] 都不再用它。
pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(BORDER_COLUMNS),
        area.height.saturating_sub(BORDER_COLUMNS),
    )
}

/// 这个尺寸下输入区最多能拿几行：它的上限，或者转录留住自己那行地板之后剩下的，两者取小。
///
/// 40×10 下那份余地是 5 行，所以在那里封住输入区的是 [`MIN_INPUT_ROWS`]，不是它。
fn max_input_rows(height: u16) -> u16 {
    let room = height.saturating_sub(CHROME + 1);
    MAX_INPUT_ROWS.min(room).max(1)
}

/// 问题覆盖层最宽能到多少。比这更宽眼睛就得来回跑了：一个问题是一句话，不是一页
/// （spec §9）。
const MODAL_MAX_WIDTH: u16 = 72;

/// 覆盖层在主列两侧留下的空白列。
const MODAL_MARGIN: u16 = 4;

/// 详情覆盖层最宽能到多少（票 03 §Answer；2026-09-23 加宽 50%：90 → 135）。一段工具正文
/// 是界面里唯一像一页而不是一句话的东西，所以它拿到那份余地，直到终端自己用完：120 列时
/// 封住它的是边距，不是这条上限。
const DETAIL_MAX_WIDTH: u16 = 135;

/// 值得为详情覆盖层打开的最少正文行数。
const DETAIL_MIN_ROWS: u16 = 1;

/// 详情覆盖层两边各留一行转录显示，上下各一行，好让读者保住他点进来的那个位置。
const DETAIL_MARGIN_ROWS: u16 = 2;

/// `/` 菜单在匹配结果开始滚动之前最多显示几行。菜单是一条提示，不是一份目录：过了这个数，
/// 读者就是在滚一份列表去找一个他本可以打出来的名字（spec §6）。
pub const MENU_MAX_ROWS: u16 = 8;

/// `/` 菜单最宽能到多少：一个名字加它那一行描述，不用眼睛来回跑。
pub const MENU_MAX_WIDTH: u16 = 72;

/// 值得画一个菜单盒子的最窄宽度：低于这个，边框就是它的大半。
const MENU_MIN_WIDTH: u16 = 12;
