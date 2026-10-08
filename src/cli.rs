//! CLI 边界：唯一读环境的地方。
//!
//! 二进制只是这个模块外面一层薄壳。库需要的一切都在这里从 `config.toml`、导出的环境与内建默认值组
//! 装出来，然后注入 [`crate::assemble`]。
//!
//! `probe` 是票 02 的手工验收工具：它对每个已配置的模型驱动一个真实回合，然后在同一场会话里再驱动
//! 第二个回合，这样第二个请求的用量行就能看出前缀缓存有没有命中。它无头运行，用一次性的会话日志。
//!
//! `prune` 与 `sessions` 读的都是票 12 建出来的那个 store：`prune` 手动删掉某个工作区的会话目录，
//! 而 `sessions` 从一场**已结束**会话自己的流里回答关于它的问题 —— `ls`、按轮分组的 `show`
//! （`--files` 是按工作区对象看的那个视图）、`replay`（投影的重算，spec §18）与 `stats`。它们住在
//! 这里，因为 store 的 root 来自环境，而库从不读环境；查询本身在
//! [`crate::session::observe`] 与 [`crate::agent::replay`]。交互式渲染器仍是唯一没有接线的那一块
//! （票 18）。

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ratatui::buffer::CellWidth;

use crate::agent::{CancelSignal, UndoOutcome, replay};
use crate::config::{self, Config, Debater, DiscussionRoster, EnvMap, ReasoningEffort};
use crate::events::{
    ContextSource, Event, EventPayload, SessionId, SpeakerId, StopReason, Usage, read_events,
    total_usage,
};
use crate::mcp::{self, McpService};
use crate::permissions::{Mode, Policy};
use crate::provider::Message;
use crate::provider::capability::caps_for;
use crate::provider::openai::{BuildError, OpenAiProvider, stderr_warnings};
use crate::questions::{UserQuestion, UserQuestions};
use crate::render::token::{self, Token};
use crate::render::{
    self, ConsoleAsker, ConsoleEvents, ConsoleHandle, ConsoleQuestions, FrontEndEvent, PickerKind,
    PickerOption, PlainOptions, RenderSinks, Renderer, SessionFacts, TuiOptions,
};
use crate::session::observe::{self, CostModel, Entry, Filter, Listing, Timeline};
use crate::session::{SessionStore, StoredSession};
use crate::tools::{self, PathLocks, Sandbox};
use crate::web::WebService;
use crate::web::fetch_http::HttpFetch;
use crate::web::search_deepseek::{DEEPSEEK_SEARCH_PROVIDER, DeepSeekSearch};
use crate::{
    AssemblyParts, DebaterParts, DiscussionHarness, DiscussionParts, Harness, SessionScaffold,
    SynthesizerParts, assemble, assemble_discussion,
};

/// 探针第二个回合的提示词；它让转录继续变长，好让第一个回合的前缀成为缓存必须匹配的那些内容。
const PROBE_FOLLOW_UP: &str = "严格只回复：done";

/// 两次探针回合之间等多久。
///
/// DeepSeek 在磁盘上建它的前缀缓存要花「秒」级的时间；立刻再问一次量到的是冷缓存，会报出假阴性。
const CACHE_WARMUP: std::time::Duration = std::time::Duration::from_secs(10);

/// 探针的第一个回合。填充得远超两家厂商的缓存下限（Kimi 只缓存 256 token 以上的 prompt），这样
/// 第二个请求才有可能显出一次命中。
fn probe_prompt() -> String {
    const FILLER: &str = "敏捷的棕色狐狸跳过了那只懒狗，前缀缓存正在热起来。";
    let mut prompt = String::from("忽略下面的填充：它只是把提示词撑长，好让前缀缓存起作用。\n");
    while prompt.len() < 2_000 {
        prompt.push_str(FILLER);
    }
    prompt.push_str("\n严格只回复：ok");
    prompt
}

/// 如果这个进程是 root，它为什么拒绝启动（spec §20）。
///
/// 刻意做成只由有效 uid 决定的纯函数：这次拒绝除了 uid 没有任何可读的东西，所以没有任何参数、环境
/// 变量或模式能把它关掉。这个项目的护栏假定最糟的情况留在工作区之内；而作为 root，一次误判就是全系
/// 统的，那是与正在管理的风险不同的一类风险。
pub fn root_refusal(euid: u32) -> Option<String> {
    (euid == 0).then(|| render::wording::root_refusal().to_owned())
}

/// 从环境里解析 `argv` 并运行。这是二进制的入口。
pub fn main() -> ExitCode {
    // 先于一切检查 —— 先于参数、先于 runtime —— 所以进不了以 root 身份运行程序的那条路径，也没有
    // 任何旗标能跳过这个检查（spec §20）。
    // SAFETY: `geteuid` 只读调用进程的 uid，不会失败。
    let euid = unsafe { libc::geteuid() };
    if let Some(message) = root_refusal(euid) {
        eprintln!("heng: {message}");
        return ExitCode::FAILURE;
    }
    let env: EnvMap = std::env::vars().collect();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!(
                "heng: {}",
                render::wording::startup_runtime(&error.to_string())
            );
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(&args, &env))
}

async fn run(args: &[String], env: &EnvMap) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("heng {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") => {
            println!("{}", render::wording::help_main());
            ExitCode::SUCCESS
        }
        Some("probe") => probe(&args[1..], env).await,
        Some("discuss") => discuss(&args[1..], env).await,
        Some("prune") => prune(&args[1..], env),
        Some("sessions") => {
            let stdout = std::io::stdout();
            let stderr = std::io::stderr();
            let mut out = stdout.lock();
            let mut err = stderr.lock();
            run_sessions(&args[1..], env, &mut out, &mut err)
        }
        // 没有子命令（或者只有一个裸旗标）就是交互式会话：常见情形就是在某个工作区里直接跑
        // `heng`。`interactive` 自己解析自己那批参数，并拒掉任何它不认识的东西。
        _ => interactive(args, env).await,
    }
}

// ---------------------------------------------------------------------------
// 交互式前端（spec §19）
// ---------------------------------------------------------------------------

/// 接着跑哪一场会话（`-c` / `--continue` / `--session`）。
#[derive(Debug, PartialEq, Eq)]
enum Resume {
    /// 本工作区里最近写下的那场。
    Latest,
    /// 指名的那个 id，或者它会话目录的路径。
    Session(String),
}

/// 一次已解析的交互式调用。渲染器在这里选定并注入组装，所以只会有一个模式在跑。
#[derive(Debug, Default)]
struct InteractiveArgs {
    /// 强制用 plain 渲染器。
    plain: bool,
    /// 强制用 TUI 渲染器。
    tui: bool,
    /// 接着跑哪一场会话（spec §11）：不写就是新开一场。
    resume: Option<Resume>,
    config: Option<PathBuf>,
    model: Option<String>,
    /// `--mode readonly|ask|workspace|auto`：这一趟跑的权限模式，覆盖 `[permissions] mode`（spec §12）。不
    /// 写表示「文件怎么写就怎么来」。
    mode: Option<Mode>,
    cwd: Option<PathBuf>,
}

fn parse_interactive(args: &[String]) -> Result<InteractiveArgs, String> {
    let mut parsed = InteractiveArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plain" => parsed.plain = true,
            "--tui" => parsed.tui = true,
            // `-c` / `--continue` 后面跟一个不以 `-` 开头的词，就是**指名**续哪一场
            // （`heng -c 20261001T155845Z-7a69cbff`）；没有就是本工作区最新那场。
            // 判的是「下一个词是不是旗标」而不是「是不是 id」：这样 `heng -c --plain`
            // 仍然是「续最新 + plain」，不会去找一场叫 `--plain` 的会话。
            "--continue" | "-c" => {
                parsed.resume = Some(match args.get(index + 1) {
                    Some(id) if !id.starts_with('-') => {
                        index += 1;
                        Resume::Session(id.clone())
                    }
                    _ => Resume::Latest,
                });
            }
            // 同一个意思的显式拼写，只是它**必须**给 id（而且和 `-c` 一样，旗标不算 id）。
            "--session" => {
                index += 1;
                let id = args
                    .get(index)
                    .filter(|value| !value.starts_with('-'))
                    .ok_or_else(|| render::wording::needs_value("--session"))?;
                parsed.resume = Some(Resume::Session(id.clone()));
            }
            flag @ ("--config" | "--model" | "--mode" | "--cwd") => {
                let flag = flag.to_owned();
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value(&flag))?;
                match flag.as_str() {
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--model" => parsed.model = Some(value.clone()),
                    // 在这里解析而不是在组装里：权限门不认识的一个模式是打错字，而打错字值得在
                    // provider 被构建、会话被创建之前就拒掉。
                    "--mode" => {
                        parsed.mode = Some(
                            Mode::parse(value)
                                .ok_or_else(|| render::wording::unknown_mode(value))?,
                        )
                    }
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            other => return Err(render::wording::unknown_argument(other)),
        }
        index += 1;
    }
    if parsed.plain && parsed.tui {
        return Err(render::wording::renderers_mutually_exclusive().to_owned());
    }
    Ok(parsed)
}

/// 一趟运行从哪一档开始：`--mode` 旗标压过 `[permissions] mode`（spec §12；
/// `.scratch/todo-and-modes/spec.md` §1）。
///
/// 做成具名函数而不是内联表达式，因为「两者谁赢」**就是**入口点的决定，而这里是测试唯一能把它按住
/// 的地方 —— 剩下的路径需要终端。
fn effective_mode(parsed: &InteractiveArgs, config: &Config) -> Mode {
    parsed.mode.unwrap_or(config.mode)
}

/// 交互式会话：一个工作区、一个渲染器、一个键盘。
///
/// 渲染器在组装之前选定并注入，而同一个控制台端口既服务循环的提示、也服务权限门的询问 —— 这两样东
/// 西来自同一个键盘（spec §19）。
async fn interactive(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_interactive());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_interactive(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("heng: {message}");
            println!("{}", render::wording::help_interactive());
            return ExitCode::FAILURE;
        }
    };
    let mut config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("heng: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };
    // 没登记的模型是启动错误，绝不是悄悄降级。
    if let Err(message) = validate_models(&config) {
        eprintln!("heng: {message}");
        return ExitCode::FAILURE;
    }
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let profile = match config.resolve_model(Some(&model)) {
        Ok((_, profile)) => profile.clone(),
        Err(error) => {
            eprintln!("heng: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!("heng: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("heng: {}", render::wording::startup_cwd(&error.to_string()));
                return ExitCode::FAILURE;
            }
        },
    };
    // 打开哪一场，以及这一趟的工作目录 —— 按 id 续上别的工作区的会话时，cwd 跟着那场会话走。
    let chosen = match choose_session(&store, &cwd, parsed.resume.as_ref()) {
        Ok(chosen) => chosen,
        Err(message) => {
            eprintln!("heng: {}", message);
            return ExitCode::FAILURE;
        }
    };
    let (stored, cwd) = (chosen.stored, chosen.cwd);
    let elsewhere = chosen.elsewhere;
    // 项目级的 MCP 配置跟着这一趟的工作目录走：仓库根的 `.mcp.json` 逐台盖用户级。
    // `[mcp] enabled = false`（缺省）时这一步连文件都不看。
    if let Err(error) = config.load_project_mcp(&cwd) {
        eprintln!(
            "heng: {}",
            render::wording::startup_config(&error.to_string())
        );
        return ExitCode::FAILURE;
    }

    let provider = match OpenAiProvider::build(&config, &model, stderr_warnings()) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("heng: {error}");
            return ExitCode::FAILURE;
        }
    };
    let session_config = match config.session_config(&model) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("heng: {error}");
            return ExitCode::FAILURE;
        }
    };
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    // 目标清单住在数据根下、会话目录的旁边（`.scratch/goal-loop/spec.md` §1）。库不读环境，所以
    // 在这里定下来，随循环一起进去。
    let goals_dir = config::goals_dir(env);

    // 这一趟从哪一档开始：旗标压过文件（spec §12）。在这里一次性定下来，因为有三样东西要同一个答
    // 案 —— 权限门跑在哪一档、横幅、以及状态行。
    let mode = effective_mode(&parsed, &config);

    // 键盘的两端：循环持有的句柄与手势接收端，以及选定的渲染器（或 plain 逐行读取器）为它服务的那
    // 个端口。
    let (console, port, mut events) = render::console();
    let use_tui = parsed.tui || (!parsed.plain && std::io::stdout().is_terminal());
    let renderer = if use_tui {
        // 表头与面板显示这些；它们都不走事件流。模式就是其中之一：它是前端要显示、手势要挪动的一个
        // 会话值，而不再是流承载的东西。窗口是模型的输入预算。上面的 provider 已经解析过同一张表，
        // 所以下面这个失败是双保险：它让没登记的模型在这里也是启动错误，而不是在构建那些事实的地方
        // panic。
        let caps = match caps_for(&model) {
            Ok(caps) => caps,
            Err(error) => {
                eprintln!("heng: {error}");
                return ExitCode::FAILURE;
            }
        };
        let facts = SessionFacts {
            switchable: true,
            session_id: stored.id.as_str().to_owned(),
            session_dir: stored.dir.display().to_string(),
            model: model.clone(),
            context_window: crate::context::usable_input(&caps),
            mode,
            budget_limit: session_config.budget.limit,
            number_style: config.number_style,
            file_viewer: config.file_viewer,
            diff_viewer: config.diff_viewer.clone(),
            // 单 agent 会话以它的档案发言，所以那就是转录的名字配色需要安排的全部名册
            // （票 07 §1）。
            speaker_order: vec![profile.name.clone()],
        };
        Renderer::tui(TuiOptions {
            port,
            facts,
            // 标题的路径段用的就是这一档工作目录 —— `--cwd` 已经解析过了。
            cwd: cwd.clone(),
            // 重新打开的会话会在横幅之前重放它的历史，所以 TUI 必须知道，在那次重放到达之前什么都
            // 不要画（`.scratch/tui-history-replay/spec.md` §1、§3）。
            reopened: parsed.resume.is_some(),
        })
    } else {
        // plain 前端读 stdin；它是行缓冲的，所以没有 raw 模式、也没有按键事件。
        render::spawn_plain_console(port);
        Renderer::plain(PlainOptions {
            sinks: RenderSinks {
                stdout_result: Box::new(std::io::stdout()),
                stderr_diagnostic: Box::new(std::io::stderr()),
            },
            color: std::io::stderr().is_terminal() && env.get("NO_COLOR").is_none(),
        })
    };
    let asker = Arc::new(ConsoleAsker::from_handle(&console));
    // 模型的问题走同一个键盘、走它自己的端口（spec §7）。交互式组装总是有一个 —— TUI 或 plain
    // —— 所以工具表提供 `ask_user_question`，而这张表是按端口在不在建的，不是按第二个容易漂移的
    // 标志建的。
    let questions: Option<Arc<dyn UserQuestions>> =
        Some(Arc::new(ConsoleQuestions::from_handle(&console)));

    // MCP 这一层也在组装期定下：`[mcp] enabled` 与仓库根的 `.mcp.json` 都在这里读一次。
    // 包成 `Arc`：工具表拿一份，`/` 菜单那半边（模板）也要拿一份 —— 同一次连接、同一个值。
    let mcp = Arc::new(mcp_service(&config, &cwd, env, home.as_deref(), questions.clone()).await);
    // 模板清单是**界面层**的东西：它进 `/` 菜单，不进工具表、也不进前缀缓存。server 不可用时
    // 它的条目根本不出现（spec §8）。
    let prompts = mcp_prompt_entries(&mcp).await;

    let mut harness = match assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            // 工具表在这里、在组装处定下：内建的那些加上每一个动态声明的工具（spec §14）。
            tools: tools::with_mcp(
                tools::with_web(
                    tools::with_dynamic(&config.tools, questions.is_some()),
                    web_service(&config),
                ),
                Arc::clone(&mcp),
            ),
            locks: PathLocks::new(),
            // 用户选的那一档：`[permissions] mode`，或者压在它上面的 `--mode`（spec §12）。无头调
            // 用方没有应答者，于是降级。
            policy: Policy::for_mode(mode).with_outside_read(config.outside_read),
            asker: Some(asker),
            questions,
            hook: None,
            home,
        },
        provider: Box::new(provider),
        speaker: SpeakerId::Debater(profile.name.clone().into()),
        config: session_config,
        renderer,
    })
    .await
    {
        Ok(harness) => harness,
        Err(error) => {
            eprintln!("heng: {error}");
            return ExitCode::FAILURE;
        }
    };

    // 重新打开的会话组装时带上的历史，在横幅之前推给前端，好让 TUI 先把它铺好、横幅落在接缝**之
    // 后**，而不是落在接缝中间（`.scratch/tui-history-replay/spec.md` §1）。payload 就是组装好的
    // 那份快照，所以它带着 `--continue` 的恢复为悬空工具调用写下的合成结果。新会话没有历史，所以
    // 什么都不发。
    if parsed.resume.is_some() {
        console.replay(harness.events());
    }

    // MCP 这一层加载成什么样，作为一条上下文注入进流（票 19）：与技能清单同形 —— 模型因此知道
    // 手里有哪些外部 server，而转录里那行**点得开**、详情就是同一段数据。
    if let Some(catalog) = mcp.catalog_text() {
        if let Err(error) = harness.inject_context(ContextSource::McpCatalog, &catalog) {
            eprintln!("heng: {}", render::wording::error_report(&error));
        }
    }

    // 用户故事 A.12：在第一个问题之前说清这是哪个模型、哪一档、哪场会话。它走渲染器而不是 stderr：
    // TUI 在 harness 组装时就已经启动，第二个往终端写的人会落在它的活动区域里、盖在状态行上。
    // 拼给模型的那段私有身份，给读的人留一条记录。它**不进事件流**（身份每次请求现拼、
    // 从不落盘，spec §15），所以轨迹页看到的是「按当前代码拼的一份」—— 那句话由来源名自己
    // 说出来。摆在横幅之前：那是模型看到的**第一样东西**。
    harness.identity(&crate::agent::agent_identity());
    harness.notice(&render::wording::banner(
        harness.session_id().as_str(),
        &model,
        harness.mode(),
        &stored.dir.display().to_string(),
        parsed.resume.is_some(),
    ));
    // 按 id 续上了一场面别的工作区的会话：工作目录跟着它走了，所以先说一句 —— 否则下一步
    // `ls` 出来的东西与横幅上的目录对不上，人会以为是自己敲错了。
    if elsewhere {
        harness.notice(&render::wording::session_followed(
            &cwd.display().to_string(),
        ));
    }

    // `/` 菜单里的名字。真正对提交作出反应的是循环，所以「有哪些名字」也由循环说了算：它解析的那
    // 些内建命令，然后是这场会话发现到的技能 —— 这正是它在这里、在组装之后、第一个提示之前发出，
    // 而不是随表头那些事实一起注入的原因。
    console.catalog(slash_catalog(
        &render::wording::BUILT_IN_COMMANDS,
        &harness.skill_catalog(),
        &prompts,
    ));

    let code = interactive_loop(
        &mut harness,
        &console,
        &mut events,
        &config,
        &GoalSetup {
            dir: goals_dir,
            store: store.clone(),
            cwd: cwd.clone(),
            settings: config.goals,
        },
        parsed.resume.is_some(),
        McpMenu {
            service: &mcp,
            prompts: &prompts,
        },
    )
    .await;
    // 会话 id 先抄下来：`shutdown` 把 harness 收走了，而回执是在那之后才打的。
    let session_id = harness.session_id().as_str().to_owned();
    harness.shutdown().await;
    // 终端交还之后那一行（`.scratch/exit-gesture/spec.md` §5）：stdout 只承载最终产物，
    // 所以回执走 stderr；TUI 与 `--plain` 共用这一处收尾。启动不打它（横幅已经有 id），
    // `probe` / `sessions` 一族也不打。
    finish_session(code, &session_id, &mut std::io::stderr())
}

/// 交互式会话退出时的收尾：在终端交还之后往 `out` 打一行能直接粘的复盘命令，然后**原样**
/// 返回 `code`（`.scratch/exit-gesture/spec.md` §5）。
///
/// 收一个 writer 是为了让它可测 —— 直接在循环里 `eprintln!` 的话没人能断言它，而「打了几
/// 次、打给谁」正是这条需求要钉住的东西。写失败（管道断了）就吞掉：一行回执绝不该改退出码。
pub fn finish_session<W: Write>(code: ExitCode, session_id: &str, out: &mut W) -> ExitCode {
    let _ = writeln!(
        out,
        "heng: {}",
        render::wording::session_receipt(session_id)
    );
    code
}

// ---------------------------------------------------------------------------
// 讨论前端（spec §15）
// ---------------------------------------------------------------------------

/// 一次已解析的 `heng discuss` 调用。
///
/// 刻意没有 `--model`：谁参与讨论是一件配置事实（`[discussion] debaters`），而一个能替换掉两者之
/// 一的旗标会是说同一件事的第二种方式。剩下可选的只是这场讨论怎么看。
#[derive(Debug, Default)]
struct DiscussArgs {
    /// 那个问题，用它被给进来时的那些词。空表示「问 stdin」。
    words: Vec<String>,
    /// `--debaters a,b`：池子里哪两位参与讨论。`None` 表示抽一对。
    debaters: Option<[String; 2]>,
    plain: bool,
    tui: bool,
    config: Option<PathBuf>,
    cwd: Option<PathBuf>,
}

fn parse_discuss(args: &[String]) -> Result<DiscussArgs, String> {
    let mut parsed = DiscussArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plain" => parsed.plain = true,
            "--tui" => parsed.tui = true,
            flag @ ("--config" | "--cwd" | "--debaters") => {
                let flag = flag.to_owned();
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value(&flag))?;
                match flag.as_str() {
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    "--debaters" => parsed.debaters = Some(split_names(value)?),
                    _ => unreachable!(),
                }
            }
            // `--` 之后的一切都是问题，所以一个问题可以以横线开头。
            "--" => {
                parsed.words.extend(args[index + 1..].iter().cloned());
                break;
            }
            other if other.starts_with("--") => {
                return Err(render::wording::unknown_argument(other));
            }
            other => parsed.words.push(other.to_owned()),
        }
        index += 1;
    }
    if parsed.plain && parsed.tui {
        return Err(render::wording::renderers_mutually_exclusive().to_owned());
    }
    Ok(parsed)
}

/// 一个问题、两位讨论者、一个合成器、一场会话。
///
/// 渲染器的选法与交互式那条路完全一样，所以在终端里看一场讨论得到的就是 TUI —— 包括权限模态框，供
/// 讨论者想跑的任何东西 —— 而管道接进来的拿到 plain 转录，stdout 上只有合成器的产出。
async fn discuss(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_discuss());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_discuss(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("heng: {message}");
            println!("{}", render::wording::help_discuss());
            return ExitCode::FAILURE;
        }
    };
    let mut config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("heng: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };
    // 没登记的模型是启动错误，绝不是悄悄降级。
    if let Err(message) = validate_models(&config) {
        eprintln!("heng: {message}");
        return ExitCode::FAILURE;
    }
    // 文件里没有名册不算错误 —— 大多数配置都是给单 agent 会话用的 —— 但对这个子命令算。
    let Some(roster) = config.discussion.clone() else {
        eprintln!("heng: {}", render::wording::discussion_no_roster());
        return ExitCode::FAILURE;
    };
    // 池子里哪两位参与讨论：命令行点了名的，或者抽出来的。
    let pair = match pick_debaters(&roster, parsed.debaters.clone(), discussion_seed()) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("heng: {message}");
            return ExitCode::FAILURE;
        }
    };
    for line in advisory_lines(&config, &pair) {
        eprintln!("heng: {line}");
    }

    // 问题在任何东西被组装之前就要拿到：读它可能在终端上阻塞，而那时终端必须还在 cooked 模式下
    // （组装会把 TUI 切进 raw 模式，并从那一刻起占住屏幕）。
    let Some(question) = question(&parsed.words) else {
        eprintln!("heng: {}", render::wording::discuss_needs_question());
        return ExitCode::FAILURE;
    };

    let Some(root) = config::sessions_dir(env) else {
        eprintln!("heng: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("heng: {}", render::wording::startup_cwd(&error.to_string()));
                return ExitCode::FAILURE;
            }
        },
    };
    // 项目级的 MCP 配置跟着这一趟的工作目录走（spec §5）。
    if let Err(error) = config.load_project_mcp(&cwd) {
        eprintln!(
            "heng: {}",
            render::wording::startup_config(&error.to_string())
        );
        return ExitCode::FAILURE;
    }
    let stored = match SessionStore::new(root).create(&cwd) {
        Ok(session) => session,
        Err(error) => {
            eprintln!(
                "heng: {}",
                render::wording::startup_store_create(&error.to_string())
            );
            return ExitCode::FAILURE;
        }
    };
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    // 每位参与者的 provider 都在渲染器启动之前建好，所以少一个密钥是在一块正常屏幕上的启动错误，
    // 而不是一个画了一半的界面。
    let (debaters, synthesizer) = match discussion_participants(&config, &pair) {
        Ok(parts) => parts,
        Err(message) => {
            eprintln!("heng: {message}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "heng: {}",
        render::wording::discussion_starting(
            &render::wording::debater_label(&pair[0].name, &pair[0].model),
            &render::wording::debater_label(&pair[1].name, &pair[1].model),
            &question,
        )
    );

    let (console, port, mut events) = render::console();
    let use_tui = parsed.tui || (!parsed.plain && std::io::stdout().is_terminal());
    let renderer = if use_tui {
        let caps = match caps_for(&pair[0].model) {
            Ok(caps) => caps,
            Err(error) => {
                eprintln!("heng: {error}");
                return ExitCode::FAILURE;
            }
        };
        let facts = SessionFacts {
            // 讨论 CLI 这条路径上没有可换的模型：`model` 那一格是两个模型的**拼法**，那是名册里
            // 的配置事实，不是一场活会话能中途改的值（`.scratch/model-switching/spec.md` §12）。
            // 点它只给一句说明，`ctrl-t` 与 `/model` 同样回这一句。
            switchable: false,
            session_id: stored.id.as_str().to_owned(),
            session_dir: stored.dir.display().to_string(),
            // 面板只有一行模型和一行上下文，而一场讨论有一对讨论者、没有单一窗口：这一行列出**两个
            // 模型**，窗口是第一位讨论者的 —— 面板也只能是这么个近似（spec §8）。名字是转录的事
            // （`debater_label`）；这一行讲的是模型，所以它显示模型。
            model: render::wording::discussion_pair(&pair[0].model, &pair[1].model),
            context_window: crate::context::usable_input(&caps),
            // `--mode` 属于交互式那条路；讨论读文件。
            mode: config.mode,
            // `session_config` 原样拷贝 `[budget]`，所以文件里的值就是这场会话的值。
            budget_limit: config.budget.limit,
            number_style: config.number_style,
            file_viewer: config.file_viewer,
            diff_viewer: config.diff_viewer.clone(),
            // 这一对，按名册顺序 —— 与 `pick_pair` 产出的顺序相同，正是它把第一个调色板槽位给了第
            // 一位讨论者（票 07 §1）。
            speaker_order: vec![pair[0].name.clone(), pair[1].name.clone()],
        };
        // 一场讨论是一个问题、一个 harness：它从不是一次重新打开，所以没有历史要重放，TUI 从第一
        // 帧起就在画。
        Renderer::tui(TuiOptions {
            port,
            facts,
            cwd: cwd.clone(),
            reopened: false,
        })
    } else {
        render::spawn_plain_console(port);
        Renderer::plain(PlainOptions {
            sinks: RenderSinks {
                stdout_result: Box::new(std::io::stdout()),
                stderr_diagnostic: Box::new(std::io::stderr()),
            },
            color: std::io::stderr().is_terminal() && env.get("NO_COLOR").is_none(),
        })
    };
    // 这个前端从不读一行 —— 一个问题进去、一场讨论出来 —— 所以它一辈子都跑在一次 run 里面，并在
    // 循环还不会开口问之前就说了这一点。在这里告诉它而不是让它推断，理由与
    // `ConsoleRequest::RunState` 记下的那条相同。
    console.set_running(true);
    // 同一个键盘也回答讨论者的权限询问：一场讨论仍然是一场带着工具的会话。
    let asker = Arc::new(ConsoleAsker::from_handle(&console));
    // 讨论者是主会话而不是执行者，所以它也可以问用户（spec §7）；这个端口与权限门用的是同一个键
    // 盘。
    let questions: Option<Arc<dyn UserQuestions>> =
        Some(Arc::new(ConsoleQuestions::from_handle(&console)));

    // 讨论者与任何会话一样拿得到四个 MCP 元工具（spec §9）。
    let mcp = mcp_service(&config, &cwd, env, home.as_deref(), questions.clone()).await;

    let mut harness = match assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd,
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            tools: tools::with_mcp(
                tools::with_web(
                    tools::with_dynamic(&config.tools, questions.is_some()),
                    web_service(&config),
                ),
                mcp,
            ),
            locks: PathLocks::new(),
            // 文件里的模式：讨论者与任何会话一样走同一个权限门，而名册共享一个策略（spec §12、
            // §15）。
            policy: Policy::for_mode(config.mode).with_outside_read(config.outside_read),
            asker: Some(asker),
            questions,
            hook: None,
            home,
        },
        debaters,
        synthesizer,
        max_rounds: roster.max_rounds,
        renderer,
    })
    .await
    {
        Ok(harness) => harness,
        Err(error) => {
            eprintln!("heng: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut quit = ExitRequest::default();
    let outcome = run_discussion(&mut harness, &mut events, &question, &mut quit).await;
    harness.shutdown().await;

    // 这行在 alt screen 恢复之后打印，因为 TUI 的转录活不过进程：用户在屏幕上漏掉的东西，会话 id
    // 是回到它身边的持久途径。
    match outcome {
        Ok(outcome) => {
            eprintln!(
                "heng: {}",
                render::wording::discussion_ended(outcome.reason, outcome.rounds, &outcome.absent)
            );
            eprintln!(
                "heng: {}",
                render::wording::session_receipt(stored.id.as_str())
            );
            // 整场失败掉的讨论是一次失败；被取消的那场正是用户要的，如同交互式会话里被取消的一个回
            // 合（spec §6）。举手退出（`Quit`）走 130，与交互式那条路同一个判定。
            if outcome.reason == StopReason::Error {
                ExitCode::FAILURE
            } else {
                exit_code_after(quit.requested())
            }
        }
        Err(error) => {
            eprintln!("heng: {}", render::wording::error_report(&error));
            ExitCode::FAILURE
        }
    }
}

/// `/discuss [--debaters a,b] [问题]`：这条命令自己的旗标，以及那个问题。
///
/// 旗标只算开头的那些 —— 一旦出现别的东西，这一行剩下的部分（含换行）就是问题，所以一个问题可以
/// 以横线开头，也可以是一整段。
#[derive(Debug)]
struct DiscussLine {
    debaters: Option<[String; 2]>,
    question: String,
}

fn parse_discuss_line(text: &str) -> Result<DiscussLine, String> {
    let mut rest = text.trim_start();
    let mut debaters = None;
    loop {
        let Some((word, tail)) = rest.split_once(char::is_whitespace) else {
            return Ok(DiscussLine {
                debaters,
                question: rest.to_owned(),
            });
        };
        match word {
            "--debaters" | "--debater" => {
                let tail = tail.trim_start();
                match tail.split_once(char::is_whitespace) {
                    Some((value, after)) => {
                        debaters = Some(split_names(value)?);
                        rest = after;
                    }
                    None => {
                        return Ok(DiscussLine {
                            debaters: Some(split_names(tail)?),
                            question: String::new(),
                        });
                    }
                }
            }
            "--" => {
                return Ok(DiscussLine {
                    debaters,
                    question: tail.trim_start().to_owned(),
                });
            }
            other if other.starts_with("--") => {
                return Err(render::wording::unknown_argument(other));
            }
            _ => {
                return Ok(DiscussLine {
                    debaters,
                    question: rest.to_owned(),
                });
            }
        }
    }
}

/// `a,b` 析成两个讨论者名字。
fn split_names(value: &str) -> Result<[String; 2], String> {
    let names: Vec<String> = value
        .split(',')
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    <[String; 2]>::try_from(names).map_err(|_| render::wording::needs_two_debaters(value))
}

/// 一场讨论用的那两位讨论者：点名要的，或者从池子里抽的。
///
/// 抽是默认做法，因为池子存在的意义就是换一对；`seed` 是注入的（运行时是时钟，测试里是常量），所
/// 以这个选择可复现。
fn pick_debaters(
    roster: &DiscussionRoster,
    requested: Option<[String; 2]>,
    seed: u64,
) -> Result<[Debater; 2], String> {
    let lookup = |name: &str| {
        roster
            .debater(name)
            .cloned()
            .ok_or_else(|| render::wording::unknown_debater(name, &roster.names()))
    };
    match requested {
        Some([first, second]) => Ok([lookup(&first)?, lookup(&second)?]),
        None => {
            let (first, second) = crate::discussion::pick_pair(roster.debaters.len(), seed)
                .ok_or_else(|| render::wording::discussion_no_roster().to_owned())?;
            Ok([
                roster.debaters[first].clone(),
                roster.debaters[second].clone(),
            ])
        }
    }
}

/// 在这一对辩论**之前**该说什么：池子里有两位同厂商，意味着拿到的是两个样本而不是两个独立判断。
///
/// 这是允许的 —— 一个订阅到期不该让讨论不可用 —— 但要明说，不能拿它冒充本设计适用的情况
/// （spec §15）。
fn advisory_lines(config: &Config, pair: &[Debater; 2]) -> Vec<String> {
    let [first, second] = pair;
    if first.model == second.model {
        return vec![render::wording::discussion_same_model(
            &render::wording::debater_label(&first.name, &first.model),
        )];
    }
    if config.debaters_share_a_vendor(&first.model, &second.model) {
        return vec![render::wording::discussion_one_vendor(
            &render::wording::debater_label(&first.name, &first.model),
            &render::wording::debater_label(&second.name, &second.model),
        )];
    }
    Vec::new()
}

/// 抽一对用的种子：时钟，按会话 id 后缀的方式混合。
///
/// 不是 PRNG —— 它只决定几个讨论者里哪两位参与 —— 但它必须在同一秒开始的两场讨论之间不同，而纳秒
/// 做到了这一点。
fn discussion_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    hasher.write_u128(nanos);
    hasher.finish()
}

/// 每位参与者一个 provider 与一组会话值，取自名册。
///
/// 两位讨论者用名册里的模型作答；合成器用路由表的落点作答 —— 什么都没路由时就是第一位讨论者的模
/// 型，也就是 `SessionConfig::model_for(Synthesizer)` 在组装内部解析出的那个。把它的 provider 恰
/// 好建在那个模型上，是让 provider 与发给它的配置保持一致的东西。
///
/// 两个入口都用它：`discuss` 子命令（自带的一场讨论）与 `/discuss`（跑在用户所在会话上的一场讨
/// 论）。
fn discussion_participants(
    config: &Config,
    pair: &[Debater; 2],
) -> Result<(Vec<DebaterParts>, SynthesizerParts), String> {
    let build = |model: &str| -> Result<(config::SessionConfig, Box<dyn crate::provider::Provider>), String> {
        let values = config
            .session_config(model)
            .map_err(|error| error.to_string())?;
        let provider = OpenAiProvider::build(config, model, stderr_warnings())
            .map_err(|error| error.to_string())?;
        Ok((values, Box::new(provider)))
    };
    let [first, second] = pair;
    let (first_config, first_provider) = build(&first.model)?;
    let (second_config, second_provider) = build(&second.model)?;
    let synthesizer_model = config
        .routing
        .synthesizer_model
        .clone()
        .unwrap_or_else(|| first.model.clone());
    let (synthesizer_config, synthesizer_provider) = build(&synthesizer_model)?;
    Ok((
        vec![
            DebaterParts {
                // 讨论者的**名字**才是它在流上的身份 —— 不是模型，池子可能同时把同一个模型交给两位
                // 讨论者（spec §5、§15）。
                speaker: SpeakerId::Debater(first.name.as_str().into()),
                config: first_config,
                provider: first_provider,
                soul: first.soul.clone(),
            },
            DebaterParts {
                speaker: SpeakerId::Debater(second.name.as_str().into()),
                config: second_config,
                provider: second_provider,
                soul: second.soul.clone(),
            },
        ],
        SynthesizerParts {
            config: synthesizer_config,
            provider: synthesizer_provider,
        },
    ))
}

/// `/discuss [问题]`：在**当前会话的流上**跑一场讨论（spec §15）。
///
/// 键盘归循环、日志归会话，所以这里从配置里读出名册，为每位参与者建一个 provider，然后把用户已经
/// 在的那场会话交给协议：讨论者是它的兄弟，于是继承它的上下文、轮次落进它的流。用户需要知道的一切
/// —— 哪些模型、什么问题、怎么收尾的 —— 都走渲染器而不是 stderr，因为屏幕归 TUI。
async fn discuss_in_session(
    harness: &mut Harness,
    events: &mut ConsoleEvents,
    config: &Config,
    asked: String,
    quit: &mut ExitRequest,
) -> Result<(), crate::Error> {
    let Some(roster) = config.discussion.clone() else {
        harness.notice(&format!(
            "heng: {}",
            render::wording::discussion_no_roster()
        ));
        return Ok(());
    };
    // 先 `--debaters a,b`，再是问题：旗标属于这条命令，而这一行剩下的部分才是用户要问的东西。
    let line = match parse_discuss_line(&asked) {
        Ok(line) => line,
        Err(message) => {
            harness.notice(&format!("heng: {message}"));
            return Ok(());
        }
    };
    // 一句光秃秃的 `/discuss` 就把这场会话刚被问到的那个东西交给两个模型：「我们刚才在谈什么」正是
    // 值得第二份判断的那个问题。
    let question = if line.question.trim().is_empty() {
        match harness.last_question() {
            Some(last) => last,
            None => {
                harness.notice(&format!(
                    "heng: {}",
                    render::wording::discuss_needs_in_session_question()
                ));
                return Ok(());
            }
        }
    } else {
        line.question
    };
    let pair = match pick_debaters(&roster, line.debaters, discussion_seed()) {
        Ok(pair) => pair,
        Err(message) => {
            harness.notice(&format!("heng: {message}"));
            return Ok(());
        }
    };
    let (debaters, synthesizer) = match discussion_participants(config, &pair) {
        Ok(parts) => parts,
        Err(message) => {
            harness.notice(&format!("heng: {message}"));
            return Ok(());
        }
    };
    for line in advisory_lines(config, &pair) {
        harness.notice(&format!("heng: {line}"));
    }
    harness.notice(&format!(
        "heng: {}",
        render::wording::discussion_starting(
            &render::wording::debater_label(&pair[0].name, &pair[0].model),
            &render::wording::debater_label(&pair[1].name, &pair[1].model),
            &question,
        )
    ));

    let signal = harness.cancel_signal();
    // 模式句柄随行，理由与取消信号相同：那个 run future 借走了 harness，而这个手势无论如何必须够
    // 得到策略。
    let modes = harness.mode_cycle();
    // 同上：`Quit` 只记请求、让这场讨论拿到收尾，130 由 `interactive_loop` 兑现。
    // 这个 future 在它运行的整段时间里借走 harness，所以它自成一个作用域：下面的通告要把 harness
    // 拿回来。
    // 回合 future 借着 harness，所以回执走一个在借用**之前**就克隆下来的渲染句柄
    // （与 `ModeCycle` 同一个理由）。
    let render = harness.render_handle();
    let outcome = {
        let mut run =
            Box::pin(harness.discuss(&question, debaters, synthesizer, roster.max_rounds));
        loop {
            tokio::select! {
                result = &mut run => break result?,
                event = events.recv() => match event {
                    // 模式是权限门每次调用都读的一个值，所以这次按键立刻生效、哪怕是在讨论中途：句
                    // 柄之所以存在，是因为 run future 借走了 harness（spec §12）。
                    Some(FrontEndEvent::CycleMode) => {
                        modes.cycle();
                    }
                    // 运行中要切换：拒绝，并说清为什么（spec §6）。与模式手势不同形 ——
                    // 换模型要换 provider 与请求参数，而一个讨论正在跑着。
                    Some(FrontEndEvent::OpenPicker(_)) => {
                        render.notice(&render::wording::heng(render::wording::switch_busy()));
                    }
                    Some(event) => quit.apply(&event, &signal),
                    // 输入结束让讨论像被取消一样落下来，所以流仍然得到它的收尾。
                    None => signal.cancel(),
                },
            }
        }
    };
    harness.notice(&format!(
        "heng: {}",
        render::wording::discussion_ended(outcome.reason, outcome.rounds, &outcome.absent)
    ));
    Ok(())
}

/// 驱动一场讨论，同时仍然盯着取消手势。
///
/// 形状与 [`run_one_turn`] 相同，理由也相同：讨论占着会话，所以循环没法自己读键盘，改为监听手势。
/// `Quit` 同样只是「取消 + 记账」，进程的收尾与退出码归调用方
/// （`.scratch/exit-gesture/spec.md` §3）。
async fn run_discussion(
    harness: &mut DiscussionHarness,
    events: &mut ConsoleEvents,
    question: &str,
    quit: &mut ExitRequest,
) -> Result<crate::agent::DiscussionOutcome, crate::Error> {
    let signal = harness.cancel_signal();
    let modes = harness.mode_cycle();
    let render = harness.render_handle();
    let mut run = Box::pin(harness.discuss(question));
    loop {
        tokio::select! {
            result = &mut run => return result,
            event = events.recv() => match event {
                // 一个策略盖住三位参与者，所以这与它在别处的是同一个手势（spec §12）。
                Some(FrontEndEvent::CycleMode) => {
                    modes.cycle();
                }
                // 讨论 CLI 这条路径上换不了模型：模型是**名册**里的配置事实，而这一场
                // 讨论是独立会话（spec §12）。所以这一格给的与忙闲无关，是另一句话。
                Some(FrontEndEvent::OpenPicker(_)) => {
                    render.notice(&render::wording::heng(
                        render::wording::switch_not_switchable(),
                    ));
                }
                Some(event) => quit.apply(&event, &signal),
                // 输入结束让讨论像被取消一样落下来，所以流仍然得到它的收尾。
                None => signal.cancel(),
            },
        }
    }
}

/// 那个问题：用它被给进来时的那些词，或者 stdin。
///
/// 终端上给一个提示 —— 还没有任何东西进过 raw 模式，所以 cooked 读仍然可用 —— 而管道读到结尾，这
/// 正是 `echo 问题 | heng discuss` 需要的。
fn question(words: &[String]) -> Option<String> {
    let typed = words.join(" ");
    if !typed.trim().is_empty() {
        return Some(typed);
    }
    if std::io::stdin().is_terminal() {
        eprint!("{}", render::wording::question_prompt());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        return match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim().to_owned()).filter(|question| !question.is_empty()),
        };
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).ok()?;
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// 一次 `/undo` 对前端说的那句话。
///
/// 三种结果**都**要开口：成功那次尤其 —— 它刚把一个文件改回原样，而屏幕上没有别的东西替它
/// 说话，少这一句就与「没东西可撤」长得一模一样（`.scratch/fs-agent-v1/issues/35-undo-receipt.md`）。
fn undo_receipt(outcome: Result<Option<UndoOutcome>, crate::Error>) -> String {
    match outcome {
        Ok(Some(undone)) => render::wording::heng(&render::wording::undone(&undone.path)),
        Ok(None) => render::wording::heng(render::wording::nothing_to_undo()),
        Err(error) => render::wording::heng(&render::wording::error_report(&error)),
    }
}

/// 读一行、跑它、重复 —— 直到用户离开或输入结束。
/// 组装一个新模型：provider、会话配置与它挂在哪个 profile 下。
///
/// 会话中途换模型与换档的**四个入口**（`/model`、`/effort`、点状态行那两格、`ctrl-t`）都从这里
/// 出（spec §7），所以「换完之后下一次请求长什么样」只有一个答案。
///
/// `carried_tokens` 由调用方给（从当前会话读出来）：累计额度是**会话**事实 —— 这一场之前已经
/// 花掉的 token，换模型不该抹掉它（`.scratch/goal-loop/spec.md` §8 的「翻页不重置额度」是同一条
/// 纪律）。其余从配置来的一切（预算、打码器、逐 agent 的旋钮、沙箱）照
/// [`Config::session_config`] 给的走 —— 换模型后预算按新模型的配置走，这是对的。
///
/// `[routing]` 的两个覆盖由 `session_config` 重新填，而不是从上一场会话的副本抄过来：它们是**配置
/// 事实**，换模型不该跟着上一场的模型走（spec「明确不做」第三条）。
fn retarget_to(
    config: &Config,
    model: &str,
    effort: Option<ReasoningEffort>,
    carried_tokens: u64,
) -> Result<Retargeted, String> {
    // 先问能力表：没登记的 model id 那条错误把已知 id 列全了，是这里最有用的一句。
    let caps = caps_for(model).map_err(|error| error.to_string())?;
    let provider = OpenAiProvider::build(config, model, stderr_warnings())
        .map_err(|error| error.to_string())?;
    let profile = config
        .resolve_model(Some(model))
        .map_err(|error| error.to_string())?
        .1
        .name
        .clone();
    let mut session_config = config
        .session_config(model)
        .map_err(|error| error.to_string())?;
    // 档位查的是刚拿到的那张能力表 —— 新模型不认的这一档在这里就落回「默认」，而不是留到回合
    // 中途由适配器告警丢掉。
    if let Some(wanted) = effort {
        session_config.params.reasoning_effort =
            caps.reasoning_efforts.contains(&wanted).then_some(wanted);
    }
    session_config.carried_tokens = carried_tokens;
    Ok(Retargeted {
        provider: Arc::new(provider),
        config: session_config,
        profile,
    })
}

/// [`retarget_to`] 的产物。`profile` 是**发言者名字的来源**（spec §4）。
struct Retargeted {
    provider: Arc<dyn crate::provider::Provider>,
    config: crate::config::SessionConfig,
    profile: String,
}

/// 模型候选（spec §8）：`config.toml` 里**显式配置过**的那些 provider 名下的模型（有密钥）。
///
/// 两条判据都要：
///
/// * **配置里写过的** —— [`Config::configured_providers`]。五个内建 profile 无论有没有被配置都在
///   （`config.toml` 只做覆盖，注释掉 `[providers.kimi-code]` 只是不给覆盖），所以「我配了哪几家」
///   只能另记一份。2026-10-08 维护者定的：注释掉一段就当它不存在。
/// * **有密钥** —— 配置了但没给 key 的那条切过去起不来，那属于配置诊断而不属于这个浮层。
///
/// **当前模型兜底**：一份什么模型段都没写的配置里，上面的两条会列出空清单 —— 而清单里看不到自己
/// 正在用的那个模型是最刺眼的一种不诚实，所以它无条件在列。
///
/// 顺序是 `BTreeMap` 的键序，不是插入序，所以两次列出来的读法一致。
fn model_picker_options(config: &Config, current: &str) -> Vec<PickerOption> {
    config
        .models
        .iter()
        .filter(|(id, model)| {
            *id == current
                || (config.configured_providers.contains(&model.provider)
                    && config
                        .providers
                        .get(&model.provider)
                        .is_some_and(|profile| profile.api_key.is_some()))
        })
        .map(|(id, model)| PickerOption {
            label: id.clone(),
            detail: render::wording::model_detail_profile(&model.provider),
            current: id == current,
            enabled: true,
        })
        .collect()
}

/// 档位候选（spec §8）：这一档（`current: true`）与「默认」（spec §5）。
///
/// **模型自己的档位表**决定有什么可切，所以两家厂商的五档与三档各自不同。表是空的
/// （`MiniMax-M3`、`kimi-for-coding-highspeed`）时给**空清单**：那一格显示 `固定`，前端点它给
/// 一句说明 —— 一个只有「默认」可选的清单会读成「有东西可切」，而那里其实什么也没有。
fn effort_picker_options(model: &str, current: Option<ReasoningEffort>) -> Vec<PickerOption> {
    let Ok(caps) = caps_for(model) else {
        return Vec::new();
    };
    if caps.reasoning_efforts.is_empty() {
        return Vec::new();
    }
    let mut options = vec![PickerOption {
        label: render::wording::effort_default().to_owned(),
        detail: render::wording::effort_default_detail().to_owned(),
        current: current.is_none(),
        enabled: true,
    }];
    // 能力表里那张表已经是弱到强排好的，所以画出来的顺序就是强度顺序。
    options.extend(caps.reasoning_efforts.iter().map(|effort| PickerOption {
        label: effort.as_str().to_owned(),
        detail: String::new(),
        current: current == Some(*effort),
        enabled: true,
    }));
    options
}

/// 当前模型认不认这一档。`Err` 那句把它的档位列全 —— 换模型才可能得到它。
fn effort_supported(model: &str, effort: ReasoningEffort) -> Result<(), String> {
    let caps = caps_for(model).map_err(|error| error.to_string())?;
    if caps.reasoning_efforts.contains(&effort) {
        return Ok(());
    }
    Err(render::wording::effort_unknown_for(
        model,
        &caps
            .reasoning_efforts
            .iter()
            .map(|known| known.as_str())
            .collect::<Vec<_>>(),
    ))
}

/// 档位在回执与状态行上的读法。`None` 是「默认」那一档，它**不是**缺省值（spec §5）。
fn effort_label(effort: Option<ReasoningEffort>) -> String {
    effort
        .map(|effort| effort.as_str().to_owned())
        .unwrap_or_else(|| render::wording::effort_default().to_owned())
}

/// 一次切换的正文：换 provider 与配置、推一条新事实回去（spec §3）。
fn apply_switch(
    harness: &mut Harness,
    console: &ConsoleHandle,
    config: &Config,
    model: &str,
    effort: Option<ReasoningEffort>,
) -> Result<crate::Switched, String> {
    let retargeted = retarget_to(config, model, effort, harness.carried_tokens())?;
    let switched =
        harness.switch_model(retargeted.provider, retargeted.config, &retargeted.profile);
    console.session_update(
        switched.model.clone(),
        switched.effort,
        switched.context_window,
        switched.speakers.clone(),
    );
    Ok(switched)
}

/// 点状态行或按 `ctrl-t` 之后的那条路（spec §7）：算候选 → 发清单 → 等答案 → 切。
///
/// `running` 是忙闲判据（§6），由调用点给 —— 它就是「这个手势到达时有没有东西在跑」这
/// 一件事，运行中给一句回执、空闲才真的开清单。四个入口共用这一个动作，所以「换完之后
/// 下一次请求长什么样」只有一个答案。
///
/// 人取消了（`Esc` 或框外点击）就什么都不发生 —— 一次取消不是一次切换，不给回执。
async fn open_picker(
    harness: &mut Harness,
    console: &ConsoleHandle,
    config: &Config,
    kind: PickerKind,
    running: bool,
) {
    if running {
        harness.notice(&render::wording::heng(render::wording::switch_busy()));
        return;
    }
    let (title, options) = match kind {
        PickerKind::Model => (
            render::wording::picker_title_model(),
            model_picker_options(config, harness.model()),
        ),
        PickerKind::Effort => (
            render::wording::picker_title_effort(),
            effort_picker_options(harness.model(), harness.effort()),
        ),
    };
    let Some(index) = console.picker(title, options).await else {
        return;
    };
    // 清单**现算一遍**（不是复用发出去的那份）：等待期间人可能已经用 `/model` 换过模型，
    // 于是档位那张表已经不同了 —— 拿旧表去切会切到一个新模型不认的档位。判据同样现读
    // `harness`：那才是此刻这一场会话真正的模型与档位。
    let fresh = match kind {
        PickerKind::Model => model_picker_options(config, harness.model()),
        PickerKind::Effort => effort_picker_options(harness.model(), harness.effort()),
    };
    let picked = fresh.get(index);
    let Some(choice) = picked else {
        return;
    };
    // 禁用行点了没反应（前端那一层就已经不响应）；这里再判一次是为了不让一条别人的清单
    // 走成切换。
    if !choice.enabled {
        return;
    }
    switch_to(harness, console, config, choice.label.clone(), kind);
}

/// 切换失败时把那句话给出去 —— 「组不出 provider」「这一档模型不认」都在这儿变成一句提示行。
fn report(harness: &mut Harness, outcome: Result<(), String>) {
    if let Err(message) = outcome {
        harness.notice(&render::wording::heng(&message));
    }
}

/// 切换本身，外加回执。两个命令与两个手势都到这里。
fn switch_to(
    harness: &mut Harness,
    console: &ConsoleHandle,
    config: &Config,
    chosen: String,
    kind: PickerKind,
) {
    // 换模型要把**当前档位**带着走（换档不需要 —— 它自己就是那一档），所以判据现读。
    let receipt = match kind {
        PickerKind::Model => {
            let before = harness.effort();
            switch_to_model(harness, console, config, &chosen, before)
        }
        PickerKind::Effort => switch_to_effort(harness, console, config, &chosen),
    };
    if let Err(message) = receipt {
        harness.notice(&render::wording::heng(&message));
    }
}

/// 换模型。档位照旧带着走 —— 人刚选的那一档不该因为换了模型而丢；新模型不认它时它落回
/// 「默认」，而那件事要说出来（`Switched::effort` 就是它）。
fn switch_to_model(
    harness: &mut Harness,
    console: &ConsoleHandle,
    config: &Config,
    model: &str,
    before: Option<ReasoningEffort>,
) -> Result<(), String> {
    let switched = apply_switch(harness, console, config, model, before)?;
    let mut receipt = render::wording::switched_model(&switched.model);
    if switched.effort != before {
        receipt.push_str(&format!(
            "，{}",
            render::wording::switched_effort(&effort_label(switched.effort))
        ));
    }
    harness.notice(&receipt);
    Ok(())
}

/// 换档。**只**改 `SessionConfig::params` —— provider 不读它（`build_body` 只从
/// `ChatRequest.params` 读），所以这是一个纯配置动作，不需要重建 provider；把它当成与换
/// 模型同一件事，是这里最容易有的误解。
fn switch_to_effort(
    harness: &mut Harness,
    console: &ConsoleHandle,
    config: &Config,
    label: &str,
) -> Result<(), String> {
    let effort = if label == render::wording::effort_default() {
        None
    } else {
        let Some(effort) = ReasoningEffort::from_token(label) else {
            return Err(format!(
                "`{label}` 不是一档思考强度；{}",
                render::wording::effort_usage()
            ));
        };
        Some(effort)
    };
    let model = harness.model().to_owned();
    let switched = apply_switch(harness, console, config, &model, effort)?;
    harness.notice(&render::wording::switched_effort(&effort_label(
        switched.effort,
    )));
    Ok(())
}

async fn interactive_loop(
    harness: &mut Harness,
    console: &ConsoleHandle,
    events: &mut ConsoleEvents,
    config: &Config,
    goals: &GoalSetup,
    resumed: bool,
    menu: McpMenu<'_>,
) -> ExitCode {
    // 「已经有一个 loop 在跑」那条边界读的状态。循环体是同步跑完的，所以正常路径上不可能在它
    // 跑着的时候再收到一次提交 —— 票 06 让输入区在这段时间里禁言，这条状态正是那件事的名字。
    let mut loop_running = false;
    // 退出账本：一次运行中途有人举手退出时，回合先收尾，码最后一起兑现
    // （`.scratch/exit-gesture/spec.md` §3）。
    let mut quit = ExitRequest::default();

    // 恢复（§10）。只对**重新打开**的会话做，而且只对认领过目标的那些：普通交互会话的
    // `--continue` 行为一个字不变。
    if resumed {
        if let Some(name) = harness.current_goal() {
            match harness.resume_goal() {
                // 正常收尾：回来是空闲等人 —— 人主动停是人的意思，该尊重它。
                None => harness.notice(&render::wording::heng(
                    &render::wording::resumed_closed_goal(&name),
                )),
                // 异常中断：接着跑，不必人点头。
                Some(name) => {
                    harness.notice(&render::wording::heng(&render::wording::resumed_goal(
                        &name,
                    )));
                    console.set_running(true);
                    if let Err(message) = run_goal_loop(
                        harness,
                        console,
                        events,
                        goals,
                        &name,
                        &mut loop_running,
                        &mut quit,
                    )
                    .await
                    {
                        harness.notice(&render::wording::heng(&message));
                    }
                    if quit.requested() {
                        return quit.code();
                    }
                }
            }
        }
    }

    loop {
        // 只有循环知道有没有东西在跑，所以它告诉前端，而不是让前端去推断（spec §6）。在拿到一次提
        // 交之前什么都没在跑，而前端从第一帧起就得这么读 —— 包括在这个循环第一次提问之前。
        console.set_running(false);
        // 回合之间循环只在等一次提示；这时候到达的手势在没有回合进行中的情况下处理掉。
        let line = loop {
            tokio::select! {
                line = console.prompt() => break line,
                event = events.recv() => match event {
                    // 一旦运行之外收到退出举手（第一下在忙碌里举的，而回合已经收尾），它
                    // 与输入结束一样是「人要求离开」—— 但码走 130 那一档。
                    Some(event @ (FrontEndEvent::Quit | FrontEndEvent::Cancel)) => {
                        quit.apply(&event, &harness.cancel_signal());
                        if quit.requested() {
                            return quit.code();
                        }
                    }
                    Some(FrontEndEvent::CycleMode) => {
                        harness.mode_cycle().cycle();
                    }
                    // 点状态行那两格或按 `ctrl-t`（spec §7）。这里是**空闲**：这一支的整段
                    // 期间只有提示行在等，没有回合、没有讨论，所以清单可以真的开出来。
                    Some(FrontEndEvent::OpenPicker(kind)) => {
                        open_picker(harness, console, config, kind, false).await;
                    }
                    None => return ExitCode::SUCCESS,
                },
            }
        };
        let Some(submitted) = line else {
            return ExitCode::SUCCESS;
        };
        // 拿到一行了，所以从这里到这个循环的下一次轮转之间，会话正在跑东西 —— 一个回合、一场讨论、
        // 一次 `/undo`。`Ctrl-C` 是它们共同的取消手势。
        console.set_running(true);
        match submission(&submitted, |name| harness.has_skill(name), menu.prompts) {
            // 一个空行：没有可答的，于是再问一次。提示处返回 `None` 是唯一结束输入的东西，而那种情
            // 况就在上面处理。
            Submission::Ignore => {}
            Submission::Quit => return ExitCode::SUCCESS,
            Submission::Undo => {
                let receipt = undo_receipt(harness.undo_last_edit().await);
                harness.notice(&receipt);
            }
            // 一次模板调用：收参数（命令行上的位置参数 + 缺的用问卷问）→ `prompts/get` → 那段
            // 文本**就是这一轮的输入**（server 渲染出来的那条消息）。
            Submission::McpPrompt {
                server,
                prompt,
                inline,
            } => {
                match run_mcp_prompt(console, menu.service, menu.prompts, server, prompt, &inline)
                    .await
                {
                    Ok(Some(text)) => {
                        if let Err(error) =
                            run_one_turn(harness, events, TurnStart::Prompt(&text), &mut quit).await
                        {
                            harness.notice(&format!(
                                "heng: {}",
                                render::wording::error_report(&error)
                            ));
                        }
                    }
                    // 人把这次询问丢掉了：什么都不发生，回到提示行。
                    Ok(None) => {}
                    Err(message) => harness.notice(&format!("heng: {}", message)),
                }
            }
            Submission::Unknown(line) => {
                let names = harness.skill_names();
                harness.notice(&format!(
                    "heng: {}",
                    render::wording::unknown_command(line, &names)
                ));
            }
            // `/<skill> [task]` 是用户侧的技能调用（spec §9）：`disable-model-invocation: true`
            // 的技能为用户留下的那条路。正文进上下文尾部。光秃秃的 `/<skill>` 就地跑 —— 正文**就
            // 是**指令 —— 而打了任务时，**整条原文**（技能名在内）作为一条普通 user 消息跟在它
            // 后面（`.scratch/ui-trim/spec.md` §2）。
            Submission::Skill { name, task, bare } => {
                if bare {
                    harness.notice(&format!("heng: {}", render::wording::skill_started(name)));
                    if let Err(error) =
                        run_one_turn(harness, events, TurnStart::Skill(name), &mut quit).await
                    {
                        harness.notice(&format!("heng: {}", render::wording::error_report(&error)));
                    }
                } else if let Err(error) = harness.load_skill(name) {
                    harness.notice(&format!("heng: {}", render::wording::error_report(&error)));
                } else {
                    harness.notice(&format!("heng: {}", render::wording::skill_loaded(name)));
                    if let Err(error) =
                        run_one_turn(harness, events, TurnStart::Prompt(&task), &mut quit).await
                    {
                        harness.notice(&format!("heng: {}", render::wording::error_report(&error)));
                    }
                }
            }
            // `/discuss [问题]`：在**当前会话的流上**的一场讨论（spec §15）。讨论者是这场会话的兄
            // 弟，所以继承它的上下文，并把轮次追加到它的日志上；之后用户回到提示符前。
            Submission::Discuss(question) => {
                if let Err(error) =
                    discuss_in_session(harness, events, config, question, &mut quit).await
                {
                    harness.notice(&format!("heng: {}", render::wording::error_report(&error)));
                }
            }
            // `/goal-new <名字> <来源>…`：从一批票生成一份目标清单（§2）。手势，不进流。
            Submission::Goal(args) => {
                let message = run_goal_command(harness, goals.dir.as_deref(), &args);
                harness.notice(&render::wording::heng(&message));
            }
            // `/clear`：结束当前会话、开一个新的（§12）。它复用翻页那条机制 —— 两段入口、
            // 一段机制 —— 只是不带压缩、不带摘要注入：人是主动清场，没有「要带过去的历史」
            // 这回事。
            Submission::Clear => {
                if loop_running {
                    // 循环跑着的时候输入区是禁言的，所以这一行本来打不出来；这条拒绝是那件事
                    // 的名字，而不是一条能走到的路径。
                    harness.notice(&render::wording::heng(
                        render::wording::clear_while_looping(),
                    ));
                } else {
                    let message = clear_session(harness, goals);
                    harness.notice(&render::wording::heng(&message));
                }
            }
            // `/loop <名字>`：选定目标并连续工作（§4）。三种启动边界在写任何事件之前判。
            Submission::Loop(args) => {
                let message = match parse_loop_line(&args) {
                    Err(message) => message,
                    Ok(name) => {
                        let result = run_goal_loop(
                            harness,
                            console,
                            events,
                            goals,
                            &name,
                            &mut loop_running,
                            &mut quit,
                        )
                        .await;
                        result.err().unwrap_or_default()
                    }
                };
                if !message.is_empty() {
                    harness.notice(&render::wording::heng(&message));
                }
            }
            // `/model <id>`：换模型。**不带参数不给开弹窗而是说一句用法** —— 选择器是 TUI
            // 独占的交互，而这一路必须能一步到位（plain 前端就靠它换模型，spec §7）。
            Submission::Model(args) => {
                let model = args.trim();
                if model.is_empty() {
                    harness.notice(&render::wording::heng(render::wording::model_usage()));
                } else if let Err(message) =
                    switch_to_model(harness, console, config, model, harness.effort())
                {
                    harness.notice(&render::wording::heng(&message));
                }
            }
            // `/effort <档>`：换档。这一条到达时一定**空闲**（提交只在回合之间被读），所以
            // 不必判忙闲 —— 忙闲是那三个运行中到达的入口的事。
            Submission::Effort(args) => {
                let arg = args.trim();
                if arg.is_empty() {
                    harness.notice(&render::wording::heng(&render::wording::effort_usage()));
                } else if let Some(effort) = ReasoningEffort::from_token(arg) {
                    // 当前模型认不认这一档 —— 换模型才可能得到它，而「它有哪几档」本身就是答案的
                    // 一半，所以这句说清，而不是默默落回「默认」。
                    match effort_supported(harness.model(), effort) {
                        Err(message) => harness.notice(&render::wording::heng(&message)),
                        Ok(()) => {
                            let label = effort.as_str();
                            let outcome = switch_to_effort(harness, console, config, label);
                            report(harness, outcome);
                        }
                    }
                } else {
                    // `默认` 与「不是一档」都由 [`switch_to_effort`] 自己判、自己说 —— 选择器那条
                    // 路与它共用同一段判断，所以命令这一层不重写一遍。
                    let outcome = switch_to_effort(harness, console, config, arg);
                    report(harness, outcome);
                }
            }
            // 其余的都是 prompt，含换行：转录把它显示成用户写下的那一条消息（spec §12）。
            Submission::Prompt(text) => {
                if let Err(error) =
                    run_one_turn(harness, events, TurnStart::Prompt(text), &mut quit).await
                {
                    harness.notice(&format!("heng: {}", render::wording::error_report(&error)));
                }
            }
        }
        // 一次运行中途有人举手退出：那个回合（或那场讨论、那趟目标循环）已经拿到它的收尾，
        // 现在把码兑现 —— 于是渲染器与终端的收尾都会跑（spec §3）。
        if quit.requested() {
            return quit.code();
        }
    }
}

/// 一个记号两侧的文字拼成任务：记号被抹掉，**前文（含它前面的每一行）**与后文接起来。
///
/// `请 /loop 我的目标` → `请 我的目标`；`第一行\n/loop 目标` → `第一行\n目标`（记号与后文
/// 原来在同一行时用一个空格接上，跨行时保留那个换行）。只丢结尾的空白行，所以一段全是空白
/// 的续行不算任务 —— 与旧 `task_of(inline, rest)` 对「记号之后」那半边的规则逐字相同
/// （`.scratch/input-tokens/spec.md` §5）。
fn task_around(text: &str, token: &Token) -> String {
    let (start, end) = token_bytes(text, token);
    let before = &text[..start];
    let after = &text[end..];
    let head = before.trim_end();
    let tail = after.trim_start().trim_end_matches('\n');
    let mut task = head.to_owned();
    if !tail.trim().is_empty() {
        if !task.is_empty() {
            task.push(if before.ends_with('\n') { '\n' } else { ' ' });
        }
        task.push_str(tail);
    }
    task
}

/// 一个记号在 `text` 里的字节范围 —— 记号的字符下标换算一次就够，两处切片都用它。
fn token_bytes(text: &str, token: &Token) -> (usize, usize) {
    (byte_offset(text, token.start), byte_offset(text, token.end))
}

/// 字符下标 → 字节偏移。记号的下标是字符意义上的（`token` 模块），而切片要字节。
fn byte_offset(text: &str, at: usize) -> usize {
    text.char_indices()
        .nth(at)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

/// 一次提交在要什么（spec §12）。
#[derive(Debug, PartialEq, Eq)]
enum Submission<'a> {
    /// 没有要发的东西：一个空行，或者去掉空白后是空的一行。循环不再起一个回合，直接再问一次 —— 在
    /// 空输入上按回车是空行，绝不是输入结束（spec §6）。
    Ignore,
    Quit,
    Undo,
    /// 第一行以 `/` 开头、却什么都没点名，而且后面只有空行：一个错字，也是唯一会被告诉用户的那种
    /// 情况。
    Unknown(&'a str),
    /// `/<skill>` 以及跟在它后面的任务。
    ///
    /// `task` 是**整条原文**，技能名留在里面：`/research 帮我查 X` 发出去的就是这一条，于是
    /// 对话里读得出「这句话是怎么发出去的」（`.scratch/ui-trim/spec.md`）。它曾经是记号
    /// **两侧**的字拼起来的（`task_around`），技能名被丢掉。
    ///
    /// `bare` 说的是「整条草稿只有那一个记号」（`/research`，尾巴上的空白不算）：那时正文
    /// **就是**指令，不再多发一条 user 消息。技能是唯一一个「名字本身就是这句话的一部分」
    /// 的记号，所以内建命令与 MCP 模板的参数仍取记号旁边的字。
    Skill {
        name: &'a str,
        task: String,
        bare: bool,
    },
    /// `/<server>:<模板>`：一次 MCP 提示词模板调用（票 17）。发起者是**人**：它走 `/` 菜单，
    /// 不进工具表。`inline` 是命令行上给的位置参数，缺的那些由循环用问卷补齐。
    McpPrompt {
        server: &'a str,
        prompt: &'a str,
        inline: String,
    },
    /// `/discuss [问题]`：在**当前会话的流上**跑一场讨论（spec §15）。用户没打问题时问题为空 ——
    /// 那时循环把这场会话最后一个问题交给那两位讨论者。
    Discuss(String),
    /// `/goal-new <名字> <来源> [--force]`：从一批票生成一份目标清单
    /// （`.scratch/goal-loop/spec.md` §2）。它是一个**手势**，不是工具调用：由循环直接执行，
    /// 不走工具表、不走权限门、也不进事件流 —— 清单文件存在即是「目标已创建」的证据。
    Goal(String),
    /// `/loop <名字>`：选定目标并连续工作（§4）。参数是目标的名字，一个不断开的词。
    Loop(String),
    /// `/model <model id>`：换这一场会话用的模型（spec §7）。缺参数是**用法错误**而不是
    /// 「打开选择器」—— 选择器是 TUI 独占的交互，命令这一路必须能一步到位（plain 前端就靠
    /// 它换模型）。
    Model(String),
    /// `/effort <档位>`：换思考强度（spec §7）。参数可以是五个档位之一或 `默认`（= 不发这个
    /// 参数）。缺参数同样是用法错误。
    Effort(String),
    /// `/clear`：结束当前会话、开一个新的（`.scratch/goal-loop/spec.md` §12）。**不是**「清空
    /// 上下文继续用」—— 旧会话留在磁盘上，进程不重启，渲染器与终端留着。
    Clear,
    /// 整条提交，含换行，作为一条 prompt。
    Prompt(&'a str),
}

/// 读一次提交。
///
/// 一条命令是**整个草稿里**任意行、任意位置上**最靠左**的那个 `/name`，其中 `name` 在命令表里
/// （内建命令、技能、MCP 模板的并集）。记号前面的字（含它前面的每一行）与它后面的字**一起**成为
/// 任务，所以 `请 /loop 我的目标` 读作 `Loop("请 我的目标")` —— 内建命令过去那条「要么整条、要么
/// 什么都不算」的规矩要防的是丢用户写下的字，而这条规则同样一个字都不丢
/// （`.scratch/input-tokens/spec.md` §5）。
///
/// **这一档是有牙的**（访谈里明码标价选下的）：`@` 被误认只是个引用，而 `/` 被误认**会真的
/// 执行** —— `用 /loop 做目标循环` 就是一条命令。别自作主张加一条「整行起头」的守卫。
///
/// 无参命令（`/quit`/`/exit`/`/undo`/`/clear`）仍要求**整条只有它**，于是「请把 `/clear` 加到文档
/// 里」是一条普通消息、不会被劫持成一次清空。草稿里一个命令记号都没有时整条仍当普通消息；唯一的
/// 例外是一个以 `/` 开头、占满整行却不认识的名字 —— 那是错字，会告诉用户。
///
/// 去掉空白后为空的提交是 [`Submission::Ignore`]：没有要发的东西，所以循环再问一次，而不是起一个回
/// 合。
///
/// 开头的斜杠一律剥掉（`//undo` 读作 `undo`）。任务结尾的空白行丢掉；其余按原样保留。
fn submission<'a>(
    text: &'a str,
    has_skill: impl Fn(&str) -> bool,
    prompts: &[McpPromptEntry],
) -> Submission<'a> {
    if text.trim().is_empty() {
        return Submission::Ignore;
    }
    let is_command = |name: &str| {
        render::wording::BUILT_IN_COMMANDS
            .iter()
            .any(|command| command.name == name)
            || has_skill(name)
            || prompts.iter().any(|entry| entry.name == name)
    };
    let token = token::tokens(text)
        .into_iter()
        .filter(|token| token.prefix == '/')
        .find(|token| is_command(command_name(text, token)));

    let Some(token) = token else {
        // 一个命令记号都没有：整条还是普通消息。唯一的例外是占满整行的一个不认识的名字 ——
        // 那是错字，一直会被告诉用户。
        if text.trim_start().starts_with('/') && !text.trim().contains('\n') {
            return Submission::Unknown(text.trim());
        }
        return Submission::Prompt(text);
    };

    let name = command_name(text, &token);
    // 整条草稿只有那**一个**记号（尾巴上的空白不算）。两类判断都从它出发：内建命令与 `bare`
    // 的技能调用都是「光秃秃的一个名字」。
    let only_token = text.trim().chars().count() == token.end - token.start;
    // 内建命令没有任务来装记号旁边的字，所以它们还多要一条：记号写成 `/<名字>`（没有多出来的
    // 斜杠 —— `//undo` 不算）。技能不受多出来那个斜杠的限制，名字本来就是剥掉斜杠之后才比的。
    let whole = only_token && token.query == name;
    if whole {
        match name {
            "quit" | "exit" => return Submission::Quit,
            "undo" => return Submission::Undo,
            "clear" => return Submission::Clear,
            _ => {}
        }
    }
    let task = task_around(text, &token);
    match name {
        // `/discuss [问题]`：这一条的问题就是记号旁边那一整段。光秃秃的 `/discuss` 把问题留给
        // 循环。
        "discuss" => Submission::Discuss(task),
        // `/goal-new <名字> <来源>`：记号旁边那一整段都是参数。`/goal` 这一族用**连字符**写成
        // 一条命令（将来的 `/goal-list` 等照走），于是 `/goal-` 一个前缀就能在 `/` 菜单里把它们
        // 全列出来 —— 空格形状的子命令补不出来。
        "goal-new" => Submission::Goal(task),
        // `/loop <名字>`：一个参数，形状与 `/goal-new` 相同。
        "loop" => Submission::Loop(task),
        // `/model <id>` 与 `/effort <档>`：换模型与换档。缺参数在这里**不**拦 —— 循环给一句
        // 用法，与 `parse_goal_new_line` 缺参数是同一个形状（`Err(())` 然后一句话）。
        "model" => Submission::Model(task),
        "effort" => Submission::Effort(task),
        _ => {
            // `/<server>:<模板>`：模板条目是运行时才知道的名字，所以这里问的是那份菜单
            // （票 17）。冒号是它与人打出来的技能名的分界。
            if let Some((server, prompt)) = name.split_once(':') {
                if prompts.iter().any(|entry| entry.name == name) {
                    return Submission::McpPrompt {
                        server,
                        prompt,
                        inline: task,
                    };
                }
            }
            if has_skill(name) {
                Submission::Skill {
                    name,
                    task: text.trim_end().to_owned(),
                    bare: only_token,
                }
            } else {
                // 一个内建命令带着记号之外的字：整条仍是一条普通消息，用户写下的东西一个字
                // 都不会被丢掉。
                Submission::Prompt(text)
            }
        }
    }
}

/// 一个 `/` 记号的命令名：记号里那一段剥掉开头的斜杠（`//undo` 读作 `undo`）。
fn command_name<'a>(text: &'a str, token: &Token) -> &'a str {
    let (start, end) = token_bytes(text, token);
    text[start..end].trim_start_matches('/')
}

/// 还缺哪些**必填**参数。
fn missing_required_arguments<'a>(
    prompt: &'a mcp::PromptSummary,
    arguments: &serde_json::Map<String, serde_json::Value>,
) -> Vec<&'a str> {
    prompt
        .arguments
        .iter()
        .filter(|argument| argument.required && !arguments.contains_key(&argument.name))
        .map(|argument| argument.name.as_str())
        .collect()
}

/// 把一次 `/` 菜单里的模板调用跑完（票 17）。
///
/// 三步：命令行上的位置参数按声明顺序填进去 → 缺的用**同一个**问询端口逐项问 → `prompts/get`。
/// 返回的文本会成为这一轮的一条 `user` 消息；`Ok(None)` 表示人把这次询问丢掉了。
async fn run_mcp_prompt(
    console: &ConsoleHandle,
    mcp: &McpService,
    prompts: &[McpPromptEntry],
    server: &str,
    prompt: &str,
    inline: &str,
) -> Result<Option<String>, String> {
    let Some(entry) = prompts
        .iter()
        .find(|entry| entry.server == server && entry.prompt.name == prompt)
    else {
        // 正常情况下到不了这里：`submission` 只在菜单清单里有的名字上产生这个变体。留着是
        // 因为它是一句人话，而不是一次 panic。
        return Err(render::wording::mcp_prompt_unknown(server, prompt));
    };

    let mut arguments = prompt_inline_arguments(&entry.name, &entry.prompt, inline)?;

    // 缺的参数逐项问：走的是 `ask_user_question` 那条通道，不是第二套界面。
    let missing: Vec<mcp::PromptArgument> = entry
        .prompt
        .arguments
        .iter()
        .filter(|argument| !arguments.contains_key(&argument.name))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let questions: Vec<UserQuestion> = missing
            .iter()
            .map(|argument| UserQuestion {
                id: argument.name.clone(),
                question: argument
                    .description
                    .clone()
                    .unwrap_or_else(|| argument.name.clone()),
                header: Some(entry.name.clone()),
                options: Vec::new(),
                multi_select: false,
            })
            .collect();
        let port = ConsoleQuestions::from_handle(console);
        let answers = match port.ask(&questions).await {
            Ok(answers) => answers,
            // 人把这次询问丢掉了（输入结束，或者这次运行被取消）：不发这次调用。
            Err(_) => return Ok(None),
        };
        for question in &questions {
            if let Some(answer) = answers
                .answers
                .iter()
                .find(|answer| answer.id == question.id)
            {
                if let Some(value) = answer
                    .custom
                    .clone()
                    .or_else(|| answer.selected.first().cloned())
                {
                    arguments.insert(question.id.clone(), serde_json::Value::String(value));
                }
            }
        }
    }

    let missing_required = missing_required_arguments(&entry.prompt, &arguments);
    if !missing_required.is_empty() {
        return Err(render::wording::mcp_prompt_missing_arguments(
            &entry.name,
            &missing_required,
        ));
    }

    mcp.get_prompt(server, prompt, serde_json::Value::Object(arguments))
        .await
        .map(Some)
        .map_err(|error| error.to_string())
}

/// `/` 菜单的条目（`.scratch/goal-loop/spec.md` §13）：**内建命令在前、技能在后**。
///
/// 命令是程序自带的、技能是用户装的，所以固定项优先更可预测：打一个 `/c` 时 `/clear` 排在
/// 用户那个恰好也叫 `c…` 的技能前面。命令按声明的固定顺序（不按字母），技能按名字。
///
/// 每一条带着它的**类别**（`.scratch/tui-feedback/spec.md` §11）：三样东西共用一个菜单，
/// 而它们该看得出是三类 —— 名字借类别上色，次序与分派一个字不改。
///
/// 抽出这个纯函数是为了让这条次序有地方断言 —— 它原来内联在组装点里，只能靠真终端看。
fn slash_catalog(
    commands: &[render::wording::Command],
    skills: &[(&str, &str)],
    prompts: &[McpPromptEntry],
) -> Vec<render::CatalogEntry> {
    let mut skills: Vec<&(&str, &str)> = skills.iter().collect();
    skills.sort_by(|left, right| left.0.cmp(right.0));
    commands
        .iter()
        .map(|command| render::CatalogEntry::command(command.name, command.description))
        .chain(
            skills
                .into_iter()
                .map(|(name, description)| render::CatalogEntry::skill(*name, *description)),
        )
        // 菜单的**动态那一半**：MCP server 的提示词模板（票 17）。它不进工具表、也不进前缀
        // 缓存 —— 这里是它唯一的落点。
        .chain(
            prompts
                .iter()
                .map(|entry| render::CatalogEntry::template(&entry.name, entry.description())),
        )
        .collect()
}

/// 命令行上的位置参数按模板的声明顺序填进参数表；多出来的那几个是错误。
fn prompt_inline_arguments(
    display: &str,
    prompt: &mcp::PromptSummary,
    inline: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut arguments = serde_json::Map::new();
    let mut words = inline.split_whitespace();
    for argument in &prompt.arguments {
        let Some(word) = words.next() else { break };
        arguments.insert(
            argument.name.clone(),
            serde_json::Value::String(word.to_owned()),
        );
    }
    if words.next().is_some() {
        return Err(render::wording::mcp_prompt_too_many_arguments(display));
    }
    Ok(arguments)
}

/// `/` 菜单的动态那一半：连接与这一轮的模板条目。
///
/// 打包成一个值，只因为 `interactive_loop` 的参数已经够多了 —— 这两样总是成对出现。
struct McpMenu<'a> {
    service: &'a McpService,
    prompts: &'a [McpPromptEntry],
}

/// `/` 菜单里的一条 MCP 模板：`/<server>:<模板名>`。
struct McpPromptEntry {
    server: String,
    prompt: mcp::PromptSummary,
    /// 菜单上那个**不带斜杠**的名字，与 `submission` 解析时看到的一样。
    name: String,
}

impl McpPromptEntry {
    fn new(server: String, prompt: mcp::PromptSummary) -> Self {
        let name = format!("{server}:{}", prompt.name);
        Self {
            server,
            prompt,
            name,
        }
    }

    /// 菜单第二列：模板自己的说明；它没写说明时，把参数名列一遍。
    fn description(&self) -> String {
        if let Some(description) = self.prompt.description.as_deref() {
            let description = description.trim();
            if !description.is_empty() {
                return description.to_owned();
            }
        }
        let names: Vec<&str> = self
            .prompt
            .arguments
            .iter()
            .map(|argument| argument.name.as_str())
            .collect();
        render::wording::mcp_prompt_description(&self.server, &names)
    }
}

/// 组装后拉一次模板清单 —— `/` 菜单的动态那一半。
///
/// **不在前缀缓存里**：它进的是菜单，不是工具表。失败的 server 直接跳过（它的模板条目根本
/// 不出现），而不是出现之后点开报错（spec §8）。
async fn mcp_prompt_entries(service: &McpService) -> Vec<McpPromptEntry> {
    service
        .prompt_entries()
        .await
        .into_iter()
        .map(|(server, prompt)| McpPromptEntry::new(server, prompt))
        .collect()
}

/// `/goal-new <名字> <来源>… [--force]`：从一批票生成一份目标清单
/// （`.scratch/goal-loop/spec.md` §2）。来源可以是**一个或多个**（空格分开）。
///
/// 返回要说给人听的那句话。**手势，不是工具调用**：它不走工具表、不走权限门、也不写事件 ——
/// 清单文件存在即是「目标已创建」的证据。
fn run_goal_command(harness: &Harness, goals_dir: Option<&Path>, args: &str) -> String {
    let Some(dir) = goals_dir else {
        return render::wording::no_goal_dir().to_owned();
    };
    let Ok(line) = parse_goal_new_line(args) else {
        return render::wording::goal_usage().to_owned();
    };
    // 每个来源相对于**会话**的 cwd 解析，而不是进程的：`--cwd` 可以指向别处。
    let sources: Vec<PathBuf> = line
        .sources
        .iter()
        .map(|source| {
            if Path::new(source).is_absolute() {
                PathBuf::from(source)
            } else {
                harness.cwd().join(source)
            }
        })
        .collect();
    match crate::goals::create(&line.name, &sources, dir, line.force) {
        Ok((manifest, path)) => render::wording::goal_created(
            &manifest.name,
            manifest.entries.len(),
            &path.display().to_string(),
        ),
        Err(error) => error.to_string(),
    }
}

/// `/goal-new` 那一行参数：`<名字> <来源>… [--force]`。
#[derive(Debug, PartialEq, Eq)]
struct GoalNewLine {
    name: String,
    /// 一个或多个来源，按写下的顺序。每个来源是一条路径（**不能带空格**：空格是来源之间的分隔）。
    sources: Vec<String>,
    force: bool,
}

/// 解析 `/goal-new` 的参数。形如 `--force` 的旗标可以出现在任何位置；第一个词是目标名，剩下的
/// 每一个词是一个来源 —— 于是「一个目标装几个 feature」是一条命令。
fn parse_goal_new_line(args: &str) -> Result<GoalNewLine, ()> {
    let mut force = false;
    let mut words: Vec<&str> = Vec::new();
    for word in args.split_whitespace() {
        if word == "--force" {
            force = true;
        } else {
            words.push(word);
        }
    }
    let [name, sources @ ..] = words.as_slice() else {
        return Err(());
    };
    if sources.is_empty() {
        return Err(());
    }
    Ok(GoalNewLine {
        name: (*name).to_owned(),
        sources: sources.iter().map(|source| (*source).to_owned()).collect(),
        force,
    })
}

/// 目标那一族命令要的环境事实：清单目录、会话桶、cwd。
struct GoalSetup {
    /// 目标清单目录；没有数据根时是 `None`（`/goal-new` 与 `/loop` 都照说）。
    dir: Option<PathBuf>,
    store: SessionStore,
    cwd: PathBuf,
    /// 两个阈值（§6）。
    settings: crate::config::GoalSettings,
}

/// `/loop <名字>` 的一次运行（`.scratch/goal-loop/spec.md` §4）。
///
/// 四条启动边界在**写任何事件之前**判；通过之后先落一条 `GoalSelected`，把清单注入上下文，
/// 然后一个回合接一个回合地跑，每个**回合边界**上依次判：完成（§1、§11）→ 阈值（§6、§7）→
/// 无进展（§9），而额度那道闸门在每回合开头自己判（§8）。走到循环末尾只剩一个原因：手势。
///
/// `running` 是这个会话「有没有一个 loop 在跑」的状态，由调用方持有：它跨过整次运行，也正是
/// 第三条启动边界读的那个值。
///
/// `Err` 是一句说给人听的拒绝或失败。
async fn run_goal_loop(
    harness: &mut Harness,
    console: &ConsoleHandle,
    events: &mut ConsoleEvents,
    goals: &GoalSetup,
    name: &str,
    running: &mut bool,
    quit: &mut ExitRequest,
) -> Result<(), String> {
    let Some(dir) = goals.dir.as_deref() else {
        return Err(render::wording::no_goal_dir().to_owned());
    };
    // 读不出来分两种：清单不在（提示先 `/goal-new`），或者它是人手改坏的文件（照说坏在哪）。
    let loaded = crate::goals::load(dir, name);
    let broken = match &loaded {
        Ok(_) | Err(crate::goals::GoalError::Read { .. }) => None,
        Err(error) => Some(error.to_string()),
    };
    let manifest = loaded.as_ref().ok();
    let progress = manifest.map(|manifest| goal_progress(goals, name, manifest));
    let complete = progress
        .as_ref()
        .is_some_and(crate::goals::Progress::is_complete);
    let unattended = harness.mode().allows_unattended();
    if let Err(refusal) = crate::goals::check_start(manifest, complete, *running, unattended) {
        return Err(match refusal {
            crate::goals::StartRefusal::AlreadyRunning => {
                render::wording::loop_already_running(name)
            }
            crate::goals::StartRefusal::Unattended => render::wording::loop_needs_unattended_mode(
                render::wording::mode_label(harness.mode()),
            ),
            crate::goals::StartRefusal::Unknown => {
                broken.unwrap_or_else(|| render::wording::loop_unknown_goal(name))
            }
            crate::goals::StartRefusal::NoWork => render::wording::loop_no_work(name),
        });
    }
    // 三条都过了：从那句提示开始，输入区就禁言了 —— 它由这只闩管着，离开作用域就解除，于是
    // 每一条返回路径都恢复得回来（§5）。
    let _muted = Muted::new(console);
    harness.notice(render::wording::input_muted());
    let manifest = manifest.expect("check_start 放行就意味着清单在");
    let progress = progress.expect("清单在就有进度");

    // 预算的口径（§8）：这个目标**别处**已经花掉的，先填进这场会话 —— 当前会话自己那份不算
    // 在里面，它就在自己这条流上。
    let current = harness.session_id().as_str().to_owned();
    harness.carry_usage(goal_usage(goals, name, &current));
    // 归属先落流：进度重算、预算与恢复都从它派生（§4、§8、§10）。
    if let Err(error) = harness.select_goal(name) {
        return Err(render::wording::error_report(&error));
    }
    // 清单是封闭的，所以模型得在开跑时就知道上面有哪些条目 —— 它不能往里加东西，只能按 id
    // 标完成（§1、§3）。
    if let Err(error) =
        harness.inject_context(crate::events::ContextSource::Goal, &manifest.render())
    {
        return Err(render::wording::error_report(&error));
    }
    harness.notice(&render::wording::goal_loop_started(
        name,
        progress.completed(),
        progress.total(),
    ));
    // 越界的 id 被忽略，但绝不静默（§3）。
    let mut reported_unknown: Vec<String> = Vec::new();
    report_unknown_goal_ids(harness, name, &progress.unknown, &mut reported_unknown);

    // 无进展的计数（§9）：连续几次翻页零条目完成。它从这一刻的进度起算。
    let mut no_progress = crate::goals::NoProgress::new(progress.completed());
    // provider 失败的重试预算（§9）。
    let mut retry = crate::goals::Retry::new(goals.settings.provider_retries);
    // 「这一档已经提醒过」这个跨回合的标记（§6）：跨过阈值只注入一次，翻页时复位。
    let mut reminded = false;
    *running = true;
    loop {
        if harness.cancel_signal().is_cancelled() {
            break;
        }
        let outcome = match run_one_turn(harness, events, TurnStart::Injected, quit).await {
            Ok(outcome) => outcome,
            // 连驱动都失败（会话 i/o，不是 provider）：这不是重试能修的那一类，所以照说、
            // 收工。流上没有收尾事件，于是 `--continue` 会把它读成一次异常中断 —— 对一次
            // i/o 失败来说正是该有的读法。
            Err(error) => {
                *running = false;
                return Err(render::wording::error_report(&error));
            }
        };
        // provider 调用失败：重试若干次，耗尽后停下并写一条收尾事件（§9）。
        if outcome.reason == StopReason::Error {
            if retry.another_attempt() {
                harness.notice(&render::wording::provider_retry(retry.failures()));
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }
            *running = false;
            return stop_goal_loop(
                harness,
                name,
                crate::events::GoalStopReason::ProviderFailed,
                retry.failures(),
                manifest,
                goals,
            );
        }
        // 撞顶（§8）：沿用既有语义 —— **降级收尾，而不是中断**，并落一条收尾事件，于是恢复
        // （§10）分得出这一次与一次崩溃。
        if outcome.reason == StopReason::BudgetExhausted {
            *running = false;
            return stop_goal_loop(
                harness,
                name,
                crate::events::GoalStopReason::BudgetExhausted,
                0,
                manifest,
                goals,
            );
        }
        // 中途成功一次：计数归零。
        retry.succeeded();
        if harness.cancel_signal().is_cancelled() {
            break;
        }

        // 回合边界上重算一次进度（§1）：判据是机械的，循环自己判，不需要人点头。一个回合是一个
        // 不可分的步子，所以「每次 `todo` 落地之后」这一刻就是它结束的时候。
        //
        // 信任假设：`completed` 是**模型自己填的**，所以判据机械**不等于**结果可靠。
        let streams = goal_streams(goals, name);
        let calls: Vec<crate::goals::TodoCall> = streams
            .iter()
            .flat_map(|events| crate::goals::todo_calls(events))
            .collect();
        let progress = crate::goals::progress(&manifest.entries, &calls);
        // 运行中新冒出来的越界 id 也要有人告诉模型（§3）。
        report_unknown_goal_ids(harness, name, &progress.unknown, &mut reported_unknown);
        if progress.is_complete() {
            // 收尾汇总由一次模型调用写（§11），而那一次调用照记用量 —— 它落进目标预算。调用
            // 没成不改变「做完了」这个事实，缺的只是那段叙述，所以退回一份机械的说明。
            let notes: Vec<String> = streams
                .iter()
                .flat_map(|events| crate::goals::notes_of(events))
                .collect();
            if let Err(error) = harness
                .finish_goal(name, manifest, &progress, streams.len(), &notes)
                .await
            {
                *running = false;
                return Err(render::wording::error_report(&error));
            }
            harness.notice(&render::wording::goal_completed_notice(
                name,
                progress.completed(),
                progress.total(),
                streams.len(),
            ));
            *running = false;
            return Ok(());
        }

        // 阈值（§6、§7）：判据取**投影前**的估算，在回合边界比对 —— 不能在一个工具跑到一半
        // 的时候判。
        let percent = harness.context_percent();
        match crate::goals::threshold_step(
            percent,
            goals.settings.remind_at,
            goals.settings.compact_at,
            reminded,
        ) {
            crate::goals::ThresholdStep::None => {}
            crate::goals::ThresholdStep::Remind => {
                // 跨过阈值时注入**一次**，不是每轮：每轮注入会每轮打掉前缀缓存，而本仓库有
                // 「前缀只增不改」的不变量。
                if let Err(error) = harness.inject_context(
                    crate::events::ContextSource::Reminder,
                    render::wording::goal_reminder(),
                ) {
                    *running = false;
                    return Err(render::wording::error_report(&error));
                }
                reminded = true;
                harness.notice(&render::wording::goal_reminded(percent));
            }
            crate::goals::ThresholdStep::Compact => {
                // 压缩与翻页是一个动作，总是成对发生（§7）。开一个新会话 —— 旧的留在磁盘上。
                let from = harness.session_id().as_str().to_owned();
                let stored = match goals.store.create(&goals.cwd) {
                    Ok(stored) => stored,
                    Err(error) => {
                        *running = false;
                        return Err(format!("开不了新会话：{error}"));
                    }
                };
                if let Err(error) = harness.compact_and_rollover(&stored).await {
                    *running = false;
                    return Err(render::wording::error_report(&error));
                }
                // 额度不随翻页重置（§8）：把到现在为止属于这个目标的用量一起填给新会话（新会话
                // 自己还是空的，所以那些就是「别处」）。
                let current = harness.session_id().as_str().to_owned();
                harness.carry_usage(goal_usage(goals, name, &current));
                // 新会话要重新认领这个目标 —— 否则进度派生看不见它；清单也重新摆一次，那是它
                // 照做的定义本身。
                if let Err(error) = harness.select_goal(name) {
                    *running = false;
                    return Err(render::wording::error_report(&error));
                }
                if let Err(error) =
                    harness.inject_context(crate::events::ContextSource::Goal, &manifest.render())
                {
                    *running = false;
                    return Err(render::wording::error_report(&error));
                }
                // 新窗口：过线时还会再提醒一次 —— 那是**新会话**的提醒，正确。
                reminded = false;
                harness.notice(&render::wording::goal_rolled_over(
                    &from,
                    stored.id.as_str(),
                    true,
                ));

                // 无进展（§9）：**连续 N 次翻页零条目完成**就停下报告。中途完成任意一条就归零，
                // 所以这里比的是「这次翻页前后完成数有没有涨」。
                let after = goal_progress(goals, name, manifest);
                let streak = no_progress.after_rollover(after.completed());
                if no_progress.reached(goals.settings.no_progress_rollovers) {
                    *running = false;
                    return stop_goal_loop(
                        harness,
                        name,
                        crate::events::GoalStopReason::NoProgress,
                        streak,
                        manifest,
                        goals,
                    );
                }
            }
        }
    }
    // 走到这里只有一个原因：**手势**。取消可能是 `Esc` 确认框里选了「停下」，也可能是
    // `Ctrl-C` 或输入结束 —— 三种都是人的意思，所以落一条「人主动停」的收尾事件（§5、§10）。
    // 它让 `--continue` 分得出「人停的」与「崩掉的」：前者回来别自己又跑起来。
    *running = false;
    stop_goal_loop(
        harness,
        name,
        crate::events::GoalStopReason::UserStopped,
        0,
        manifest,
        goals,
    )
}

/// 一个目标当下的进度：扫本桶，按归属筛出属于它的会话，再把它们的 `todo` 调用合并起来
/// （§3、§4）。
///
/// 派生的，不新增任何状态文件 —— 与日账本扫会话文件是同一条路。
fn goal_progress(
    goals: &GoalSetup,
    name: &str,
    manifest: &crate::goals::Manifest,
) -> crate::goals::Progress {
    let calls: Vec<crate::goals::TodoCall> = goal_streams(goals, name)
        .iter()
        .flat_map(|events| crate::goals::todo_calls(events))
        .collect();
    crate::goals::progress(&manifest.entries, &calls)
}

/// 这个目标在**别处**已经花掉的 token（§8）：该目标下**除当前会话之外**那些会话的和。
///
/// 派生的：扫本桶、按归属筛出属于它的会话，把每条流的 `UsageRecorded` 加起来。不新增状态文件
/// —— 与日账本扫会话文件按日聚合是同一条路。
///
/// 当前会话被**排除**掉了，而那正是这个数的定义：闸门读的是「整条流的求和 + 别处已经花掉的」，
/// 当前会话自己的花费已经在它自己那条流里了。把它算进来，一道闸门就会把同一笔钱数两遍 ——
/// `--continue` 回到一条已经认领过目标的流时、以及同一个会话第二次 `/loop` 同一个目标时，
/// 都会走到那条路上。
fn goal_usage(goals: &GoalSetup, name: &str, current: &str) -> u64 {
    let Ok(sessions) = goals.store.list(&goals.cwd) else {
        return 0;
    };
    let mut claimed: Vec<(String, Vec<Event>)> = Vec::new();
    for session in sessions {
        let Ok(events) = read_events(&session.log_path) else {
            continue;
        };
        if crate::goals::has_goal(&events, name) {
            claimed.push((session.id.as_str().to_owned(), events));
        }
    }
    crate::goals::usage_apart_from(&claimed, current)
}

/// 属于这个目标的那些会话的流（§4 的归属筛）：扫本桶、按 `GoalSelected` 过滤。
///
/// 扫的是磁盘上的会话文件，所以进行中的这一场也在里面 —— 它的每一行都已经刷下去了。
fn goal_streams(goals: &GoalSetup, name: &str) -> Vec<Vec<Event>> {
    let Ok(sessions) = goals.store.list(&goals.cwd) else {
        return Vec::new();
    };
    let mut streams = Vec::new();
    for session in sessions {
        let Ok(events) = read_events(&session.log_path) else {
            continue;
        };
        if crate::goals::has_goal(&events, name) {
            streams.push(events);
        }
    }
    streams
}

/// `/clear`：结束当前会话、开一个新的（`.scratch/goal-loop/spec.md` §12）。
///
/// 它调的就是翻页那条机制（[`Harness::rollover`]），差别只有一处：**不带压缩、不带摘要注入**
/// —— 人是主动清场，没有「要带过去的历史」这回事。旧会话留在磁盘上，`--continue` 打开的是
/// 最新的那一场。
///
/// 手势本身不进事件流：这件事没有流上的痕迹，审计看的是**会话边界**（新会话的第一条
/// `SessionStarted`）。
fn clear_session(harness: &mut Harness, goals: &GoalSetup) -> String {
    let from = harness.session_id().as_str().to_owned();
    let stored = match goals.store.create(&goals.cwd) {
        Ok(stored) => stored,
        Err(error) => return format!("开不了新会话：{error}"),
    };
    if let Err(error) = harness.rollover(&stored) {
        return render::wording::error_report(&error);
    }
    render::wording::cleared(&from, stored.id.as_str())
}

/// 输入区禁言的那只闩（`.scratch/goal-loop/spec.md` §5）。
///
/// 它一造出来就禁言，一离开作用域就解除 —— 于是「循环的每一条返回路径都恢复得回来」是构造上
/// 的性质，而不是一句要人记住的规矩。
struct Muted<'a>(&'a ConsoleHandle);

impl<'a> Muted<'a> {
    fn new(console: &'a ConsoleHandle) -> Self {
        console.set_muted(true);
        Self(console)
    }
}

impl Drop for Muted<'_> {
    fn drop(&mut self) {
        self.0.set_muted(false);
    }
}

/// provider 失败之后等多久再驱动一次那个回合（§9）。
///
/// 定死在这里，不做退避：无人值守的重试是为了盖过一次抖动，不是为了把一次真正的故障熬过去
/// —— 后者该做的是停下报告。两秒足够让限流窗口挪一格。
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// 越界 id 的报账（`.scratch/goal-loop/spec.md` §3）：只在集合**变了**的时候报。
///
/// 沉默会让模型以为它记下了；而每回合都刷同一行会把转录淹掉。启动时报一次，此后循环里每回合
/// 重算发现新的再报 —— 于是「运行中新冒出来的越界 id」不会一直没人说。
fn report_unknown_goal_ids(
    harness: &Harness,
    name: &str,
    unknown: &[String],
    reported: &mut Vec<String>,
) {
    if unknown.is_empty() || unknown == reported.as_slice() {
        return;
    }
    *reported = unknown.to_vec();
    harness.notice(&render::wording::goal_unknown_ids(name, unknown));
}

/// 停下并报告（`.scratch/goal-loop/spec.md` §9）：落一条收尾事件，把话说在转录里。
///
/// 报告里带 `count`（连续几次翻页零完成 / 失败了几次 / 试了几次）与卡住的条目 id —— 只说
/// 「停了」没有用，回头要核得出它是怎么卡住的。
///
/// 那份进度**由这里自己算**（这一刻的派生值），不由调用方递进来：调用点有四条（撞顶 / 无进展 /
/// provider 失败 / 人主动停），而「每一处都记得传刚重算的那份」是一条会失守的约定 —— 第一次真机
/// 跑撞顶时就失守了：报告把已经做完的 01 也列成「还卡着」。
fn stop_goal_loop(
    harness: &mut Harness,
    name: &str,
    reason: crate::events::GoalStopReason,
    count: u32,
    manifest: &crate::goals::Manifest,
    goals: &GoalSetup,
) -> Result<(), String> {
    let progress = goal_progress(goals, name, manifest);
    let stuck = crate::goals::unfinished(&manifest.entries, &progress);
    let detail = render::wording::goal_stopped(name, reason, count, &stuck);
    if let Err(error) = harness.stop_goal(name, reason, &detail, stuck, count) {
        return Err(render::wording::error_report(&error));
    }
    harness.notice(&detail);
    Ok(())
}

/// 解析 `/loop` 的参数：一个目标名字，一个不断开的词。
fn parse_loop_line(args: &str) -> Result<String, String> {
    let name = args.trim();
    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(render::wording::loop_usage().to_owned());
    }
    Ok(name.to_owned())
}

/// 循环即将驱动的那个回合由什么起头。
enum TurnStart<'a> {
    /// 用户打的 prompt：它成为一条 `user` 消息，然后回合跑起来。
    Prompt(&'a str),
    /// 光秃秃的 `/<skill>`：[`Harness::run_skill`] 加载正文，它自己就投影成一条 `user` 消息，所以
    /// 为它凭空造一条 prompt 是不对的。转录绝不能显示用户没打过的字。
    Skill(&'a str),
    /// 目标循环的一个回合：起头的是刚刚注入的那条上下文（清单、提醒或摘要）。
    Injected,
}

/// 一条退出路径该用哪个码（`.scratch/exit-gesture/spec.md` §3）。
///
/// 人主动退（空闲双击、`/quit`、输入结束）= 0；忙碌中被打断而退 = 130（与 `SIGINT` 的
/// 128 + 2 惯例一致）。空闲那条路**不许**「顺便」变成 130，所以两档只在这里定义一次。
fn exit_code_after(quit: bool) -> ExitCode {
    if quit {
        ExitCode::from(130)
    } else {
        ExitCode::SUCCESS
    }
}

/// 交互式会话的退出账本：人在一次运行中途有没有要求退出
/// （`.scratch/exit-gesture/spec.md` §3、§5）。
///
/// 忙碌里的第二下 `Ctrl-C` 不是「立刻把进程按下去」：它**记下请求**，并让当前回合像被取消
/// 一样拿到它的收尾事件 —— 等回合落地之后由循环用 [`exit_code_after`] 兑现那个码。这样
/// `TerminalModes` 的 `Drop`、`ratatui::restore()` 与退出回执都会跑；旧写法
/// ``std::process::exit`` 会跳过一切析构，把终端留在 raw + 备用屏幕里。
#[derive(Debug, Default, Clone, Copy)]
struct ExitRequest {
    quit: bool,
}

impl ExitRequest {
    /// 把一个前端手势并进这份账。
    fn apply(&mut self, event: &FrontEndEvent, signal: &CancelSignal) {
        match event {
            FrontEndEvent::Quit => {
                self.quit = true;
                signal.cancel();
            }
            FrontEndEvent::Cancel => signal.cancel(),
            // 模式循环在调用点自己处理。
            FrontEndEvent::CycleMode => {}
            // 切换由调用点自己处理（忙闲判据在那一侧）。
            FrontEndEvent::OpenPicker(_) => {}
        }
    }

    /// 有没有人要求退出。
    fn requested(&self) -> bool {
        self.quit
    }

    /// 这一趟该用的退出码。
    fn code(&self) -> ExitCode {
        exit_code_after(self.quit)
    }
}

/// 跑一个回合，同时仍然盯着取消手势。
///
/// 回合占着会话，所以循环没法自己读键盘；它改为 select 控制台那些未被请求的事件。取消手势让回合
/// 拿到它的收尾事件；`Quit`（忙碌里举手之后的那第二下）同样先取消，只把「要退出」记进
/// [`ExitRequest`] —— 进程不在这一层结束（spec §6、`.scratch/exit-gesture/spec.md` §3）。
async fn run_one_turn(
    harness: &mut Harness,
    events: &mut ConsoleEvents,
    start: TurnStart<'_>,
    quit: &mut ExitRequest,
) -> Result<crate::agent::TurnOutcome, crate::Error> {
    let signal = harness.cancel_signal();
    let modes = harness.mode_cycle();
    // 同上：回执走借用之前克隆下来的渲染句柄。
    let render = harness.render_handle();
    let mut turn = Box::pin(async move {
        match start {
            TurnStart::Prompt(input) => harness.run_turn(input).await,
            TurnStart::Skill(name) => harness.run_skill(name).await,
            TurnStart::Injected => harness.run_injected_turn().await,
        }
    });
    loop {
        tokio::select! {
            result = &mut turn => return result,
            event = events.recv() => match event {
                // 权限门每次调用都读策略，所以这次按键挪动的是「下一次调用」的立场 —— 这就是「回合
                // 中途换档」的意思。
                Some(FrontEndEvent::CycleMode) => {
                    modes.cycle();
                }
                // 运行中要切换：拒绝，并说清为什么（spec §6）。
                Some(FrontEndEvent::OpenPicker(_)) => {
                    render.notice(&render::wording::heng(render::wording::switch_busy()));
                }
                Some(event) => quit.apply(&event, &signal),
                // 输入结束让回合像被取消一样落下来，所以流仍然得到它的收尾。
                None => signal.cancel(),
            },
        }
    }
}

#[derive(Debug, Default)]
struct ProbeArgs {
    config: Option<PathBuf>,
    models: Vec<String>,
}

fn parse_probe(args: &[String]) -> Result<ProbeArgs, String> {
    let mut parsed = ProbeArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_path("--config"))?;
                parsed.config = Some(PathBuf::from(value));
            }
            "--model" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_model("--model"))?;
                parsed.models.push(value.clone());
            }
            other => return Err(render::wording::unknown_argument(other)),
        }
        index += 1;
    }
    Ok(parsed)
}

/// 加载配置：显式的 `--config` 必须存在；默认路径只在它存在时才被采用。
fn load_config(explicit: Option<PathBuf>, env: &EnvMap) -> Result<Config, String> {
    match explicit {
        Some(path) => config::load(&path, env).map_err(|error| error.to_string()),
        None => {
            let path = config::default_path(env);
            if path.exists() {
                config::load(&path, env).map_err(|error| error.to_string())
            } else {
                config::resolve(None, env).map_err(|error| error.to_string())
            }
        }
    }
}

async fn probe(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_probe());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_probe(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("heng: {message}");
            println!("{}", render::wording::help_probe());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config, env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("heng: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };

    // 没登记的模型 id 是启动错误，绝不是悄悄降级：探测任何东西之前先查整张表。
    if let Err(message) = validate_models(&config) {
        eprintln!("heng: {message}");
        return ExitCode::FAILURE;
    }

    let models: Vec<String> = if parsed.models.is_empty() {
        config
            .models_with_keys()
            .into_iter()
            .map(|model| model.id.clone())
            .collect()
    } else {
        parsed.models
    };
    if models.is_empty() {
        eprintln!("heng: {}", render::wording::probe_no_key());
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    // 库不读任何环境，所以 CLI 把它唯一需要的那件事实交给它 —— `rm` 断路器要用的那个。
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    for model in models {
        match probe_model(&config, &model, home.as_deref(), env).await {
            Ok(()) => {}
            Err(ProbeError::Skipped(message)) => eprintln!("heng: 跳过 {model}：{message}"),
            Err(ProbeError::Failed(message)) => {
                eprintln!("heng: {model}: {message}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// 每一个已配置的模型 id 都必须在任何东西跑起来之前就在能力表里，所以没登记的 id 是启动错误，而不
/// 是一个回合开始之后才发现的意外。
fn validate_models(config: &Config) -> Result<(), String> {
    for model in config.models.values() {
        caps_for(&model.id).map_err(|error| error.to_string())?;
    }
    Ok(())
}

enum ProbeError {
    Skipped(String),
    Failed(String),
}

impl ProbeError {
    fn failed(error: impl std::fmt::Display) -> Self {
        ProbeError::Failed(error.to_string())
    }
}

async fn probe_model(
    config: &Config,
    model_id: &str,
    home: Option<&Path>,
    env: &EnvMap,
) -> Result<(), ProbeError> {
    let (_, profile) = config
        .resolve_model(Some(model_id))
        .map_err(ProbeError::failed)?;
    let provider = match OpenAiProvider::build(config, model_id, stderr_warnings()) {
        Ok(provider) => provider,
        Err(BuildError::MissingKey { hint, .. }) => {
            return Err(ProbeError::Skipped(format!(
                "没有 API key（请 export {hint}）"
            )));
        }
        Err(error) => return Err(ProbeError::failed(error)),
    };

    let dir = probe_dir(model_id);
    std::fs::create_dir_all(&dir).map_err(ProbeError::failed)?;
    let log_path = dir.join("log.jsonl");
    let _ = std::fs::remove_file(&log_path);

    // 会话需要的每一个已配置的值 —— 生成参数、token 配额、价目表、路由覆盖 —— 都经同一个函数到达
    // （spec §17），所以这个探针与以后任何前端组装出来的东西都一样。
    let session_config = config
        .session_config(model_id)
        .map_err(ProbeError::failed)?;
    let mcp = mcp_service(config, &dir, env, home, None).await;
    let mut harness = assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: dir,
            log_path: log_path.clone(),
            session_id: SessionId::new(format!("probe-{model_id}")),
            // 探针要跑真实回合，所以给它真实的工具表 —— 除了 `ask_user_question`：无头没有应答者，
            // 而一个只能失败的工具会白白浪费一次模型调用（spec §19）。
            tools: tools::with_mcp(
                tools::with_web(
                    tools::with_dynamic(&config.tools, false),
                    web_service(config),
                ),
                mcp,
            ),
            locks: PathLocks::new(),
            // 探针是无头的、没有应答者，所以配置那一档的 `ask`（默认）会拒掉写，而不是挂在一个谁也
            // 看不见的问题上。
            policy: Policy::for_mode(config.mode).with_outside_read(config.outside_read),
            asker: None,
            questions: None,
            // 票 05 落地挂载点；把用户声明的钩子接进 CLI 还没有归属的票，所以探针不带钩子跑。
            hook: None,
            home: home.map(Path::to_path_buf),
        },
        provider: Box::new(provider),
        speaker: SpeakerId::Debater(profile.name.clone().into()),
        config: session_config,
        renderer: Renderer::headless(RenderSinks {
            // 探针把自己的报告打在 stdout 上；渲染器只往 stderr 叙述。
            stdout_result: Box::new(std::io::sink()),
            stderr_diagnostic: Box::new(std::io::stderr()),
        }),
    })
    .await
    .map_err(ProbeError::failed)?;

    println!(
        "模型 {model_id}（provider {}，{}）",
        profile.name, profile.base_url
    );
    let first = probe_prompt();
    let turns: [&str; 2] = [first.as_str(), PROBE_FOLLOW_UP];
    for (turn, prompt) in turns.iter().enumerate() {
        if turn > 0 {
            // 在再问一次之前，先让厂商把第一个回合的前缀持久化下来。
            tokio::time::sleep(CACHE_WARMUP).await;
        }
        harness.run_turn(prompt).await.map_err(ProbeError::failed)?;
        // 那个回合刚写下的流是这份报告唯一的来源。读不了它时，这次运行不会被悄悄报成免费；它会说
        // 出来。
        let events = match read_events(&log_path) {
            Ok(events) => events,
            Err(error) => {
                println!("  第 {} 回合：读不了会话流：{error}", turn + 1);
                continue;
            }
        };
        match last_usage(&events) {
            Some(usage) => println!(
                "  第 {} 回合：input={} output={} cached={} miss={}",
                turn + 1,
                usage.input_tokens,
                usage.output_tokens,
                usage.cached_tokens,
                usage.miss_tokens,
            ),
            None => println!("  第 {} 回合：没有记下用量", turn + 1),
        }
        // 钱只作显示（spec §17）：模型有价格时探针显示这场会话花了多少，没有时说清缺的是哪张表。
        // `sessions stats`（票 17）才是它长期该待的地方；探针是今天真有人读的那份报告。
        let spent = total_usage(&events);
        match config.pricing.cost(model_id, spent) {
            Some(cost) => println!("  会话：{} token，${cost:.6}", spent.total_tokens()),
            None => println!(
                "  会话：{} token（没有 [pricing.{model_id}] 条目，所以不算钱）",
                spent.total_tokens()
            ),
        }
    }
    harness.shutdown().await;
    Ok(())
}

fn last_usage(events: &[Event]) -> Option<Usage> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventPayload::UsageRecorded { usage } => Some(*usage),
        _ => None,
    })
}

/// 这个会话的联网服务：`[web] enabled` 决定那两个工具在不在表里，后端在这里解析并挂上。
///
/// 组装期建一次 —— 工具表建完不再变化，所以这不是一个运行期开关。**解析不出后端（名字不认识）
/// 或解析不出凭据时，工具仍在表里**：调用给出的是结构化错误，而不是让表随凭据状态抖动
/// （`.scratch/web-search-tool/spec.md` §3）。
fn web_service(config: &Config) -> WebService {
    let settings = config.web.clone();
    if !settings.enabled {
        return WebService::new(settings);
    }
    // 抓取的后端是自己的 HTTP：没有服务器工具可用（DeepSeek 的兼容表里没有
    // `web_fetch_tool_result`），所以那一层的 SSRF 防护就是全部的防线。
    let service = WebService::new(settings.clone()).with_fetch(Arc::new(
        HttpFetch::new(
            settings.fetch_max_chars,
            std::time::Duration::from_millis(settings.fetch_timeout_ms),
        )
        .with_trust_proxy_dns(settings.trust_proxy_dns),
    ));
    match settings.search_provider.as_str() {
        DEEPSEEK_SEARCH_PROVIDER => {
            // 零新密钥：搜索复用会话这一家已经配好的那一把。
            let key = config
                .provider(DEEPSEEK_SEARCH_PROVIDER)
                .and_then(|provider| provider.api_key.clone());
            service.with_search(Arc::new(DeepSeekSearch::new(
                &settings.search_base_url,
                key,
            )))
        }
        // 不认识的名字：不挂搜索后端，调用给出 `WEB_PROVIDER_UNAVAILABLE`，消息里点名配的是
        // 哪一家。
        _ => service,
    }
}

/// 这个会话的 MCP 服务（`.scratch/mcp-support/spec.md` §1、§3、§5）。
///
/// `[mcp] enabled`（缺省关）决定四个元工具在不在表里。项目级的 `.mcp.json` 已经由
/// [`Config::load_project_mcp`] 在 cwd 定下来之后逐台并进 `config.mcp`，所以这里读到的就是
/// 最终那一份。
///
/// 打开时在组装期把每台 server **并发**连起来，失败的跳过：它的元工具调用给出的是一条结构化
/// 错误，而其余几台照常工作（spec §3）。
async fn mcp_service(
    config: &Config,
    cwd: &Path,
    env: &EnvMap,
    home: Option<&Path>,
    questions: Option<Arc<dyn UserQuestions>>,
) -> McpService {
    let sandbox = mcp_sandbox(config, cwd);
    // `env` 是整份环境快照；连接层只从里面挑白名单那三个键（`PATH` / `HOME` / `LANG`）——
    // 「只给 server 该拿的」这件事在连接层强制，前端不负责挑。
    let options = mcp::ConnectOptions::new(cwd, &sandbox, env)
        .with_home(home)
        // server 的输入请求走**同一个**问询端口（票 15）：两条路不会漂成两套实现。
        .with_questions(questions)
        .with_stderr(Arc::new(|line: &str| {
            eprintln!("heng: {}", render::wording::mcp_server_stderr(line));
        }));
    mcp::connect_all(&config.mcp, &options).await
}

/// 建 MCP 连接时要用的沙箱。
///
/// 与 `Harness::session` 用的是同一份 [`config::SandboxSettings`]，只是这里要**自己探测一次**：
/// MCP 的连接在组装之前就建好了，那时 harness 还不存在。`mode = "off"` 时不探测也没关系。
fn mcp_sandbox(config: &Config, cwd: &Path) -> Sandbox {
    let mut settings = config.sandbox.clone();
    // 走组装期那同一个入口（`resolve_availability` 自己看 `needs_probe()`），不在这一处另写
    // 一遍「该不该探」的判断。
    settings.availability = tools::sandbox::resolve_availability(&settings, cwd);
    Sandbox::new(&settings)
}

fn probe_dir(model_id: &str) -> PathBuf {
    let slug: String = model_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    std::env::temp_dir().join("heng-probe").join(slug)
}

#[derive(Debug)]
struct PruneArgs {
    /// 这个桶里要留多少场最近的会话。
    keep: usize,
    dry_run: bool,
    /// 要 prune 哪个工作区的桶；默认是当前目录。
    cwd: Option<PathBuf>,
}

fn parse_prune(args: &[String]) -> Result<PruneArgs, String> {
    let mut parsed = PruneArgs {
        // 留下 `--continue` 会接着跑的那场会话，删掉其余的。
        keep: 1,
        dry_run: false,
        cwd: None,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--keep" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value("--keep"))?;
                parsed.keep = value
                    .parse()
                    .map_err(|_| render::wording::needs_number("--keep", value))?;
            }
            "--dry-run" => parsed.dry_run = true,
            "--cwd" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_path("--cwd"))?;
                parsed.cwd = Some(PathBuf::from(value));
            }
            other => return Err(render::wording::unknown_argument(other)),
        }
        index += 1;
    }
    Ok(parsed)
}

/// `prune` 手动删掉会话目录（spec §11：什么都不自动 prune）。
///
/// 它只作用在一个桶上 —— 这个工作区的桶 —— 因为那是 store 所绑定的单位，而它留下最新的 `--keep`
/// 场会话（默认一场，即 `--continue` 会接着跑的那场）。stdout 承载结果，诊断走 stderr。
fn prune(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_prune());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_prune(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("heng: {message}");
            println!("{}", render::wording::help_prune());
            return ExitCode::FAILURE;
        }
    };
    let cwd = match parsed.cwd {
        Some(path) => path,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("heng: {}", render::wording::startup_cwd(&error.to_string()));
                return ExitCode::FAILURE;
            }
        },
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!("heng: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);

    if parsed.dry_run {
        return match store.list(&cwd) {
            Ok(sessions) => {
                for session in sessions.into_iter().skip(parsed.keep) {
                    println!(
                        "{}",
                        render::wording::prune_would_remove(
                            session.id.as_str(),
                            &session.dir.display().to_string()
                        )
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!(
                    "heng: {}",
                    render::wording::startup_store_read(&error.to_string())
                );
                ExitCode::FAILURE
            }
        };
    }

    match store.prune(&cwd, parsed.keep) {
        Ok(removed) => {
            for session in removed {
                println!(
                    "{}",
                    render::wording::prune_removed(
                        session.id.as_str(),
                        &session.dir.display().to_string()
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "heng: {}",
                render::wording::startup_store_prune(&error.to_string())
            );
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// `sessions`：可观测性 CLI（spec §18）
// ---------------------------------------------------------------------------

/// 一次已解析的 `sessions` 调用。由动词决定读哪些字段。
#[derive(Debug, Default)]
struct SessionsArgs {
    verb: String,
    id: Option<String>,
    all: bool,
    files: bool,
    only_error: bool,
    json: bool,
    limit: Option<usize>,
    round: Option<u32>,
    speaker: Option<String>,
    kind: Option<String>,
    tool: Option<String>,
    model: Option<String>,
    config: Option<PathBuf>,
    cwd: Option<PathBuf>,
}

fn parse_sessions(args: &[String]) -> Result<SessionsArgs, String> {
    let mut parsed = SessionsArgs::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--all" => parsed.all = true,
            "--files" => parsed.files = true,
            "--only-error" => parsed.only_error = true,
            "--json" => parsed.json = true,
            "--round" | "--speaker" | "--kind" | "--tool" | "--model" | "--config" | "--cwd"
            | "--limit" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value(arg))?;
                match arg {
                    "--round" => {
                        parsed.round = Some(
                            value
                                .parse()
                                .map_err(|_| render::wording::needs_number("--round", value))?,
                        )
                    }
                    "--speaker" => parsed.speaker = Some(value.clone()),
                    "--kind" => parsed.kind = Some(value.clone()),
                    "--tool" => parsed.tool = Some(value.clone()),
                    "--model" => parsed.model = Some(value.clone()),
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    "--limit" => {
                        parsed.limit = Some(
                            value
                                .parse()
                                .map_err(|_| render::wording::needs_number("--limit", value))?,
                        )
                    }
                    _ => unreachable!(),
                }
            }
            other if other.starts_with('-') => {
                return Err(render::wording::unknown_argument(other));
            }
            other if parsed.verb.is_empty() => parsed.verb = other.to_owned(),
            other if parsed.id.is_none() => parsed.id = Some(other.to_owned()),
            other => return Err(render::wording::extra_argument(other)),
        }
        index += 1;
    }
    Ok(parsed)
}

/// `sessions` 子命令一族：`ls`、`show`、`replay`、`stats`。
///
/// 不管哪个动词，stdout 都承载结果、stderr 承载诊断；每个视图都有 `--json` 形式，好让管道能读它。
/// 没有索引：找一场会话靠扫它的桶，而 `--continue` 的「最新在前」就是 `ls` 显示的顺序。
pub fn run_sessions(
    args: &[String],
    env: &EnvMap,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_sessions_help(out);
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_sessions(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = writeln!(err, "heng: {message}");
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
    };

    let result = match parsed.verb.as_str() {
        "ls" => sessions_ls(&parsed, env, out, err),
        "show" => sessions_show(&parsed, env, out),
        "replay" => sessions_replay(&parsed, env, out),
        "stats" => sessions_stats(&parsed, env, out),
        "" => {
            let _ = writeln!(err, "heng: {}", render::wording::sessions_needs_verb());
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
        other => {
            let _ = writeln!(
                err,
                "heng: {}",
                render::wording::unknown_sessions_verb(other)
            );
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(err, "heng: {message}");
            ExitCode::FAILURE
        }
    }
}

/// 会话 store，或者「没有可看的地方」这句拒绝。
fn open_store(env: &EnvMap) -> Result<SessionStore, String> {
    let root = config::sessions_dir(env)
        .ok_or_else(|| render::wording::startup_no_session_store().to_owned())?;
    Ok(SessionStore::new(root))
}

/// 先搜哪个工作区的桶：`--cwd`，否则当前目录。
fn search_cwd(parsed: &SessionsArgs) -> Result<PathBuf, String> {
    match &parsed.cwd {
        Some(path) => Ok(path.clone()),
        None => std::env::current_dir()
            .map_err(|error| render::wording::startup_cwd(&error.to_string())),
    }
}

/// 这一趟打开的那场会话，以及它落在哪里。
#[derive(Debug)]
struct SessionChoice {
    stored: StoredSession,
    /// 这一趟的工作目录。
    cwd: PathBuf,
    /// 那场会话在**别的工作区**里（工作目录因此跟着它走）—— 前端要说一句。
    elsewhere: bool,
}

/// 这一趟打开哪场会话，以及它落在哪个工作目录里。
///
/// `-c` 的三种含义都在这里：不带 `resume` 是新开一场；`Latest` 是这个工作区最近写下的那场；
/// `Session(id)` 是**指名**续哪一场 —— 先在本桶找，找不到就全 store 找（id 全局唯一，
/// `sessions show <id>` 一直是这么做的）。
///
/// 按 id 续上别处的那场时，工作目录取**那场会话自己的**（它流里 `SessionStarted` 记的那个），
/// 而不是命令行上那个：续写要发生在它自己的目录里，否则工具的相对路径、前缀缓存与横幅全不对。
/// 读不出 cwd（流坏了）就退回请求的那个。
fn choose_session(
    store: &SessionStore,
    cwd: &Path,
    resume: Option<&Resume>,
) -> Result<SessionChoice, String> {
    let fresh = |stored: StoredSession| SessionChoice {
        stored,
        cwd: cwd.to_path_buf(),
        elsewhere: false,
    };
    match resume {
        None => match store.create(cwd) {
            Ok(stored) => Ok(fresh(stored)),
            Err(error) => Err(render::wording::startup_store_create(&error.to_string())),
        },
        Some(Resume::Latest) => match store.latest(cwd) {
            Ok(Some(stored)) => Ok(fresh(stored)),
            Ok(None) => Err(render::wording::startup_no_session_to_continue(
                &cwd.display().to_string(),
            )),
            Err(error) => Err(render::wording::startup_store_read(&error.to_string())),
        },
        Some(Resume::Session(id)) => {
            let stored = find_session(store, cwd, id)?;
            // 它在不在**请求的那个工作区**里：在就沿用请求的目录。读不出桶就当它在本桶 —— 保守：
            // 宁可少换一次目录，也不凭一次读失败去改这一趟的工作目录。
            let here = store.is_in_bucket(cwd, &stored.id).unwrap_or(true);
            // 续写要发生在它自己的目录里。读不出它自己的 cwd（流坏了）就老实退回请求的那个，而且
            // **不说**那句「跟着走了」—— 那时它说的会是假话。
            let own = if here {
                None
            } else {
                crate::session::store::session_cwd(&stored)
            };
            Ok(SessionChoice {
                cwd: own
                    .as_deref()
                    .map_or_else(|| cwd.to_path_buf(), PathBuf::from),
                elsewhere: own.is_some(),
                stored,
            })
        }
    }
}

/// 按 id（或按它目录的路径）找一场会话。
fn find_session(store: &SessionStore, cwd: &Path, id: &str) -> Result<StoredSession, String> {
    let direct = PathBuf::from(id);
    if direct.join(crate::session::store::LOG_FILE).is_file() {
        return Ok(session_at(direct));
    }
    // id 是全局唯一的，但拥有它的那个桶才是快路径；全 store 扫一遍是让 `sessions show` 在任何目录
    // 下都能用的东西。
    let mut candidates = store.list(cwd).unwrap_or_default();
    candidates.extend(store.list_all().unwrap_or_default());
    candidates
        .into_iter()
        .find(|session| session.id.as_str() == id)
        .ok_or_else(|| render::wording::no_session(id, &store_list_label(cwd)))
}

fn store_list_label(cwd: &Path) -> String {
    render::wording::session_search_label(&cwd.display().to_string())
}

fn session_at(dir: PathBuf) -> StoredSession {
    let id = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    StoredSession {
        id: SessionId::new(id),
        outputs_dir: dir.join(crate::session::store::OUTPUTS_DIR),
        log_path: dir.join(crate::session::store::LOG_FILE),
        dir,
    }
}

/// 解析 `--speaker`：`user`、`system`、`executor:<id>`，或某位讨论者的名字。
fn parse_speaker(raw: &str) -> SpeakerId {
    match raw {
        "user" => SpeakerId::User,
        "system" => SpeakerId::System,
        other => match other.strip_prefix("executor:") {
            Some(id) => SpeakerId::Executor(id.into()),
            None => SpeakerId::Debater(other.into()),
        },
    }
}

fn sessions_ls(
    parsed: &SessionsArgs,
    env: &EnvMap,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let listings = observe::list(&store, (!parsed.all).then_some(cwd.as_path()))
        .map_err(|error| render::wording::startup_store_read(&error.to_string()))?;
    let mut listings: Vec<Listing> = listings;
    if let Some(limit) = parsed.limit {
        listings.truncate(limit);
    }

    if parsed.json {
        return write_json(out, &listings);
    }
    if listings.is_empty() {
        let _ = writeln!(err, "heng: {}", render::wording::no_sessions());
        return Ok(());
    }
    // 表头是按**显示列宽**补齐的：一个中文列名按字符数算比按列宽算要窄，所以 `{:<36}` 会让它相对
    // 它标注的那一行左移两列。
    let columns = render::wording::ls_columns();
    let _ = writeln!(
        out,
        "{}  {}  {}  {}  {}  {}  {}",
        pad_end(columns[0], 36),
        pad_end(columns[1], 28),
        pad_end(columns[2], 20),
        pad_start(columns[3], 10),
        pad_start(columns[4], 6),
        pad_start(columns[5], 5),
        columns[6],
    );
    for listing in listings {
        let _ = writeln!(
            out,
            "{:<36}  {:<28}  {:<20}  {:>10}  {:>6}  {:>5}  {}",
            listing.id,
            listing.cwd.as_deref().unwrap_or("-"),
            listing
                .started
                .map(|at| at.format("%Y-%m-%d %H:%M:%SZ").to_string())
                .unwrap_or_else(|| "-".to_owned()),
            listing.tokens,
            listing.rounds,
            listing.messages,
            listing
                .ended
                .map(render::wording::stop_reason)
                .unwrap_or_else(render::wording::open_session),
        );
    }
    Ok(())
}

/// 把 `text` 右侧补齐到 `width` 个**显示列**。
fn pad_end(text: &str, width: usize) -> String {
    let used = text.cell_width() as usize;
    if used >= width {
        return text.to_owned();
    }
    format!("{text}{}", " ".repeat(width - used))
}

/// 把 `text` 左侧补齐到 `width` 个**显示列**。
fn pad_start(text: &str, width: usize) -> String {
    let used = text.cell_width() as usize;
    if used >= width {
        return text.to_owned();
    }
    format!("{}{text}", " ".repeat(width - used))
}

fn sessions_show(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or_else(render::wording::show_needs_id)?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path).map_err(|error| {
        render::wording::cannot_read(&session.log_path.display().to_string(), &error.to_string())
    })?;

    if parsed.files {
        let changes = filter_changes(observe::file_history(&events), parsed);
        if parsed.json {
            return write_json(out, &changes);
        }
        for change in changes {
            let _ = writeln!(
                out,
                "{:<18}  {:<14}  {}",
                change
                    .round
                    .map(render::wording::round_label)
                    .unwrap_or_else(|| render::wording::session_group().to_owned()),
                change.speaker,
                change.path,
            );
        }
        return Ok(());
    }

    let filter = Filter {
        round: parsed.round,
        speaker: parsed.speaker.as_deref().map(parse_speaker),
        kind: parsed.kind.clone(),
        tool: parsed.tool.clone(),
        only_error: parsed.only_error,
    };
    let timeline = observe::timeline(&events);
    let timeline = if filter.is_empty() {
        timeline
    } else {
        timeline.filtered(&filter)
    };
    if parsed.json {
        return write_json(out, &timeline);
    }
    print_timeline(out, &timeline, &filter);
    Ok(())
}

/// `--files` 认 `--round`、`--speaker` 与 `--tool`；按行形状来的过滤器（`--kind`、`--only-error`）
/// 对一行文件改动没有意义。
fn filter_changes(
    changes: Vec<observe::FileChange>,
    parsed: &SessionsArgs,
) -> Vec<observe::FileChange> {
    let speaker = parsed.speaker.as_deref().map(parse_speaker);
    changes
        .into_iter()
        .filter(|change| {
            parsed.round.is_none_or(|round| change.round == Some(round))
                && speaker
                    .as_ref()
                    .is_none_or(|speaker| &change.speaker == speaker)
                && parsed.tool.as_ref().is_none_or(|tool| &change.tool == tool)
        })
        .collect()
}

fn sessions_replay(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or_else(render::wording::replay_needs_id)?;
    let speaker = parsed
        .speaker
        .as_deref()
        .ok_or_else(render::wording::replay_needs_speaker)?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path).map_err(|error| {
        render::wording::cannot_read(&session.log_path.display().to_string(), &error.to_string())
    })?;

    let config = load_config(parsed.config.clone(), env)?;
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let speaker = parse_speaker(speaker);
    // 能力表跟着路由走，与那次真实调用完全一样：被路由的合成器或执行者可能用另一个模型作答，而它的
    // 能力表事实不同，拿讨论者的能力表去重现那条请求会是一个无声的谎言（spec §17、§18）。
    let routing = CostModel::new(&model, config.pricing.clone()).with_routing(&config.routing);
    let caps = caps_for(routing.model_for(&speaker)).map_err(|error| error.to_string())?;

    let messages = replay::replay(&events, &speaker, parsed.round, &caps)
        .map_err(|error| error.to_string())?;
    if parsed.json {
        return write_json(out, &messages);
    }
    print_messages(out, &messages);
    Ok(())
}

fn sessions_stats(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or_else(render::wording::stats_needs_id)?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path).map_err(|error| {
        render::wording::cannot_read(&session.log_path.display().to_string(), &error.to_string())
    })?;

    // 钱需要一个模型，而流不携带模型，所以模型在这里点名：`--model`，否则配置里的默认值。给人看的
    // 视图会说它按哪个模型计了价，所以这个数字永远不会被无声地归到某个模型上。
    let config = load_config(parsed.config.clone(), env)?;
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let cost = CostModel::new(model.clone(), config.pricing.clone()).with_routing(&config.routing);
    let stats = observe::stats(&events, Some(&cost));

    if parsed.json {
        return write_json(out, &stats);
    }
    print_stats(out, &stats, &model);
    Ok(())
}

fn write_json<T: serde::Serialize>(out: &mut dyn Write, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    writeln!(out, "{text}").map_err(|error| error.to_string())
}

fn print_timeline(out: &mut dyn Write, timeline: &Timeline, filter: &Filter) {
    // 用量行是统计视图的素材；默认的转录跳过它们，除非有人点名要。
    let kind = filter.kind.as_deref().map(observe::canonical_kind);
    let show_usage = kind.as_deref() == Some("usagerecorded");
    for group in &timeline.groups {
        match group.round {
            Some(round) => {
                let _ = writeln!(out, "\n{}", render::wording::round_group(round, group.mode));
            }
            None => {
                let _ = writeln!(out, "\n{}", render::wording::session_group());
            }
        }
        for entry in &group.entries {
            if matches!(entry, Entry::RoundStarted { .. }) {
                continue;
            }
            if matches!(entry, Entry::Usage { .. }) && !show_usage {
                continue;
            }
            let _ = writeln!(out, "{}", render_entry(entry));
        }
    }
}

fn render_entry(entry: &Entry) -> String {
    let label = |speaker: &SpeakerId| render::wording::speaker_label(speaker);
    match entry {
        Entry::Message { speaker, text, .. } => format!("{} {text}", label(speaker)),
        Entry::Tool {
            speaker,
            tool,
            args,
            ok,
            output,
            error,
            hook,
            ..
        } => {
            let mut lines = vec![format!(
                "{} → {}",
                label(speaker),
                render::wording::tool_call(tool, &args.to_string())
            )];
            match (ok, output, error) {
                (Some(true), Some(output), _) => {
                    lines.push(indent(
                        &render::wording::tool_output_preview(output, PREVIEW),
                        2,
                    ));
                }
                (Some(false), _, error) => {
                    let message = error
                        .clone()
                        .unwrap_or_else(|| render::wording::no_message().to_owned());
                    lines.push(indent(&render::wording::tool_error(&message), 2));
                }
                _ => lines.push(indent(render::wording::no_tool_result(), 2)),
            }
            if let Some(hook) = hook {
                lines.push(indent(&render::wording::hook_feedback(hook), 2));
            }
            lines.join("\n")
        }
        Entry::RoundStarted { round, mode } => render::wording::round_section(*round, *mode),
        Entry::RoundEnded { round, reason } => render::wording::round_ended(*round, *reason),
        Entry::SessionEnded { reason } => render::wording::session_ended(*reason),
        Entry::TurnStarted { speaker, iteration } => {
            format!(
                "{} {}",
                label(speaker),
                render::wording::turn_started(*iteration)
            )
        }
        Entry::TurnEnded { speaker, reason } => {
            format!(
                "{} {}",
                label(speaker),
                render::wording::turn_ended(*reason)
            )
        }
        Entry::Divergence {
            topic, positions, ..
        } => {
            let mut lines = vec![format!("!! {}", render::wording::divergence(topic))];
            for position in positions {
                lines.push(indent(&format!("- {position}"), 2));
            }
            lines.join("\n")
        }
        Entry::PermissionAsked {
            speaker, request, ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::permission_asked(
                crate::events::permission_format::tool_name(request),
                &render::transcript::summarize_permission_target(request)
            )
        ),
        Entry::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
            ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::permission_decided(*decision, *source, reason.as_deref())
        ),
        Entry::Hook {
            speaker,
            point,
            outcome,
            ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::hook(point, outcome)
        ),
        Entry::ExecutorSpawned {
            speaker,
            executor_id,
            ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::executor_spawned(executor_id.as_str())
        ),
        Entry::ExecutorFinished {
            executor_id,
            reason,
            summary,
            ..
        } => render::wording::executor_finished(executor_id.as_str(), *reason, summary),
        Entry::Usage { speaker, usage } => {
            format!(
                "{} {}",
                label(speaker),
                render::wording::usage_summary(usage)
            )
        }
        Entry::AgentError {
            speaker, message, ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::agent_error(message)
        ),
        Entry::SessionError { code, detail, .. } => render::wording::session_error(code, detail),
        Entry::History {
            reason, summary, ..
        } => render::wording::history(*reason, summary.as_deref()),
        Entry::Context { source, .. } => render::wording::context_injected(source.clone()),
        Entry::Sandbox {
            mode,
            unavailable_reason,
        } => render::wording::sandbox(mode, unavailable_reason.as_deref()),
    }
}

fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `sessions show` 在略去之前打印一个工具结果的多大部分。
const PREVIEW: usize = 500;

fn print_messages(out: &mut dyn Write, messages: &[Message]) {
    for message in messages {
        match message {
            Message::System { content, .. } => {
                let _ = writeln!(out, "{}\n{content}\n", render::wording::replay_system());
            }
            Message::User { content, name, .. } => {
                let label = render::wording::replay_user(name.as_deref());
                let _ = writeln!(out, "{label}\n{content}\n");
            }
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let _ = writeln!(out, "{}", render::wording::replay_assistant());
                if let Some(content) = content {
                    let _ = writeln!(out, "{content}");
                }
                for call in tool_calls {
                    let _ = writeln!(
                        out,
                        "{}",
                        render::wording::replay_tool_call(&call.name, &call.arguments)
                    );
                }
                let _ = writeln!(out);
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let _ = writeln!(
                    out,
                    "{}\n{content}\n",
                    render::wording::replay_tool(tool_call_id)
                );
            }
        }
    }
}

fn print_stats(out: &mut dyn Write, stats: &observe::Stats, model: &str) {
    let cost = match stats.session.cost {
        Some(cost) => render::wording::stats_cost(cost, model),
        None => render::wording::stats_no_cost(model),
    };
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_session(
            stats.session.tokens.total_tokens(),
            stats.session.calls,
            stats.session.messages,
            stats.session.rounds,
            &cost,
        )
    );
    for speaker in &stats.speakers {
        let hit = speaker
            .hit_rate
            .map(|rate| format!("{:.0}%", rate * 100.0))
            .unwrap_or_else(|| "-".to_owned());
        let cost = speaker
            .cost
            .map(|cost| format!("  ${cost:.6}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_speaker(
                &pad_end(&speaker.speaker.to_string(), 16),
                speaker.tokens.total_tokens(),
                speaker.calls,
                &hit,
                &cost,
            )
        );
    }
    let rate = stats
        .absence
        .rate
        .map(|rate| format!(" ({:.0}%)", rate * 100.0))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_absence(stats.absence.one_sided, stats.absence.rounds, &rate)
    );
    for agent in &stats.absence.per_speaker {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_absent(&agent.name, agent.count)
        );
    }
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_edits(stats.edits.succeeded, stats.edits.failed_matches)
    );
    for level in &stats.edits.levels {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_match_level(&level.name, level.count)
        );
    }
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_guards(
            stats.guards.read_before_write,
            stats.guards.invalidated_reads
        )
    );
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_executors(stats.executors.spawned, stats.executors.finished)
    );
    for reason in &stats.executors.by_reason {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_executor_reason(
                render::wording::stop_reason_name(&reason.name),
                reason.count
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_hooks(
            stats.hooks.executed,
            stats.hooks.pre,
            stats.hooks.post,
            stats.hooks.feedback,
            stats.hooks.failed
        )
    );
    let decisions = stats
        .permissions
        .decided
        .iter()
        .map(|count| {
            render::wording::stats_decision(
                count.count,
                render::wording::decision_name(&count.name),
            )
        })
        .collect::<Vec<_>>()
        .join("、");
    let decided = if decisions.is_empty() {
        String::new()
    } else {
        format!("，已裁决 {decisions}")
    };
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_permissions(stats.permissions.asked, &decided)
    );
    let rate = stats
        .divergence_rate
        .map(|rate| format!(" ({:.0}%)", rate * 100.0))
        .unwrap_or_default();
    let rounds = stats
        .rounds
        .iter()
        .map(|round| {
            let ended = round
                .ended
                .map(render::wording::stats_round_ended)
                .unwrap_or_default();
            render::wording::stats_round(round.round, round.mode, round.calls, &ended)
        })
        .collect::<Vec<_>>()
        .join("; ");
    let head = format!(
        "{}{}",
        render::wording::stats_divergences(stats.divergences, stats.absence.rounds, &rate),
        render::wording::stats_rounds_prefix()
    );
    let _ = writeln!(out, "{head}{rounds}");
    for stop in &stats.stops {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_stop(render::wording::stop_reason_name(&stop.name), stop.count)
        );
    }
}

fn print_sessions_help(out: &mut dyn Write) {
    let _ = writeln!(out, "{}", render::wording::help_sessions());
}

#[cfg(test)]
mod tests {
    use super::{
        ConsoleHandle, ExitRequest, McpPromptEntry, Mode, Submission, exit_code_after,
        finish_session, retarget_to, submission, undo_receipt,
    };
    use crate::agent::{CancelSignal, UndoOutcome};
    use crate::config::{Config, ReasoningEffort};
    use crate::events::{SessionId, SpeakerId, ToolCallId};
    use crate::render::{ConsolePort, ConsoleRequest, FrontEndEvent, PickerKind, console};
    use crate::{Harness, Policy};
    use std::path::PathBuf;
    use std::process::ExitCode;

    /// 一张三家厂商都给了 key 的配置，于是 `retarget_to` 能真的组装出 provider。
    fn switchable_config() -> crate::config::Config {
        crate::config::resolve(
            None,
            &[
                ("MOONSHOT_API_KEY", "sk-kimi"),
                ("DEEPSEEK_API_KEY", "sk-deepseek"),
                ("MINIMAX_API_KEY", "sk-minimax"),
                ("MINIMAX_CN_API_KEY", "sk-minimax-cn"),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        )
        .expect("一张能跑的配置")
    }

    /// 换模型是**重建**：provider 与配置都重新组装，而累计额度（会话事实）照旧带着走。
    #[test]
    fn retargeting_rebuilds_the_provider_and_carries_the_goal_usage_over() {
        let config = switchable_config();
        let retargeted = retarget_to(&config, "MiniMax-M3.1-Flash-Preview", None, 12_345)
            .expect("MiniMax M3.1 有 key");

        assert_eq!(retargeted.profile, "minimax");
        assert_eq!(retargeted.config.model, "MiniMax-M3.1-Flash-Preview");
        assert_eq!(retargeted.config.carried_tokens, 12_345);
        // 请求参数是从这个模型的配置条目读的，不是从上一场会话抄的。
        assert_eq!(
            retargeted.config.executor_model,
            config.routing.executor_model
        );
        // 换一个厂商就换 profile，于是发言者的名字也跟着换（spec §4）。
        assert_eq!(
            retargeted.config.model, "MiniMax-M3.1-Flash-Preview",
            "模型 id 就是线上发的那一个"
        );
    }

    /// 当前档位新模型不认时退回「默认」—— 而不是让适配器在回合中途把它丢掉。
    #[test]
    fn an_effort_the_new_model_cannot_have_falls_back_to_the_default() {
        let config = switchable_config();
        // M3.1 认 xhigh。
        let kept = retarget_to(
            &config,
            "MiniMax-M3.1-Flash-Preview",
            Some(ReasoningEffort::Xhigh),
            0,
        )
        .unwrap();
        assert_eq!(
            kept.config.params.reasoning_effort,
            Some(ReasoningEffort::Xhigh)
        );

        // Kimi 只有 low/high/max，于是 xhigh 落回「默认」（不发这个参数）。
        let dropped = retarget_to(&config, "kimi-k3", Some(ReasoningEffort::Xhigh), 0).unwrap();
        assert_eq!(dropped.config.params.reasoning_effort, None);
        assert_eq!(dropped.profile, "kimi");

        // 换档只是配置动作：`high` 照旧留着。
        let high = retarget_to(&config, "kimi-k3", Some(ReasoningEffort::High), 0).unwrap();
        assert_eq!(
            high.config.params.reasoning_effort,
            Some(ReasoningEffort::High)
        );
    }

    /// 没登记的模型与缺 key 的模型都是错误，且错的那句把修法说全。
    #[test]
    fn retargeting_to_a_model_that_cannot_be_assembled_says_why() {
        let config = switchable_config();
        let unknown = retarget_to(&config, "gpt-4o", None, 0).err().unwrap();
        assert!(unknown.contains("gpt-4o"), "{unknown}");
        assert!(unknown.contains("kimi-k3"), "已知 id 要列全：{unknown}");

        // 一个指向没给 key 的 profile 的模型：那条错误必须点名那个环境变量（修法能直接抄）。
        let config = crate::config::resolve(
            Some(
                "[providers.kimi-code-cn]\nbase_url = \"https://api.kimi.com/coding/v1\"\n\
                 api_key_env = \"KIMI_CODE_CN_API_KEY\"\n\n\
                 [models.\"kimi-k3\"]\nprovider = \"kimi-code-cn\"\n",
            ),
            &Default::default(),
        )
        .unwrap();
        let message = retarget_to(&config, "kimi-k3", None, 0)
            .err()
            .expect("缺 key 就组不出 provider");
        assert!(message.contains("KIMI_CODE_CN_API_KEY"), "{message}");
    }

    /// `/model` 与 `/effort` 进 `Submission`，且**命令记号在任何位置都算命令**。
    ///
    /// 后一半是 `input-tokens` 票 04 那条纪律：一条普通消息里夹着命令记号，仍然被解析成
    /// 命令（而不是把整条当成 prompt）。
    #[test]
    fn the_switch_commands_are_parsed_and_a_token_anywhere_still_counts() {
        assert!(matches!(read("/model kimi-k3"), Submission::Model(id) if id == "kimi-k3"));
        assert!(matches!(read("/effort high"), Submission::Effort(effort) if effort == "high"));
        assert!(matches!(read("/effort 默认"), Submission::Effort(effort) if effort == "默认"));
        // 缺参数**不是**一条错误命令：它到循环那里才判（与 `parse_goal_new_line` 同形）。
        assert!(matches!(read("/model"), Submission::Model(id) if id.is_empty()));
        // 带点号的 id 不按点号切（`MiniMax-M3.1-Flash-Preview`）。
        assert!(matches!(
            read("/model MiniMax-M3.1-Flash-Preview"),
            Submission::Model(id) if id == "MiniMax-M3.1-Flash-Preview"
        ));
        // 命令记号在**任何位置**都算命令；参数照旧是「记号旁边那一整段」，与 `/goal-new`
        // 同一个形状（`input-tokens` 票 04 的纪律）。
        assert!(matches!(
            read("先看看这个 /model kimi-k3"),
            Submission::Model(id) if id == "先看看这个 kimi-k3"
        ));
        // 内建命令带着记号之外的字仍然**不是**内建命令 —— 整条退回普通消息。
        assert!(matches!(read("/model 这个"), Submission::Model(_)));
    }

    /// 模型候选：**`config.toml` 里显式配置过的那几家**名下、有密钥的模型，按键序。
    ///
    /// 两条判据都要，而**当前模型无条件在列**（一份什么都不写的配置里清单会空，而看不到自己正在用
    /// 的那个模型是最刺眼的一种不诚实）。
    #[test]
    fn the_model_candidates_are_the_configured_providers_models_and_the_current_one() {
        let config = crate::config::resolve(
            Some(
                // 只给两条 profile：一条有 key（minimax），一条没给 key（deepseek）。
                "[providers.minimax]\napi_key = \"sk-minimax\"\n\n[providers.deepseek]\n",
            ),
            &Default::default(),
        )
        .unwrap();
        // 当前模型挂在 `kimi`（内建、config.toml 里没写）上 —— 它兜底进清单。
        let options = super::model_picker_options(&config, "kimi-k3");
        let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                // 配置里有、且有密钥的那家（两个 id）。
                "MiniMax-M3",
                "MiniMax-M3.1-Flash-Preview",
                // 当前模型：内建 profile，也无条件在列（键序落在最后）。
                "kimi-k3",
                // 配置里有、但没给密钥的那家（`deepseek-flash` 与 `deepseek-v4-pro`）
                // 一个都不在 —— 切过去起不来。
            ],
            "配置里写过的 + 当前模型"
        );
        // 缺密钥的那家的模型一个都不在。
        assert!(
            !labels.contains(&"deepseek-v4-pro") && !labels.contains(&"deepseek-flash"),
            "缺 key 的不列：{labels:?}"
        );
        // 内建但没被配置的 profile（`kimi-code`）名下的模型也不在 —— 那正是注释掉
        // `[providers.kimi-code]` 的效果。
        assert!(
            !labels
                .iter()
                .any(|label| label.starts_with("k3") || label.contains("coding")),
            "没配置的那家不列：{labels:?}"
        );
        // 当前那个被标出来了，detail 说它走哪个 profile。
        let current = options
            .iter()
            .find(|option| option.current)
            .expect("当前那个被标出来了");
        assert_eq!(current.label, "kimi-k3");
        assert!(current.detail.contains("kimi"), "{:?}", current.detail);
    }

    /// 档位候选：**第一档是「默认」**，其余按强度从弱到强；表为空时给空清单（前端显示 `固定`）。
    #[test]
    fn the_effort_candidates_start_with_the_default_and_follow_the_strength_order() {
        let five =
            super::effort_picker_options("MiniMax-M3.1-Flash-Preview", Some(ReasoningEffort::High));
        assert_eq!(
            five.iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>(),
            vec!["默认", "low", "medium", "high", "xhigh", "max"]
        );
        let current = five.iter().find(|option| option.current).unwrap();
        assert_eq!(current.label, "high");

        // Kimi 只有三档。
        assert_eq!(
            super::effort_picker_options("kimi-k3", None)
                .iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>(),
            vec!["默认", "low", "high", "max"]
        );
        assert!(
            super::effort_picker_options("kimi-k3", None)[0].current,
            "None 就是「默认」那一档"
        );

        // 没有旋钮的模型：空清单。那一格显示 `固定`，而不是给一个只有「默认」可选的假清单。
        assert!(super::effort_picker_options("MiniMax-M3", None).is_empty());
        assert!(super::effort_picker_options("kimi-for-coding-highspeed", None).is_empty());
    }

    /// 一个共享的 `Vec<u8>`，拿来当 headless 渲染器的写出口。
    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("sink 已中毒").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("sink 已中毒").clone()).expect("sink 里是 UTF-8")
        }
    }

    /// 一个交互式的循环与它那条键盘：忙闲判定、切换、回执都在这一对上面验。
    async fn live_session(
        config: &Config,
    ) -> (Harness, ConsoleHandle, ConsolePort, Sink, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("临时目录");
        let cwd = dir.path().join("workspace");
        std::fs::create_dir_all(&cwd).expect("工作区");
        let stderr = Sink::default();
        let (console, port, _events) = console();
        let harness = crate::assemble(crate::AssemblyParts {
            provider: Box::new(
                crate::provider::openai::OpenAiProvider::build(
                    config,
                    "kimi-k3",
                    crate::provider::openai::silent_warnings(),
                )
                .expect("kimi-k3 有 key"),
            ),
            speaker: SpeakerId::Debater("kimi".into()),
            config: config.session_config("kimi-k3").expect("kimi-k3 在配置里"),
            renderer: crate::render::Renderer::headless(crate::render::RenderSinks {
                stdout_result: Box::new(std::io::sink()),
                stderr_diagnostic: Box::new(stderr.clone()),
            }),
            scaffold: crate::SessionScaffold {
                cwd,
                log_path: dir.path().join("log.jsonl"),
                session_id: SessionId::new("s-1"),
                tools: crate::tools::builtin(false),
                locks: crate::tools::PathLocks::new(),
                policy: Policy::for_mode(Mode::Ask),
                // 没有交互式应答者，于是权限门降级为「不动手」：这些测试不写文件。
                asker: None,
                questions: None,
                hook: None,
                home: None,
            },
        })
        .await
        .expect("组装得起来");
        (harness, console, port, stderr, dir)
    }

    /// 运行中切换被**拒绝**，而拒绝要说清为什么（spec §6）。
    #[tokio::test]
    async fn a_switch_while_something_runs_is_refused_with_a_reason() {
        let config = switchable_config();
        let (mut harness, console, mut port, stderr, _dir) = live_session(&config).await;

        super::open_picker(&mut harness, &console, &config, PickerKind::Model, true).await;
        // 没有清单发出去，而会话一点没变 —— 被拒绝的切换不重建 provider，也不推新事实。
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), port.recv())
                .await
                .is_err(),
            "运行中不发清单"
        );
        assert_eq!(harness.model(), "kimi-k3");
        // 渲染器是另一个 task：把它收掉，每一条渲染事件才都落到那两个写出口上。
        harness.shutdown().await;
        assert!(
            stderr
                .text()
                .contains(super::render::wording::switch_busy()),
            "拒绝给了那一句：{:?}",
            stderr.text()
        );
    }

    /// 空闲时那一份清单真的发到端口上了，而且人取消了之后什么都不会变。
    #[tokio::test]
    async fn an_idle_switch_asks_the_front_end_and_a_cancellation_changes_nothing() {
        let config = switchable_config();
        let (mut harness, console, mut port, stderr, _dir) = live_session(&config).await;

        let front_end = tokio::spawn(async move {
            match port.recv().await {
                Some(ConsoleRequest::Picker(request)) => {
                    let title = request.title.clone();
                    let count = request.options.len();
                    let _ = request.reply.send(None);
                    (title, count)
                }
                other => panic!("期望一份清单，实际 {other:?}"),
            }
        });
        super::open_picker(&mut harness, &console, &config, PickerKind::Model, false).await;
        let (title, count) = front_end.await.expect("前端回来了");

        assert_eq!(title, super::render::wording::picker_title_model());
        assert_eq!(
            count,
            // `switchable_config()` 只给环境变量、config.toml 是空的 —— 于是候选只有**当前模型**
            // 兜底那一个（「配置里写过的」那一条在它下面没有可选项）。
            1,
            "没有显式配置时清单里只有当前模型"
        );
        assert_eq!(harness.model(), "kimi-k3", "取消不是一次切换");
        harness.shutdown().await;
        assert!(
            !stderr.text().contains("前缀缓存"),
            "取消不给回执：{:?}",
            stderr.text()
        );
    }

    /// 这些测试里一场会话知道的技能。
    fn has_skill(name: &str) -> bool {
        matches!(name, "ask-matt" | "review")
    }

    /// 这一轮 `/` 菜单里的 MCP 模板条目。
    fn prompts() -> Vec<McpPromptEntry> {
        vec![McpPromptEntry::new(
            "db".to_owned(),
            crate::mcp::PromptSummary {
                name: "user_report".to_owned(),
                description: Some("按 id 出一份报告".to_owned()),
                arguments: vec![],
            },
        )]
    }

    fn read(text: &str) -> Submission<'_> {
        let prompts = prompts();
        submission(text, has_skill, &prompts)
    }

    /// 一次 `/undo` 的三种结果各说一句话，成功那次也**必须**说 —— 回滚在屏幕上本来是无声的
    /// （`.scratch/fs-agent-v1/issues/35-undo-receipt.md`）。
    #[test]
    fn an_undo_always_answers_with_one_line() {
        let undone = undo_receipt(Ok(Some(UndoOutcome {
            tool_call_id: ToolCallId::new("call-1"),
            path: PathBuf::from("src/a.rs"),
        })));
        assert_eq!(undone, "heng: 已撤销对 src/a.rs 的这次编辑");
        assert_eq!(undo_receipt(Ok(None)), "heng: 没有可撤销的修改");
        assert_eq!(
            undo_receipt(Err(crate::Error::Undo("快照对不上".to_owned()))),
            "heng: 撤销失败：快照对不上"
        );
    }

    #[test]
    fn required_parameters_must_be_filled() {
        let prompt = crate::mcp::PromptSummary {
            name: "user_report".to_owned(),
            description: None,
            arguments: vec![
                crate::mcp::PromptArgument {
                    name: "id".to_owned(),
                    description: None,
                    required: true,
                },
                crate::mcp::PromptArgument {
                    name: "format".to_owned(),
                    description: None,
                    required: false,
                },
            ],
        };
        let empty = serde_json::Map::new();
        assert_eq!(
            super::missing_required_arguments(&prompt, &empty),
            vec!["id"]
        );

        let mut filled = serde_json::Map::new();
        filled.insert("id".to_owned(), serde_json::Value::String("42".to_owned()));
        assert!(
            super::missing_required_arguments(&prompt, &filled).is_empty(),
            "必填填上了就不该再报"
        );
    }

    #[test]
    fn the_menu_shows_the_builtin_commands_skills_and_mcp_prompts() {
        let prompts = vec![McpPromptEntry::new(
            "db".to_owned(),
            crate::mcp::PromptSummary {
                name: "user_report".to_owned(),
                description: Some("按 id 出一份报告".to_owned()),
                arguments: vec![],
            },
        )];
        let entries = slash_catalog(
            &crate::render::wording::BUILT_IN_COMMANDS,
            &[("review", "审一遍")],
            &prompts,
        );
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"undo"), "常量条目照旧：{names:?}");
        assert!(names.contains(&"review"), "技能照旧：{names:?}");
        assert!(
            names.contains(&"db:user_report"),
            "运行时那一半也在菜单里：{names:?}"
        );
        let entry = entries
            .iter()
            .find(|entry| entry.name == "db:user_report")
            .unwrap();
        assert_eq!(entry.description, "按 id 出一份报告");
    }

    #[test]
    fn inline_arguments_fill_the_declared_parameters_in_order() {
        let prompt = crate::mcp::PromptSummary {
            name: "user_report".to_owned(),
            description: None,
            arguments: vec![
                crate::mcp::PromptArgument {
                    name: "id".to_owned(),
                    description: None,
                    required: true,
                },
                crate::mcp::PromptArgument {
                    name: "format".to_owned(),
                    description: None,
                    required: false,
                },
            ],
        };
        let filled = super::prompt_inline_arguments("db:user_report", &prompt, "42 json").unwrap();
        assert_eq!(filled["id"], "42");
        assert_eq!(filled["format"], "json");

        let too_many = super::prompt_inline_arguments("db:user_report", &prompt, "42 json extra");
        assert!(too_many.is_err(), "多出来的参数该被拒");
    }

    #[test]
    fn a_prompt_entry_carries_its_inline_arguments() {
        let parsed = read("/db:user_report 42");
        match parsed {
            Submission::McpPrompt {
                server,
                prompt,
                inline,
            } => {
                assert_eq!(server, "db");
                assert_eq!(prompt, "user_report");
                assert_eq!(inline, "42");
            }
            other => panic!("该解析成模板调用，实际是 {other:?}"),
        }
    }

    #[test]
    fn a_colon_name_that_is_not_in_the_menu_is_an_unknown_command() {
        // 菜单里没有这个名字时，它与任何打错的 `/` 名字同类 —— 不会被当成模板调用。
        assert!(matches!(read("/db:nope 42"), Submission::Unknown(_)));
        assert!(matches!(
            read("/db:user_report 42"),
            Submission::McpPrompt { .. }
        ));
    }

    #[test]
    fn the_exit_code_is_zero_unless_someone_asked_to_quit() {
        // `.scratch/exit-gesture/spec.md` §3：人主动退 = 0；忙碌里被打断而退 = 130
        // （与 `SIGINT` 的 128 + 2 惯例一致）。空闲那条路不许「顺便」变成 130。
        assert_eq!(exit_code_after(false), ExitCode::SUCCESS);
        assert_eq!(exit_code_after(true), ExitCode::from(130));
    }

    #[test]
    fn the_receipt_names_the_session_and_never_changes_the_exit_code() {
        // `.scratch/exit-gesture/spec.md` §5：交互式会话（TUI 与 `--plain`）退出时在终端
        // 交还之后打一行能直接粘的复盘命令。writer 由调用方传（生产那边是 stderr），所以
        // 「打在哪」由调用点保证，这里钉的是内容与「绝不改退出码」。
        for code in [ExitCode::SUCCESS, ExitCode::from(130)] {
            let mut out: Vec<u8> = Vec::new();
            let returned = finish_session(code, "01J8ZQ4K7M", &mut out);
            let text = String::from_utf8(out).expect("回执是 utf-8");
            assert_eq!(text, "heng: 会话 01J8ZQ4K7M；接着跑：heng -c 01J8ZQ4K7M\n");
            assert_eq!(returned, code, "一行回执不该改退出码");
        }
    }

    #[test]
    fn a_quit_gesture_is_recorded_and_cancels_instead_of_killing_the_process() {
        // 忙碌里的第二下（举手之后的 `Quit`）不是「立刻把进程按下去」：它记下退出请求，
        // 并让当前回合像被取消一样拿到它的收尾事件。旧写法 ``std::process::exit``
        // 会跳过一切析构，把终端留在 raw + 备用屏幕里（§3）。
        let signal = CancelSignal::new();
        let mut quit = ExitRequest::default();
        assert!(!quit.requested());
        assert_eq!(quit.code(), ExitCode::SUCCESS);

        quit.apply(&FrontEndEvent::Quit, &signal);
        assert!(quit.requested(), "退出请求被记下来了");
        assert!(signal.is_cancelled(), "而回合拿到了它的收尾");
        assert_eq!(quit.code(), ExitCode::from(130));

        // 普通的取消手势只取消：取消不等于退出。
        let signal = CancelSignal::new();
        let mut quit = ExitRequest::default();
        quit.apply(&FrontEndEvent::Cancel, &signal);
        assert!(signal.is_cancelled());
        assert!(!quit.requested(), "Esc / 第一下 Ctrl-C 不是退出");
        assert_eq!(quit.code(), ExitCode::SUCCESS);
    }

    #[test]
    fn a_blank_submission_is_ignored_rather_than_sent_or_read_as_the_end_of_input() {
        // 在空输入上按回车产生什么。以前是循环自己决定这件事的，所以 `cargo test` 里没有任何东西把
        // 它钉住 —— 而 TUI 曾经把空草稿当作输入结束的哨兵发出去，结果退掉了整场会话。
        assert!(matches!(read(""), Submission::Ignore));
        assert!(matches!(read("   "), Submission::Ignore));
        assert!(matches!(read("\n\n\t\n"), Submission::Ignore));
        // 它也不是一条 prompt：空消息绝不能到达模型。
        assert!(!matches!(read("  \n  "), Submission::Prompt(_)));
    }

    #[test]
    fn a_single_line_still_reads_exactly_as_it_did() {
        assert_eq!(read("/quit"), Submission::Quit);
        assert_eq!(read("/exit"), Submission::Quit);
        assert_eq!(read("/undo"), Submission::Undo);
        assert_eq!(read("  /quit  "), Submission::Quit, "与从前一样去掉空白");
        assert_eq!(read("/nope"), Submission::Unknown("/nope"));
        assert_eq!(
            read("/ask-matt 帮我看一下"),
            Submission::Skill {
                name: "ask-matt",
                task: "/ask-matt 帮我看一下".to_owned(),
                bare: false,
            },
            "技能名留在发给模型的文本里（ui-trim §2）"
        );
        assert_eq!(
            read("/ask-matt"),
            Submission::Skill {
                name: "ask-matt",
                task: "/ask-matt".to_owned(),
                bare: true,
            },
            "光秃秃的技能名只加载正文"
        );
        assert_eq!(read("hello"), Submission::Prompt("hello"));
    }

    #[test]
    fn a_multi_line_brief_follows_the_skill_named_on_the_first_line() {
        assert_eq!(
            read("/ask-matt\n第一行\n第二行"),
            Submission::Skill {
                name: "ask-matt",
                task: "/ask-matt\n第一行\n第二行".to_owned(),
                bare: false,
            }
        );
        // 任务可以从技能那一行开始，再往下面续。
        assert_eq!(
            read("/ask-matt 第一行\n第二行"),
            Submission::Skill {
                name: "ask-matt",
                task: "/ask-matt 第一行\n第二行".to_owned(),
                bare: false,
            }
        );
    }

    #[test]
    fn a_skill_keeps_the_words_around_its_name() {
        // 技能名**前面**的字也不许丢，技能名本身也不许丢：整条原文就是发出去的那条消息
        // （`.scratch/ui-trim/spec.md` §2）。内建命令那套「参数取记号旁边的字」不适用于它 ——
        // 技能的名字本身就是这句话的一部分。
        assert_eq!(
            read("请用 /ask-matt 帮我看一下"),
            Submission::Skill {
                name: "ask-matt",
                task: "请用 /ask-matt 帮我看一下".to_owned(),
                bare: false,
            }
        );
    }

    #[test]
    fn a_pasted_paragraph_that_opens_with_a_slash_is_a_prompt() {
        // 后面带行的东西都按它是一整段来读，所以粘贴一个路径或一段代码不会换来一句它从未想说的
        // 「未知命令」。
        assert_eq!(
            read("/usr/bin/env cargo test\n第二行"),
            Submission::Prompt("/usr/bin/env cargo test\n第二行")
        );
        // 只占一行时它仍然是个错字，而且是会告诉用户的那一种。
        assert_eq!(read("/usr/bin/env"), Submission::Unknown("/usr/bin/env"));
        // 它后面的空行仍然算「后面什么都没有」。
        assert_eq!(read("/nope\n   "), Submission::Unknown("/nope"));
    }

    #[test]
    fn a_built_in_takes_no_task_so_a_line_after_it_is_not_dropped() {
        // `/undo` 后面跟着别的东西时就不是这条命令了：后面的行绝不能消失，所以整条提交按「以 `/` 开
        // 头的行」那一套规则读 —— `/undo` 没点名任何技能，所以它是一条 prompt。
        assert_eq!(
            read("/undo\n把 X 改成 Y"),
            Submission::Prompt("/undo\n把 X 改成 Y")
        );
        assert_eq!(read("/undo"), Submission::Undo);
        assert_eq!(read("/undo  "), Submission::Undo, "结尾的空白没关系");
    }

    #[test]
    fn a_doubled_slash_is_not_a_built_in_command() {
        // 内建命令要求记号的原文就是 `/<名字>`，所以 `//undo` 不再是 `Undo` —— 旧路径把它当
        // 错字，而 `Undo` 有回滚副作用，不该由多出来的一个斜杠换来。技能那一侧不受影响：
        // 名字本来就是剥掉斜杠之后才比的。
        assert_eq!(read("//undo"), Submission::Prompt("//undo"));
        assert_eq!(read("//clear"), Submission::Prompt("//clear"));
        assert_eq!(
            read("//review"),
            Submission::Skill {
                name: "review",
                task: "//review".to_owned(),
                bare: true,
            },
            "多出来那个斜杠不把技能挡在裸调用之外"
        );
    }

    #[test]
    fn a_command_can_sit_anywhere_and_earlier_words_join_the_task() {
        // `.scratch/input-tokens/spec.md` §5：整个草稿里任意行、任意位置的 `/name` 都算命令，
        // 取**最靠左**的那一个，而它前面的字（含前面的每一行）并入任务 —— 这一条正是为了不
        // 重蹈「丢用户写下的字」。
        assert_eq!(
            read("请 /loop 我的目标"),
            Submission::Loop("请 我的目标".to_owned())
        );
        assert_eq!(
            read("第一行\n/loop 目标"),
            Submission::Loop("第一行\n目标".to_owned()),
            "位置不看行"
        );
        // 这一档是**有牙**的（访谈里明码标价选下的）：`@` 被误认没有后果，`/` 被误认会真的
        // 执行。如实钉住这个代价，别把它当缺陷改掉。
        assert_eq!(
            read("用 /loop 做目标循环"),
            Submission::Loop("用 做目标循环".to_owned())
        );
        // 记号在末尾时，前文就是全部任务。
        assert_eq!(read("请 /loop"), Submission::Loop("请".to_owned()));
        // 一个句子里只有**命令**记号算数，别的记号只是普通文字。
        assert_eq!(
            read("看 /tmp/x，然后 /loop 目标"),
            Submission::Loop("看 /tmp/x，然后 目标".to_owned())
        );
    }

    #[test]
    fn a_no_argument_command_still_needs_the_whole_draft() {
        // 无参命令要么是整条、要么什么都不算 —— 一句话里提一句 `/clear` 不会被劫持成一次清空
        // （spec §5，「明确不做」那一条）。
        assert_eq!(
            read("请把 /clear 加到文档里"),
            Submission::Prompt("请把 /clear 加到文档里")
        );
        assert_eq!(read("请看 /undo"), Submission::Prompt("请看 /undo"));
        // 单独一行仍旧是它自己（不变）。
        assert_eq!(read("/clear"), Submission::Clear);
        assert_eq!(read("/undo"), Submission::Undo);
    }

    #[test]
    fn a_draft_without_a_command_token_is_still_a_message() {
        // 草稿里找不到命令记号时整条仍当普通消息（不变）。
        assert_eq!(
            read("看 /tmp/x 这个文件"),
            Submission::Prompt("看 /tmp/x 这个文件")
        );
        assert_eq!(
            read("第一行\n/loopish 目标"),
            Submission::Prompt("第一行\n/loopish 目标")
        );
    }

    #[test]
    fn a_retired_command_is_now_an_unknown_one() {
        // 在它们所控制的那个模式退场之前，`plan` 与 `endplan` 都是内建命令
        // （`.scratch/todo-and-modes`）。现在没有东西去解析它们了，答案是未知命令那段文本 —— 那段
        // 文本会点名真正存在的内建命令。
        assert_eq!(read("/plan"), Submission::Unknown("/plan"));
        assert_eq!(read("/endplan"), Submission::Unknown("/endplan"));
    }

    #[test]
    fn the_mode_flag_parses_the_four_modes_and_refuses_the_rest() {
        use super::parse_interactive;

        let args = |words: &[&str]| {
            parse_interactive(
                &words
                    .iter()
                    .map(|word| (*word).to_owned())
                    .collect::<Vec<_>>(),
            )
        };
        for (written, expected) in [
            ("readonly", Mode::Readonly),
            ("ask", Mode::Ask),
            ("workspace", Mode::Workspace),
            ("auto", Mode::Auto),
        ] {
            assert_eq!(args(&["--mode", written]).unwrap().mode, Some(expected));
        }
        assert_eq!(args(&[]).unwrap().mode, None, "不写表示由文件决定");
        let error = args(&["--mode", "plan"]).unwrap_err();
        for word in ["plan", "readonly", "ask", "workspace", "auto"] {
            assert!(error.contains(word), "`{word}` 没有出现在：{error}");
        }
    }

    #[test]
    fn the_continue_flag_parses_both_spellings_and_defaults_to_off() {
        // `.scratch/exit-gesture/spec.md` 的「补充说明」点名要补的两条缺口之一：`exit(130)` 那条
        // 路径与 `--continue` 的旗标解析（这里的 `mod tests` 原先只测过 `--mode`）。
        use super::{Resume, parse_interactive};

        let args = |words: &[&str]| {
            parse_interactive(
                &words
                    .iter()
                    .map(|word| (*word).to_owned())
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(args(&["--continue"]).unwrap().resume, Some(Resume::Latest));
        assert_eq!(
            args(&["-c"]).unwrap().resume,
            Some(Resume::Latest),
            "短拼写是同一个旗标"
        );
        assert_eq!(args(&[]).unwrap().resume, None, "不写就是不接着跑");
        // 它与别的旗标可以混着写，顺序无所谓。
        let mixed = args(&["--plain", "-c", "--model", "kimi-k3"]).unwrap();
        assert_eq!(mixed.resume, Some(Resume::Latest));
        assert!(mixed.plain);
        assert_eq!(mixed.model.as_deref(), Some("kimi-k3"));
    }

    #[test]
    fn a_session_is_named_by_id_or_by_its_directory() {
        // `heng -c 20261001T155845Z-7a69cbff`：`-c` / `--continue` 后面跟一个不以 `-` 开头
        // 的词就是**指名**续哪一场；`--session <id>` 是同一个意思的显式拼写。
        use super::{Resume, parse_interactive};

        let args = |words: &[&str]| {
            parse_interactive(
                &words
                    .iter()
                    .map(|word| (*word).to_owned())
                    .collect::<Vec<_>>(),
            )
        };
        let named = Some(Resume::Session("20261001T155845Z-7a69cbff".to_owned()));
        assert_eq!(
            args(&["-c", "20261001T155845Z-7a69cbff"]).unwrap().resume,
            named
        );
        assert_eq!(
            args(&["--continue", "20261001T155845Z-7a69cbff"])
                .unwrap()
                .resume,
            named,
            "长拼写也吃 id"
        );
        assert_eq!(
            args(&["--session", "20261001T155845Z-7a69cbff"])
                .unwrap()
                .resume,
            named
        );
        // 会话目录也算 id（`sessions show` 一直这么收）。
        assert_eq!(
            args(&[
                "--session",
                "/tmp/sessions/bucket/20261001T155845Z-7a69cbff"
            ])
            .unwrap()
            .resume,
            Some(Resume::Session(
                "/tmp/sessions/bucket/20261001T155845Z-7a69cbff".to_owned()
            ))
        );

        // `-c` 后面那个词要是旗标，就不当 id —— 否则 `heng -c --plain` 会去续一场叫
        // 「--plain」的会话。
        let mixed = args(&["-c", "--plain"]).unwrap();
        assert_eq!(mixed.resume, Some(Resume::Latest));
        assert!(mixed.plain);
        // 顺序反过来也一样。
        let mixed = args(&["--plain", "-c"]).unwrap();
        assert_eq!(mixed.resume, Some(Resume::Latest));
        assert!(mixed.plain);

        // `--session` 没有可选值：缺了就是打错了，而旗标也不算 id。
        let error = args(&["--session"]).unwrap_err();
        assert!(error.contains("--session"), "{error}");
        let error = args(&["--session", "--plain"]).unwrap_err();
        assert!(error.contains("--session"), "`--plain` 不是 id：{error}");
    }

    #[test]
    fn opening_a_session_is_newest_named_or_fresh() {
        // `choose_session` 是 `-c` 三种含义的**唯一**决定处：新开、续本工作区最新那场、按 id 续
        // 指名的那场（先本桶、再全 store，命中别处时工作目录跟着那场会话走）。
        use super::{Resume, choose_session};
        use crate::events::{Event, EventPayload, SCHEMA_VERSION, SpeakerId};
        use crate::session::SessionStore;

        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("here");
        let there = dir.path().join("there");
        std::fs::create_dir_all(&here).unwrap();
        std::fs::create_dir_all(&there).unwrap();
        let store = SessionStore::new(dir.path().join("store"));
        // 一场会话开在哪里由它流里那条 `SessionStarted` 说 —— 目录名是编码过的桶名，读不回来。
        let seed = |session: &crate::session::store::StoredSession, cwd: &std::path::Path| {
            let event = Event::new(
                1,
                SpeakerId::System,
                EventPayload::SessionStarted {
                    session_id: session.id.clone(),
                    cwd: cwd.display().to_string(),
                    schema_version: SCHEMA_VERSION,
                },
            );
            std::fs::write(
                &session.log_path,
                format!("{}\n", serde_json::to_string(&event).unwrap()),
            )
            .unwrap();
        };

        // 不带 `resume`：新开一场，工作目录就是请求的那个。
        let fresh = choose_session(&store, &here, None).unwrap();
        assert_eq!(fresh.cwd, here);
        assert!(!fresh.elsewhere);
        seed(&fresh.stored, &here);

        // 续最新：这个桶里最近写下的那场。
        let latest = choose_session(&store, &here, Some(&Resume::Latest)).unwrap();
        assert_eq!(latest.stored.id, fresh.stored.id);
        assert_eq!(latest.cwd, here);
        assert!(!latest.elsewhere);

        // 桶里一场都没有：报错，而不是悄悄新开一场 —— `-c` 是「接着跑」，接不上就要说出来。
        let error = choose_session(&store, &there, Some(&Resume::Latest)).unwrap_err();
        assert!(error.contains("没有可继续的会话"), "{error}");

        // 按 id：本桶命中，工作目录不变。
        let named = Resume::Session(fresh.stored.id.as_str().to_owned());
        let found = choose_session(&store, &here, Some(&named)).unwrap();
        assert_eq!(found.stored.id, fresh.stored.id);
        assert_eq!(found.cwd, here);
        assert!(!found.elsewhere, "本桶命中不算「跟着走了」");

        // 按 id：那场会话在**别的工作区**里 —— 找得到（id 全局唯一），而工作目录跟着它走：
        // 续写要发生在它自己的目录里，否则工具的相对路径、前缀缓存与横幅全不对。
        let other = store.create(&there).unwrap();
        seed(&other, &there);
        let elsewhere = Resume::Session(other.id.as_str().to_owned());
        let found = choose_session(&store, &here, Some(&elsewhere)).unwrap();
        assert_eq!(found.stored.id, other.id);
        assert_eq!(found.cwd, there, "续上别处的会话，工作目录跟着那场会话走");
        assert!(found.elsewhere, "这一条要让前端说一句");

        // 会话目录的路径也算 id（`sessions show` 一直这么收）。
        let by_dir = Resume::Session(other.dir.display().to_string());
        let found = choose_session(&store, &here, Some(&by_dir)).unwrap();
        assert_eq!(found.stored.id, other.id);
        assert_eq!(found.cwd, there);
        assert!(found.elsewhere);

        // 找不到：点名那个 id，并且说清搜过哪儿。
        let missing = Resume::Session("20261001T000000Z-deadbeef".to_owned());
        let error = choose_session(&store, &here, Some(&missing)).unwrap_err();
        assert!(error.contains("20261001T000000Z-deadbeef"), "{error}");
        assert!(error.contains("任何其他桶"), "说清搜过哪儿：{error}");
    }

    #[test]
    fn the_mode_flag_overrides_the_configuration_and_its_absence_does_not() {
        use super::{InteractiveArgs, effective_mode};

        // 那一条入口点规则的两个方向：旗标设了就根本不看文件，没设时文件正是这场会话开始于的那一档
        // （`.scratch/todo-and-modes/spec.md` §1）。
        let configured = |written: Option<&str>| {
            crate::config::resolve(written, &crate::config::EnvMap::new()).unwrap()
        };
        let flag = |mode| InteractiveArgs {
            mode,
            ..Default::default()
        };

        let default = configured(None);
        assert_eq!(effective_mode(&flag(None), &default), Mode::Ask);
        assert_eq!(
            effective_mode(&flag(Some(Mode::Readonly)), &default),
            Mode::Readonly
        );

        let auto = configured(Some("[permissions]\nmode = \"auto\"\n"));
        assert_eq!(effective_mode(&flag(None), &auto), Mode::Auto);
        assert_eq!(
            effective_mode(&flag(Some(Mode::Readonly)), &auto),
            Mode::Readonly
        );
    }

    #[test]
    fn a_skill_followed_only_by_blanks_still_only_loads() {
        assert_eq!(
            read("/review\n   \n"),
            Submission::Skill {
                name: "review",
                task: "/review".to_owned(),
                bare: true,
            }
        );
    }

    #[test]
    fn anything_else_is_the_whole_message() {
        let text = "第一行\n第二行\n第三行";
        assert_eq!(read(text), Submission::Prompt(text));
        // 按原样保留：这条消息就是用户打下的东西，不是修剪过的版本。
        assert_eq!(
            read("  第一行\n第二行  "),
            Submission::Prompt("  第一行\n第二行  ")
        );
    }

    // --- 会话内的讨论（spec §15） --------------------------------------------

    #[test]
    fn a_session_discussion_takes_a_question_or_leaves_it_to_the_loop() {
        // 问题就是这一行剩下的部分加上跟在它后面的一切，与技能的任务完全一样：一个讨论的问题可以是
        // 一整段。
        assert_eq!(
            read("/discuss 要不要换掉权限模型"),
            Submission::Discuss("要不要换掉权限模型".to_owned())
        );
        assert_eq!(
            read("/discuss 第一行\n第二行\n\n"),
            Submission::Discuss("第一行\n第二行".to_owned())
        );
        // 光秃秃的：循环把这场会话最后一个问题交给那两位讨论者。
        assert_eq!(read("/discuss"), Submission::Discuss(String::new()));
        assert_eq!(read("  /discuss   "), Submission::Discuss(String::new()));
        // 只有那个名字才是命令：一个叫 `discussion` 的技能就只是个技能。
        assert_eq!(read("/discussion x"), Submission::Unknown("/discussion x"));
    }

    // --- 讨论抽取的池子（spec §15） ------------------------------------------

    use super::{parse_discuss_line, pick_debaters, split_names};
    use crate::config::{Debater, DiscussionRoster};

    fn pool() -> DiscussionRoster {
        DiscussionRoster {
            debaters: vec![
                Debater {
                    name: "保守".to_owned(),
                    model: "kimi-k3".to_owned(),
                    soul: Some("保守".to_owned()),
                },
                Debater {
                    name: "激进".to_owned(),
                    model: "deepseek-v4-pro".to_owned(),
                    soul: None,
                },
                Debater {
                    name: "审查".to_owned(),
                    model: "deepseek-flash".to_owned(),
                    soul: None,
                },
            ],
            max_rounds: None,
        }
    }

    fn names(pair: &[Debater; 2]) -> [String; 2] {
        [pair[0].name.clone(), pair[1].name.clone()]
    }

    #[test]
    fn a_discussion_draws_a_pair_or_takes_the_one_it_is_given() {
        // 点名：恰好是这两位，按要的顺序。
        let picked =
            pick_debaters(&pool(), Some(["审查".to_owned(), "保守".to_owned()]), 0).unwrap();
        assert_eq!(names(&picked), ["审查", "保守"]);
        assert_eq!(picked[0].model, "deepseek-flash");

        // 抽的：池子里两位不同的成员，由种子决定是哪两位。
        let first = pick_debaters(&pool(), None, 0).unwrap();
        assert_eq!(names(&first), ["保守", "激进"]);
        let second = pick_debaters(&pool(), None, 2).unwrap();
        assert_eq!(names(&second), ["保守", "审查"], "种子挪动了这一对");

        // 池子里没有的名字，会说清池子里有什么。
        let error =
            pick_debaters(&pool(), Some(["保守".to_owned(), "没有".to_owned()]), 0).unwrap_err();
        assert!(error.contains("没有"), "{error}");
        assert!(error.contains("`审查`"), "{error}");
    }

    #[test]
    fn a_discussion_command_reads_its_flags_before_the_question() {
        let line = parse_discuss_line("--debaters 保守,激进 换个角度再看").unwrap();
        assert_eq!(line.debaters, Some(["保守".to_owned(), "激进".to_owned()]));
        assert_eq!(line.question, "换个角度再看");

        // 没有要问的问题：循环补上这场会话最后一个。
        let bare = parse_discuss_line("--debaters 保守,激进").unwrap();
        assert_eq!(bare.question, "");
        assert_eq!(bare.debaters.unwrap(), ["保守", "激进"]);

        // 一个不带旗标的问题，含换行。
        let plain = parse_discuss_line("第一行\n第二行").unwrap();
        assert_eq!(plain.debaters, None);
        assert_eq!(plain.question, "第一行\n第二行");

        // `--` 之后的一切都是问题，所以一个问题可以长得像旗标。
        let after = parse_discuss_line("-- --看起来像参数的题目").unwrap();
        assert_eq!(after.question, "--看起来像参数的题目");

        assert!(
            parse_discuss_line("--nope x")
                .unwrap_err()
                .contains("--nope")
        );
        assert!(split_names("保守").unwrap_err().contains("两个名字"));
        assert_eq!(
            split_names("保守, 激进").unwrap(),
            ["保守".to_owned(), "激进".to_owned()],
            "逗号后面的空白没关系"
        );
    }

    #[test]
    fn the_discuss_subcommand_takes_the_same_selection() {
        let parsed = discuss_args(&["--debaters", "保守,激进", "问题"]).unwrap();
        assert_eq!(parsed.debaters.unwrap(), ["保守", "激进"]);
        assert_eq!(parsed.words, vec!["问题".to_owned()]);
        assert!(
            discuss_args(&["--debaters", "只有一个", "问题"])
                .unwrap_err()
                .contains("两个名字")
        );
    }

    // --- 讨论的参数（spec §15） ----------------------------------------------

    use super::{DiscussArgs, parse_discuss, question};

    fn discuss_args(args: &[&str]) -> Result<DiscussArgs, String> {
        parse_discuss(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn a_discussion_question_is_every_word_that_is_not_a_flag() {
        let parsed = discuss_args(&["把", "X", "换掉", "风险在哪"]).unwrap();
        assert_eq!(parsed.words.join(" "), "把 X 换掉 风险在哪");
        assert_eq!(
            question(&parsed.words).as_deref(),
            Some("把 X 换掉 风险在哪")
        );
        // 没有要问的：调用方改为读 stdin。
        assert_eq!(question(&[]), None);
        assert_eq!(question(&["   ".to_owned()]), None);
    }

    #[test]
    fn a_discussion_reads_the_renderer_and_the_paths_it_is_given() {
        let parsed =
            discuss_args(&["--tui", "--cwd", "/tmp/x", "--config", "/tmp/c.toml", "q"]).unwrap();
        assert!(parsed.tui);
        assert!(!parsed.plain);
        assert_eq!(parsed.cwd.unwrap().to_str(), Some("/tmp/x"));
        assert_eq!(parsed.config.unwrap().to_str(), Some("/tmp/c.toml"));
        assert_eq!(parsed.words, vec!["q".to_owned()]);
    }

    #[test]
    fn a_discussion_rejects_what_it_cannot_honour() {
        // 两个渲染器，一个进程。
        assert!(
            discuss_args(&["--plain", "--tui", "q"])
                .unwrap_err()
                .contains("互斥")
        );
        // 一个不存在的旗标，以及一个缺了值的旗标。
        assert!(
            discuss_args(&["--model", "kimi-k3"])
                .unwrap_err()
                .contains("--model")
        );
        assert!(discuss_args(&["--cwd"]).unwrap_err().contains("--cwd"));
        // 没有 `--continue`：一场讨论是一个问题、一个 harness。
        assert!(
            discuss_args(&["--continue"])
                .unwrap_err()
                .contains("--continue")
        );
    }

    #[test]
    fn a_question_that_looks_like_a_flag_goes_after_a_double_dash() {
        let parsed = discuss_args(&["--", "--这段以横线开头"]).unwrap();
        assert_eq!(parsed.words, vec!["--这段以横线开头".to_owned()]);
    }

    // --- `/goal-new`（`.scratch/goal-loop/spec.md` §2） -----------------------

    use super::{GoalNewLine, parse_goal_new_line};

    #[test]
    fn goal_new_is_one_hyphenated_command_taking_a_name_and_a_source() {
        // 连字符形状：一条命令、一个词，于是 `/goal-` 一个前缀就能在 `/` 菜单里把它列出来
        // （空格形状的 `new` 补不出来）。
        assert_eq!(
            read("/goal-new sandbox .scratch/sandbox"),
            Submission::Goal("sandbox .scratch/sandbox".to_owned())
        );
        assert_eq!(
            parse_goal_new_line("sandbox .scratch/sandbox").unwrap(),
            GoalNewLine {
                name: "sandbox".to_owned(),
                sources: vec![".scratch/sandbox".to_owned()],
                force: false,
            }
        );
        assert_eq!(
            read("/goal-new sandbox .scratch/sandbox\n"),
            Submission::Goal("sandbox .scratch/sandbox".to_owned())
        );
    }

    #[test]
    fn goal_new_takes_one_or_more_sources_in_the_order_they_are_written() {
        // 一个目标装几个 feature：第一个词是名字，剩下每一个词是一个来源。
        let line = parse_goal_new_line("seeds .scratch/usage-stats-format .scratch/terminal-title")
            .unwrap();
        assert_eq!(line.name, "seeds");
        assert_eq!(
            line.sources,
            [".scratch/usage-stats-format", ".scratch/terminal-title"]
        );
        assert!(!line.force);

        // `--force` 照旧可以出现在任何位置，而且不占来源的位置。
        let forced = parse_goal_new_line("--force seeds .scratch/a .scratch/b").unwrap();
        assert!(forced.force);
        assert_eq!(forced.sources, [".scratch/a", ".scratch/b"]);

        assert_eq!(
            read("/goal-new seeds .scratch/a .scratch/b"),
            Submission::Goal("seeds .scratch/a .scratch/b".to_owned())
        );
    }

    #[test]
    fn goal_new_without_a_name_and_a_source_is_a_usage_error() {
        for args in ["", "sandbox", "sandbox --force"] {
            assert!(
                parse_goal_new_line(args).is_err(),
                "`{args}` 不是一条合法的 /goal-new"
            );
        }
        // 退役的空格形状是未知命令，与 `/plan` 一样的读法：命令名就是命令名。
        assert_eq!(
            read("/goal new sandbox .scratch/sandbox"),
            Submission::Unknown("/goal new sandbox .scratch/sandbox")
        );
    }

    // --- `/clear`（`.scratch/goal-loop/spec.md` §12） --------------------------

    #[test]
    fn clear_is_a_whole_submission_and_not_a_task() {
        assert_eq!(read("/clear"), Submission::Clear);
        assert_eq!(read("  /clear  "), Submission::Clear, "去掉空白照旧");
        // 内建命令要么是整条提交，要么什么都不算：它后面的一行绝不能被丢在地上。
        assert_eq!(
            read("/clear\n再写点什么"),
            Submission::Prompt("/clear\n再写点什么")
        );
    }

    // --- 无人值守的前提：档位（`.scratch/goal-loop/spec.md` §5） ---------------

    /// 一个从不被调用的 provider：拒绝发生在任何一次调用之前，所以这里被调到就是测试失败。
    struct NoCalls;

    #[async_trait::async_trait]
    impl crate::provider::Provider for NoCalls {
        async fn send(
            &self,
            _request: crate::provider::ChatRequest,
        ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
            Err(crate::provider::ProviderError::Transport {
                detail: "这个测试不该调到 provider".to_owned(),
            })
        }

        fn caps(&self) -> crate::provider::capability::ModelCaps {
            crate::provider::capability::caps_for("deepseek-flash").expect("内置模型")
        }
    }

    #[tokio::test]
    async fn an_ask_mode_session_refuses_a_goal_loop_before_writing_anything() {
        // 票 06 的 e2e：`ask` 档下 `/loop` 拒绝，而**流上没有** `GoalSelected` —— 拒绝发生在写
        // 任何事件之前。走的是**组装入口 + 假 provider**，也就是 spec 说的那条接缝。
        use super::{GoalSetup, run_goal_loop};
        use crate::config::SessionConfig;
        use crate::events::{EventPayload, SpeakerId};
        use crate::permissions::Policy;
        use crate::render::{RenderSinks, Renderer};
        use crate::session::SessionStore;
        use crate::tools::{self, PathLocks};
        use crate::{AssemblyParts, SessionScaffold, assemble};

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let goals_dir = dir.path().join("goals");
        crate::goals::create(
            "sandbox",
            &[std::path::PathBuf::from(".scratch/sandbox")],
            &goals_dir,
            false,
        )
        .expect("清单从票生成");

        let store = SessionStore::new(dir.path().join("store"));
        let stored = store.create(&workspace).unwrap();
        let mut harness = assemble(AssemblyParts {
            provider: Box::new(NoCalls),
            speaker: SpeakerId::Debater("kimi".into()),
            config: SessionConfig::new("fake-model"),
            renderer: Renderer::headless(RenderSinks {
                stdout_result: Box::new(std::io::sink()),
                stderr_diagnostic: Box::new(std::io::sink()),
            }),
            scaffold: SessionScaffold {
                cwd: workspace.clone(),
                log_path: stored.log_path.clone(),
                session_id: stored.id.clone(),
                tools: tools::builtin(false),
                locks: PathLocks::new(),
                policy: Policy::for_mode(Mode::Ask),
                asker: None,
                questions: None,
                hook: None,
                home: None,
            },
        })
        .await
        .expect("组装一场 ask 档的会话");

        let (console, _port, mut events) = crate::render::console();
        let goals = GoalSetup {
            dir: Some(goals_dir),
            store,
            cwd: workspace,
            settings: crate::config::GoalSettings::default(),
        };
        let mut running = false;
        let refusal = run_goal_loop(
            &mut harness,
            &console,
            &mut events,
            &goals,
            "sandbox",
            &mut running,
            &mut ExitRequest::default(),
        )
        .await
        .expect_err("ask 档下拒绝启动");

        assert!(refusal.contains("无人值守"), "{refusal}");
        assert!(refusal.contains("workspace"), "要说清换哪一档：{refusal}");
        assert!(!running, "被拒的启动没有把循环标成在跑");
        assert!(
            !harness
                .events()
                .iter()
                .any(|event| matches!(event.payload, EventPayload::GoalSelected { .. })),
            "拒绝发生在写事件之前：流上没有归属"
        );

        harness.shutdown().await;
    }

    #[tokio::test]
    async fn an_auto_mode_session_starts_the_loop_and_lands_its_ownership_on_the_stream() {
        // 另一半 e2e：档位够了就真的启动 —— 归属落流、清单注入，然后跑一个回合。provider 这里
        // 每一次都失败，而 `provider_retries = 0` 让循环当场停下（不睡那两次重试），于是这条
        // 用例既证明「能启动」，也把「provider 失败 → 停下 + 收尾事件」走了一遍。
        use super::{GoalSetup, run_goal_loop};
        use crate::config::SessionConfig;
        use crate::events::{EventPayload, SpeakerId};
        use crate::permissions::Policy;
        use crate::render::{RenderSinks, Renderer};
        use crate::session::SessionStore;
        use crate::tools::{self, PathLocks};
        use crate::{AssemblyParts, SessionScaffold, assemble};

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let goals_dir = dir.path().join("goals");
        crate::goals::create(
            "sandbox",
            &[std::path::PathBuf::from(".scratch/sandbox")],
            &goals_dir,
            false,
        )
        .expect("清单从票生成");

        let store = SessionStore::new(dir.path().join("store"));
        let stored = store.create(&workspace).unwrap();
        let mut harness = assemble(AssemblyParts {
            provider: Box::new(NoCalls),
            speaker: SpeakerId::Debater("kimi".into()),
            config: SessionConfig::new("fake-model"),
            renderer: Renderer::headless(RenderSinks {
                stdout_result: Box::new(std::io::sink()),
                stderr_diagnostic: Box::new(std::io::sink()),
            }),
            scaffold: SessionScaffold {
                cwd: workspace.clone(),
                log_path: stored.log_path.clone(),
                session_id: stored.id.clone(),
                tools: tools::builtin(false),
                locks: PathLocks::new(),
                policy: Policy::for_mode(Mode::Auto),
                asker: None,
                questions: None,
                hook: None,
                home: None,
            },
        })
        .await
        .expect("组装一场 auto 档的会话");

        let (console, _port, mut events) = crate::render::console();
        let goals = GoalSetup {
            dir: Some(goals_dir),
            store,
            cwd: workspace,
            settings: crate::config::GoalSettings {
                provider_retries: 0,
                ..crate::config::GoalSettings::default()
            },
        };
        let mut running = false;
        run_goal_loop(
            &mut harness,
            &console,
            &mut events,
            &goals,
            "sandbox",
            &mut running,
            &mut ExitRequest::default(),
        )
        .await
        .expect("auto 档下启动不拒绝");

        let written = harness.events();
        assert!(
            written
                .iter()
                .any(|event| matches!(event.payload, EventPayload::GoalSelected { .. })),
            "启动之后归属落流"
        );
        assert!(
            written
                .iter()
                .any(|event| matches!(event.payload, EventPayload::ContextInjected { .. })),
            "清单也注入了上下文"
        );
        assert!(
            written.iter().any(|event| matches!(
                &event.payload,
                EventPayload::GoalStopped { reason, count, .. }
                    if *reason == crate::events::GoalStopReason::ProviderFailed && *count == 1
            )),
            "provider 失败一次就停下报告：{written:?}"
        );
        assert!(!running, "停下之后回到空闲");

        harness.shutdown().await;
    }

    #[tokio::test]
    async fn the_budget_report_names_only_the_entries_that_are_still_open() {
        // 报告里点名的必须是**这一刻**没完成的那几条。进度由 `stop_goal_loop` 自己派生，所以
        // 这里钉的是它算对了：预造一条「已经认领目标、且已把 01 标完成」的流，再让额度为 0 ——
        // 循环在第一个回合开头就撞顶，报告不许再把 01 列成「还卡着」（它第一次真机跑出来时正是
        // 这么误导的：11 条全列上，而 01、02 其实已经做完）。
        use super::{GoalSetup, run_goal_loop};
        use crate::config::SessionConfig;
        use crate::events::{
            Event, EventPayload, GoalStopReason, SCHEMA_VERSION, SpeakerId, ToolCallId,
        };
        use crate::permissions::Policy;
        use crate::render::{RenderSinks, Renderer};
        use crate::session::SessionStore;
        use crate::tools::{self, PathLocks};
        use crate::{AssemblyParts, SessionScaffold, assemble};

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let goals_dir = dir.path().join("goals");
        crate::goals::create(
            "sandbox",
            &[std::path::PathBuf::from(".scratch/sandbox")],
            &goals_dir,
            false,
        )
        .expect("清单从票生成");

        let store = SessionStore::new(dir.path().join("store"));
        let stored = store.create(&workspace).unwrap();
        let seeded = [
            Event::new(
                1,
                SpeakerId::System,
                EventPayload::SessionStarted {
                    session_id: stored.id.clone(),
                    cwd: workspace.display().to_string(),
                    schema_version: SCHEMA_VERSION,
                },
            ),
            Event::new(
                2,
                SpeakerId::System,
                EventPayload::GoalSelected {
                    goal: "sandbox".to_owned(),
                },
            ),
            Event::new(
                3,
                SpeakerId::Debater("kimi".into()),
                EventPayload::ToolCallStarted {
                    tool_call_id: ToolCallId::new("call-1"),
                    tool_name: tools::TODO_TOOL.to_owned(),
                    args: serde_json::json!({
                        "items": [{"id": "01", "content": "一条", "status": "completed"}]
                    }),
                },
            ),
            Event::new(
                4,
                SpeakerId::Debater("kimi".into()),
                EventPayload::ToolCallCompleted {
                    tool_call_id: ToolCallId::new("call-1"),
                    ok: true,
                    output: Some("todo：1 项（1 项已完成）".to_owned()),
                    error: None,
                    duration_ms: 1,
                },
            ),
        ];
        let mut jsonl = String::new();
        for event in &seeded {
            jsonl.push_str(&serde_json::to_string(event).unwrap());
            jsonl.push('\n');
        }
        std::fs::write(&stored.log_path, jsonl).unwrap();

        let mut harness = assemble(AssemblyParts {
            provider: Box::new(NoCalls),
            speaker: SpeakerId::Debater("kimi".into()),
            config: SessionConfig::new("fake-model").with_session_token_limit(0),
            renderer: Renderer::headless(RenderSinks {
                stdout_result: Box::new(std::io::sink()),
                stderr_diagnostic: Box::new(std::io::sink()),
            }),
            scaffold: SessionScaffold {
                cwd: workspace.clone(),
                log_path: stored.log_path.clone(),
                session_id: stored.id.clone(),
                tools: tools::builtin(false),
                locks: PathLocks::new(),
                policy: Policy::for_mode(Mode::Auto),
                asker: None,
                questions: None,
                hook: None,
                home: None,
            },
        })
        .await
        .expect("组装一场接手的会话");

        let (console, _port, mut front_end) = crate::render::console();
        let goals = GoalSetup {
            dir: Some(goals_dir),
            store,
            cwd: workspace,
            settings: crate::config::GoalSettings::default(),
        };
        let mut running = false;
        run_goal_loop(
            &mut harness,
            &console,
            &mut front_end,
            &goals,
            "sandbox",
            &mut running,
            &mut ExitRequest::default(),
        )
        .await
        .expect("额度为 0 是一次正常的降级收尾");

        let (reason, stuck) = harness
            .events()
            .into_iter()
            .find_map(|event| match event.payload {
                EventPayload::GoalStopped { reason, stuck, .. } => Some((reason, stuck)),
                _ => None,
            })
            .expect("撞顶落了一条收尾");
        assert_eq!(reason, GoalStopReason::BudgetExhausted);
        assert!(
            !stuck.contains(&"01".to_owned()),
            "01 已经做完，不该出现在「还卡着」里：{stuck:?}"
        );
        assert!(
            stuck.contains(&"02".to_owned()),
            "没做的那些要在里面：{stuck:?}"
        );

        harness.shutdown().await;
    }

    // --- `/` 菜单（`.scratch/goal-loop/spec.md` §13） --------------------------

    use super::slash_catalog;
    use crate::render::wording::BUILT_IN_COMMANDS;

    #[test]
    fn the_menu_lists_the_built_in_commands_before_the_skills() {
        // 命令一组、技能一组；同前缀时命令优先，因为程序自带的那批更可预测。
        let skills = [
            ("clear-ish", "一个与 /c 同前缀的技能"),
            ("ask-matt", "审一遍"),
        ];
        let entries = slash_catalog(&BUILT_IN_COMMANDS, &skills, &[]);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();

        let declared: Vec<&str> = BUILT_IN_COMMANDS
            .iter()
            .map(|command| command.name)
            .collect();
        assert_eq!(
            &names[..declared.len()],
            declared.as_slice(),
            "命令在前，组内就是声明顺序（不按字母）"
        );
        assert_eq!(
            &names[declared.len()..],
            ["ask-matt", "clear-ish"],
            "技能在后，按名字"
        );

        let clear = names.iter().position(|name| *name == "clear").unwrap();
        let skill = names.iter().position(|name| *name == "clear-ish").unwrap();
        assert!(clear < skill, "同前缀时命令优先：{names:?}");
    }

    #[test]
    fn the_menu_covers_exactly_the_commands_the_loop_parses() {
        // 防「加了命令忘了补全」：`submission()` 认哪些名字，`/` 菜单就列出哪些。两边各有一份
        // 真相（参数形状只能各自解析），所以一致性由这条测试钉住。
        let mut parsed: Vec<&str> = Vec::new();
        for name in [
            "undo", "discuss", "goal-new", "loop", "model", "effort", "clear", "quit", "exit",
        ] {
            let line = format!("/{name}");
            let submission = read(&line);
            assert!(
                !matches!(submission, Submission::Unknown(_) | Submission::Prompt(_)),
                "{line} 是一条内建命令，不是未知命令、也不是 prompt"
            );
            parsed.push(name);
        }
        let listed: Vec<&str> = BUILT_IN_COMMANDS
            .iter()
            .map(|command| command.name)
            .collect();
        assert_eq!(
            listed, parsed,
            "补全里列的就是循环解析的那些，一个不多一个不少"
        );

        // 一个真不存在的名字仍然是未知命令。
        assert!(matches!(read("/nope"), Submission::Unknown("/nope")));
    }

    // --- `/loop <名字>`（`.scratch/goal-loop/spec.md` §4） ---------------------

    use super::parse_loop_line;

    #[test]
    fn loop_takes_exactly_one_name() {
        assert_eq!(parse_loop_line(" sandbox ").unwrap(), "sandbox");
        assert_eq!(
            read("/loop sandbox"),
            Submission::Loop("sandbox".to_owned())
        );

        // 名字是一个不断开的词，所以一个都没有、多出一个都是用法错误 —— 而这条命令读到的
        // 是整条提交，与 `/discuss`、`/goal-new` 同一条规矩。
        for args in ["", "   ", "sandbox extra", "两个 词"] {
            let message = parse_loop_line(args).unwrap_err();
            assert!(message.contains("/loop"), "`{args}`：{message}");
        }
        assert_eq!(
            read("/loop sandbox\n"),
            Submission::Loop("sandbox".to_owned())
        );
    }
}
