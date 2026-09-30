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

use crate::agent::replay;
use crate::config::{self, Config, Debater, DiscussionRoster, EnvMap};
use crate::events::{
    read_events, total_usage, Event, EventPayload, SessionId, SpeakerId, StopReason, Usage,
};
use crate::permissions::{Mode, Policy};
use crate::provider::capability::caps_for;
use crate::provider::openai::{stderr_warnings, BuildError, OpenAiProvider};
use crate::provider::Message;
use crate::questions::UserQuestions;
use crate::render::{
    self, ConsoleAsker, ConsoleEvents, ConsoleHandle, ConsoleQuestions, FrontEndEvent,
    PlainOptions, RenderSinks, Renderer, SessionFacts, TuiOptions,
};
use crate::session::observe::{self, CostModel, Entry, Filter, Listing, Timeline};
use crate::session::{SessionStore, StoredSession};
use crate::tools::{self, PathLocks};
use crate::{
    assemble, assemble_discussion, AssemblyParts, DebaterParts, DiscussionHarness, DiscussionParts,
    Harness, SessionScaffold, SynthesizerParts,
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
        eprintln!("fs-agent: {message}");
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
                "fs-agent: {}",
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
            println!("fs-agent {}", env!("CARGO_PKG_VERSION"));
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
        // `fs-agent`。`interactive` 自己解析自己那批参数，并拒掉任何它不认识的东西。
        _ => interactive(args, env).await,
    }
}

// ---------------------------------------------------------------------------
// 交互式前端（spec §19）
// ---------------------------------------------------------------------------

/// 一次已解析的交互式调用。渲染器在这里选定并注入组装，所以只会有一个模式在跑。
#[derive(Debug, Default)]
struct InteractiveArgs {
    /// 强制用 plain 渲染器。
    plain: bool,
    /// 强制用 TUI 渲染器。
    tui: bool,
    /// 接着跑这个工作区最新的那场会话（spec §11）。
    resume: bool,
    config: Option<PathBuf>,
    model: Option<String>,
    /// `--mode readonly|ask|auto`：这一趟跑的权限模式，覆盖 `[permissions] mode`（spec §12）。不
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
            "--continue" | "-c" => parsed.resume = true,
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
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_interactive());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };
    // 没登记的模型是启动错误，绝不是悄悄降级。
    if let Err(message) = validate_models(&config) {
        eprintln!("fs-agent: {message}");
        return ExitCode::FAILURE;
    }
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let profile = match config.resolve_model(Some(&model)) {
        Ok((_, profile)) => profile.clone(),
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!("fs-agent: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_cwd(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        },
    };
    let stored = if parsed.resume {
        match store.latest(&cwd) {
            Ok(Some(session)) => session,
            Ok(None) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_no_session_to_continue(&cwd.display().to_string())
                );
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_store_read(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        match store.create(&cwd) {
            Ok(session) => session,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_store_create(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        }
    };

    let provider = match OpenAiProvider::build(&config, &model, stderr_warnings()) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };
    let session_config = match config.session_config(&model) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

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
                eprintln!("fs-agent: {error}");
                return ExitCode::FAILURE;
            }
        };
        let facts = SessionFacts {
            session_id: stored.id.as_str().to_owned(),
            session_dir: stored.dir.display().to_string(),
            model: model.clone(),
            context_window: crate::context::usable_input(&caps),
            mode,
            budget_limit: session_config.budget.limit,
            // 单 agent 会话以它的档案发言，所以那就是转录的名字配色需要安排的全部名册
            // （票 07 §1）。
            speaker_order: vec![profile.name.clone()],
        };
        Renderer::tui(TuiOptions {
            port,
            facts,
            // 重新打开的会话会在横幅之前重放它的历史，所以 TUI 必须知道，在那次重放到达之前什么都
            // 不要画（`.scratch/tui-history-replay/spec.md` §1、§3）。
            reopened: parsed.resume,
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

    let mut harness = match assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            // 工具表在这里、在组装处定下：内建的那些加上每一个动态声明的工具（spec §14）。
            tools: tools::with_dynamic(&config.tools, questions.is_some()),
            locks: PathLocks::new(),
            // 用户选的那一档：`[permissions] mode`，或者压在它上面的 `--mode`（spec §12）。无头调
            // 用方没有应答者，于是降级。
            policy: Policy::for_mode(mode),
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
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };

    // 重新打开的会话组装时带上的历史，在横幅之前推给前端，好让 TUI 先把它铺好、横幅落在接缝**之
    // 后**，而不是落在接缝中间（`.scratch/tui-history-replay/spec.md` §1）。payload 就是组装好的
    // 那份快照，所以它带着 `--continue` 的恢复为悬空工具调用写下的合成结果。新会话没有历史，所以
    // 什么都不发。
    if parsed.resume {
        console.replay(harness.events());
    }

    // 用户故事 A.12：在第一个问题之前说清这是哪个模型、哪一档、哪场会话。它走渲染器而不是 stderr：
    // TUI 在 harness 组装时就已经启动，第二个往终端写的人会落在它的活动区域里、盖在状态行上。
    harness.notice(&render::wording::banner(
        harness.session_id().as_str(),
        &model,
        harness.mode(),
        &stored.dir.display().to_string(),
        parsed.resume,
    ));

    // `/` 菜单里的名字。真正对提交作出反应的是循环，所以「有哪些名字」也由循环说了算：它解析的那
    // 些内建命令，然后是这场会话发现到的技能 —— 这正是它在这里、在组装之后、第一个提示之前发出，
    // 而不是随表头那些事实一起注入的原因。
    console.catalog(
        render::wording::BUILT_IN_COMMANDS
            .iter()
            .map(|command| render::CatalogEntry::new(command.name, command.description))
            .chain(
                harness
                    .skill_catalog()
                    .into_iter()
                    .map(|(name, description)| render::CatalogEntry::new(name, description)),
            )
            .collect(),
    );

    let code = interactive_loop(&mut harness, &console, &mut events, &config).await;
    harness.shutdown().await;
    code
}

// ---------------------------------------------------------------------------
// 讨论前端（spec §15）
// ---------------------------------------------------------------------------

/// 一次已解析的 `fs-agent discuss` 调用。
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
                return Err(render::wording::unknown_argument(other))
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
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_discuss());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };
    // 没登记的模型是启动错误，绝不是悄悄降级。
    if let Err(message) = validate_models(&config) {
        eprintln!("fs-agent: {message}");
        return ExitCode::FAILURE;
    }
    // 文件里没有名册不算错误 —— 大多数配置都是给单 agent 会话用的 —— 但对这个子命令算。
    let Some(roster) = config.discussion.clone() else {
        eprintln!("fs-agent: {}", render::wording::discussion_no_roster());
        return ExitCode::FAILURE;
    };
    // 池子里哪两位参与讨论：命令行点了名的，或者抽出来的。
    let pair = match pick_debaters(&roster, parsed.debaters.clone(), discussion_seed()) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            return ExitCode::FAILURE;
        }
    };
    for line in advisory_lines(&config, &pair) {
        eprintln!("fs-agent: {line}");
    }

    // 问题在任何东西被组装之前就要拿到：读它可能在终端上阻塞，而那时终端必须还在 cooked 模式下
    // （组装会把 TUI 切进 raw 模式，并从那一刻起占住屏幕）。
    let Some(question) = question(&parsed.words) else {
        eprintln!("fs-agent: {}", render::wording::discuss_needs_question());
        return ExitCode::FAILURE;
    };

    let Some(root) = config::sessions_dir(env) else {
        eprintln!("fs-agent: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_cwd(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        },
    };
    let stored = match SessionStore::new(root).create(&cwd) {
        Ok(session) => session,
        Err(error) => {
            eprintln!(
                "fs-agent: {}",
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
            eprintln!("fs-agent: {message}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "fs-agent: {}",
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
                eprintln!("fs-agent: {error}");
                return ExitCode::FAILURE;
            }
        };
        let facts = SessionFacts {
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
            // 这一对，按名册顺序 —— 与 `pick_pair` 产出的顺序相同，正是它把第一个调色板槽位给了第
            // 一位讨论者（票 07 §1）。
            speaker_order: vec![pair[0].name.clone(), pair[1].name.clone()],
        };
        // 一场讨论是一个问题、一个 harness：它从不是一次重新打开，所以没有历史要重放，TUI 从第一
        // 帧起就在画。
        Renderer::tui(TuiOptions {
            port,
            facts,
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

    let mut harness = match assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd,
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            tools: tools::with_dynamic(&config.tools, questions.is_some()),
            locks: PathLocks::new(),
            // 文件里的模式：讨论者与任何会话一样走同一个权限门，而名册共享一个策略（spec §12、
            // §15）。
            policy: Policy::for_mode(config.mode),
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
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };

    let outcome = run_discussion(&mut harness, &mut events, &question).await;
    harness.shutdown().await;

    // 这行在 alt screen 恢复之后打印，因为 TUI 的转录活不过进程：用户在屏幕上漏掉的东西，会话 id
    // 是回到它身边的持久途径。
    match outcome {
        Ok(outcome) => {
            eprintln!(
                "fs-agent: {}",
                render::wording::discussion_ended(outcome.reason, outcome.rounds, &outcome.absent)
            );
            eprintln!(
                "fs-agent: {}",
                render::wording::discussion_replay(stored.id.as_str())
            );
            // 整场失败掉的讨论是一次失败；被取消的那场正是用户要的，如同交互式会话里被取消的一个回
            // 合（spec §6）。
            if outcome.reason == StopReason::Error {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("fs-agent: {}", render::wording::error_report(&error));
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
                        })
                    }
                }
            }
            "--" => {
                return Ok(DiscussLine {
                    debaters,
                    question: tail.trim_start().to_owned(),
                })
            }
            other if other.starts_with("--") => {
                return Err(render::wording::unknown_argument(other))
            }
            _ => {
                return Ok(DiscussLine {
                    debaters,
                    question: rest.to_owned(),
                })
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
) -> Result<(), crate::Error> {
    let Some(roster) = config.discussion.clone() else {
        harness.notice(&format!(
            "fs-agent: {}",
            render::wording::discussion_no_roster()
        ));
        return Ok(());
    };
    // 先 `--debaters a,b`，再是问题：旗标属于这条命令，而这一行剩下的部分才是用户要问的东西。
    let line = match parse_discuss_line(&asked) {
        Ok(line) => line,
        Err(message) => {
            harness.notice(&format!("fs-agent: {message}"));
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
                    "fs-agent: {}",
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
            harness.notice(&format!("fs-agent: {message}"));
            return Ok(());
        }
    };
    let (debaters, synthesizer) = match discussion_participants(config, &pair) {
        Ok(parts) => parts,
        Err(message) => {
            harness.notice(&format!("fs-agent: {message}"));
            return Ok(());
        }
    };
    for line in advisory_lines(config, &pair) {
        harness.notice(&format!("fs-agent: {line}"));
    }
    harness.notice(&format!(
        "fs-agent: {}",
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
    // 这个 future 在它运行的整段时间里借走 harness，所以它自成一个作用域：下面的通告要把 harness
    // 拿回来。
    let outcome = {
        let mut run =
            Box::pin(harness.discuss(&question, debaters, synthesizer, roster.max_rounds));
        loop {
            tokio::select! {
                result = &mut run => break result?,
                event = events.recv() => match event {
                    Some(FrontEndEvent::Cancel) => {
                        if signal.is_cancelled() {
                            std::process::exit(130);
                        }
                        signal.cancel();
                    }
                    // 输入结束或显式退出让讨论像被取消一样落下来，所以流仍然得到它的收尾。
                    Some(FrontEndEvent::Quit) | None => signal.cancel(),
                    // 模式是权限门每次调用都读的一个值，所以这次按键立刻生效、哪怕是在讨论中途：句
                    // 柄之所以存在，是因为 run future 借走了 harness（spec §12）。
                    Some(FrontEndEvent::CycleMode) => {
                        modes.cycle();
                    }
                },
            }
        }
    };
    harness.notice(&format!(
        "fs-agent: {}",
        render::wording::discussion_ended(outcome.reason, outcome.rounds, &outcome.absent)
    ));
    Ok(())
}

/// 驱动一场讨论，同时仍然盯着取消手势。
///
/// 形状与 [`run_one_turn`] 相同，理由也相同：讨论占着会话，所以循环没法自己读键盘，改为监听手势。
/// 在取消已经举起时再按一次会把进程按下去 —— 流仍然得到它的收尾，因为第一次按键已经要了它。
async fn run_discussion(
    harness: &mut DiscussionHarness,
    events: &mut ConsoleEvents,
    question: &str,
) -> Result<crate::agent::DiscussionOutcome, crate::Error> {
    let signal = harness.cancel_signal();
    let modes = harness.mode_cycle();
    let mut run = Box::pin(harness.discuss(question));
    loop {
        tokio::select! {
            result = &mut run => return result,
            event = events.recv() => match event {
                Some(FrontEndEvent::Cancel) => {
                    if signal.is_cancelled() {
                        std::process::exit(130);
                    }
                    signal.cancel();
                }
                // 输入结束或显式退出让讨论像被取消一样落下来，所以流仍然得到它的收尾。
                Some(FrontEndEvent::Quit) | None => signal.cancel(),
                // 一个策略盖住三位参与者，所以这与它在别处的是同一个手势（spec §12）。
                Some(FrontEndEvent::CycleMode) => {
                    modes.cycle();
                }
            },
        }
    }
}

/// 那个问题：用它被给进来时的那些词，或者 stdin。
///
/// 终端上给一个提示 —— 还没有任何东西进过 raw 模式，所以 cooked 读仍然可用 —— 而管道读到结尾，这
/// 正是 `echo 问题 | fs-agent discuss` 需要的。
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

/// 读一行、跑它、重复 —— 直到用户离开或输入结束。
async fn interactive_loop(
    harness: &mut Harness,
    console: &ConsoleHandle,
    events: &mut ConsoleEvents,
    config: &Config,
) -> ExitCode {
    loop {
        // 只有循环知道有没有东西在跑，所以它告诉前端，而不是让前端去推断（spec §6）。在拿到一次提
        // 交之前什么都没在跑，而前端从第一帧起就得这么读 —— 包括在这个循环第一次提问之前。
        console.set_running(false);
        // 回合之间循环只在等一次提示；这时候到达的手势在没有回合在飞的情况下处理掉。
        let line = loop {
            tokio::select! {
                line = console.prompt() => break line,
                event = events.recv() => match event {
                    Some(FrontEndEvent::Quit) | None => return ExitCode::SUCCESS,
                    Some(FrontEndEvent::CycleMode) => {
                        harness.mode_cycle().cycle();
                    }
                    Some(FrontEndEvent::Cancel) => {}
                },
            }
        };
        let Some(submitted) = line else {
            return ExitCode::SUCCESS;
        };
        // 拿到一行了，所以从这里到这个循环的下一次轮转之间，会话正在跑东西 —— 一个回合、一场讨论、
        // 一次 `/undo`。`Ctrl-C` 是它们共同的取消手势。
        console.set_running(true);
        match submission(&submitted, |name| harness.has_skill(name)) {
            // 一个空行：没有可答的，于是再问一次。提示处返回 `None` 是唯一结束输入的东西，而那种情
            // 况就在上面处理。
            Submission::Ignore => {}
            Submission::Quit => return ExitCode::SUCCESS,
            Submission::Undo => match harness.undo_last_edit().await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    harness.notice(&format!("fs-agent: {}", render::wording::nothing_to_undo()))
                }
                Err(error) => harness.notice(&format!(
                    "fs-agent: {}",
                    render::wording::error_report(&error)
                )),
            },
            Submission::Unknown(line) => {
                let names = harness.skill_names();
                harness.notice(&format!(
                    "fs-agent: {}",
                    render::wording::unknown_command(line, &names)
                ));
            }
            // `/<skill> [task]` 是用户侧的技能调用（spec §9）：`disable-model-invocation: true`
            // 的技能为用户留下的那条路。正文进上下文尾部。光秃秃的 `/<skill>` 就地跑 —— 正文**就
            // 是**指令 —— 而打了任务时，任务作为一条普通 user 消息跟在它后面。
            Submission::Skill { name, task } => {
                if task.is_empty() {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::skill_started(name)
                    ));
                    if let Err(error) = run_one_turn(harness, events, TurnStart::Skill(name)).await
                    {
                        harness.notice(&format!(
                            "fs-agent: {}",
                            render::wording::error_report(&error)
                        ));
                    }
                } else if let Err(error) = harness.load_skill(name) {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::error_report(&error)
                    ));
                } else {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::skill_loaded(name)
                    ));
                    if let Err(error) =
                        run_one_turn(harness, events, TurnStart::Prompt(&task)).await
                    {
                        harness.notice(&format!(
                            "fs-agent: {}",
                            render::wording::error_report(&error)
                        ));
                    }
                }
            }
            // `/discuss [问题]`：在**当前会话的流上**的一场讨论（spec §15）。讨论者是这场会话的兄
            // 弟，所以继承它的上下文，并把轮次追加到它的日志上；之后用户回到提示符前。
            Submission::Discuss(question) => {
                if let Err(error) = discuss_in_session(harness, events, config, question).await {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::error_report(&error)
                    ));
                }
            }
            // 其余的都是 prompt，含换行：转录把它显示成用户写下的那一条消息（spec §12）。
            Submission::Prompt(text) => {
                if let Err(error) = run_one_turn(harness, events, TurnStart::Prompt(text)).await {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::error_report(&error)
                    ));
                }
            }
        }
    }
}

/// 跟在命令名之后、位于它第一行上的任务或问题：那一行剩下的部分，然后是下面每一行，按原样。只丢
/// 结尾的空白行，所以一段全是空白的续行不算任务。
///
/// `/<skill>` 与 `/discuss` 共用一个函数，因为「用户在命令之后写了什么」是一条规则，两份拷贝迟早
/// 会漂移。
fn task_of(inline: &str, rest: &str) -> String {
    let mut task = inline.to_owned();
    let rest = rest.trim_end_matches('\n');
    if !rest.trim().is_empty() {
        if !task.is_empty() {
            task.push('\n');
        }
        task.push_str(rest);
    }
    task
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
    Skill {
        name: &'a str,
        task: String,
    },
    /// `/discuss [问题]`：在**当前会话的流上**跑一场讨论（spec §15）。用户没打问题时问题为空 ——
    /// 那时循环把这场会话最后一个问题交给那两位讨论者。
    Discuss(String),
    /// 整条提交，含换行，作为一条 prompt。
    Prompt(&'a str),
}

/// 读一次提交。
///
/// 由**只看第一行**来决定这是不是一条命令，所以 `/<skill>` 后面可以跟一段多行的简报 —— 它第一行剩
/// 下的部分和下面每一行都成为任务。第一行以 `/` 开头却点名了不认识的东西时，如果整条提交就这一行那
/// 就是一个错字（会告诉用户，一如既往），否则就是贴进来的一段文字。
///
/// 去掉空白后为空的提交是 [`Submission::Ignore`]：没有要发的东西，所以循环再问一次，而不是起一个回
/// 合。
///
/// 内建命令不接受任务，所以它们只在作为**整条**提交时匹配：`/undo` 后面的一行绝不能被丢在地上。这样
/// 的提交会落到上面那几条规则上 —— `/undo` 没点名任何技能，所以带着后面的行时整条被当作一条 prompt
/// 读，至少还让用户看见自己发了什么。
///
/// 开头的斜杠一律剥掉（`//undo` 读作 `undo`），旧代码读技能名时也是这样。任务结尾的空白行丢掉；其
/// 余按原样保留。
fn submission<'a>(text: &'a str, has_skill: impl Fn(&str) -> bool) -> Submission<'a> {
    if text.trim().is_empty() {
        return Submission::Ignore;
    }
    let (first, rest) = match text.split_once('\n') {
        Some((first, rest)) => (first.trim(), rest),
        None => (text.trim(), ""),
    };
    // 内建命令要么是整条提交，要么什么都不算：它没有任务来装后面的那些行，而丢掉它们会丢掉用户写下
    // 的东西。
    let whole = rest.trim().is_empty();
    match first {
        "/quit" | "/exit" if whole => Submission::Quit,
        "/undo" if whole => Submission::Undo,
        _ if first.starts_with('/') => {
            let rest_of_line = first.trim_start_matches('/');
            let (name, inline) = match rest_of_line.split_once(char::is_whitespace) {
                Some((name, task)) => (name, task.trim()),
                None => (rest_of_line, ""),
            };
            // `/discuss [问题]` 是唯一带参数的内建命令，所以它取与技能的任务相同的形状：这一行剩下
            // 的部分，然后是下面每一行。光秃秃的 `/discuss` 把问题留给循环。
            if name == "discuss" {
                return Submission::Discuss(task_of(inline, rest));
            }
            if !has_skill(name) {
                if rest.trim().is_empty() {
                    return Submission::Unknown(first);
                }
                // 一段碰巧以 `/` 开头的贴进来的文字，就是一段文字。
                return Submission::Prompt(text);
            }
            Submission::Skill {
                name,
                task: task_of(inline, rest),
            }
        }
        // 根本不是命令：整段文本，不管多少行，就是 prompt。
        _ => Submission::Prompt(text),
    }
}

/// 循环即将驱动的那个回合由什么起头。
enum TurnStart<'a> {
    /// 用户打的 prompt：它成为一条 `user` 消息，然后回合跑起来。
    Prompt(&'a str),
    /// 光秃秃的 `/<skill>`：[`Harness::run_skill`] 加载正文，它自己就投影成一条 `user` 消息，所以
    /// 为它凭空造一条 prompt 是不对的。转录绝不能显示用户没打过的字。
    Skill(&'a str),
}

/// 跑一个回合，同时仍然盯着取消手势。
///
/// 回合占着会话，所以循环没法自己读键盘；它改为 select 控制台那些未被请求的事件。在取消已经举起时
/// 再按一次会把进程按下去（spec §6）—— 会话永远不需要知道自己是怎么死的，因为 `--continue` 会关掉
/// 进程留下的任何东西。
async fn run_one_turn(
    harness: &mut Harness,
    events: &mut ConsoleEvents,
    start: TurnStart<'_>,
) -> Result<(), crate::Error> {
    let signal = harness.cancel_signal();
    let modes = harness.mode_cycle();
    let mut turn = Box::pin(async move {
        match start {
            TurnStart::Prompt(input) => harness.run_turn(input).await,
            TurnStart::Skill(name) => harness.run_skill(name).await,
        }
    });
    loop {
        tokio::select! {
            result = &mut turn => return result.map(|_| ()),
            event = events.recv() => match event {
                Some(FrontEndEvent::Cancel) => {
                    if signal.is_cancelled() {
                        std::process::exit(130);
                    }
                    signal.cancel();
                }
                // 输入结束或显式退出让回合像被取消一样落下来，所以流仍然得到它的收尾。
                Some(FrontEndEvent::Quit) | None => signal.cancel(),
                // 权限门每次调用都读策略，所以这次按键挪动的是「下一次调用」的立场 —— 这就是「回合
                // 中途换档」的意思。
                Some(FrontEndEvent::CycleMode) => {
                    modes.cycle();
                }
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
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_probe());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config, env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };

    // 没登记的模型 id 是启动错误，绝不是悄悄降级：探测任何东西之前先查整张表。
    if let Err(message) = validate_models(&config) {
        eprintln!("fs-agent: {message}");
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
        eprintln!("fs-agent: {}", render::wording::probe_no_key());
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    // 库不读任何环境，所以 CLI 把它唯一需要的那件事实交给它 —— `rm` 断路器要用的那个。
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    for model in models {
        match probe_model(&config, &model, home.as_deref()).await {
            Ok(()) => {}
            Err(ProbeError::Skipped(message)) => eprintln!("fs-agent: 跳过 {model}：{message}"),
            Err(ProbeError::Failed(message)) => {
                eprintln!("fs-agent: {model}: {message}");
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
) -> Result<(), ProbeError> {
    let (_, profile) = config
        .resolve_model(Some(model_id))
        .map_err(ProbeError::failed)?;
    let provider = match OpenAiProvider::build(config, model_id, stderr_warnings()) {
        Ok(provider) => provider,
        Err(BuildError::MissingKey { hint, .. }) => {
            return Err(ProbeError::Skipped(format!(
                "没有 API key（请 export {hint}）"
            )))
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
    let mut harness = assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: dir,
            log_path: log_path.clone(),
            session_id: SessionId::new(format!("probe-{model_id}")),
            // 探针要跑真实回合，所以给它真实的工具表 —— 除了 `ask_user_question`：无头没有应答者，
            // 而一个只能失败的工具会白白浪费一次模型调用（spec §19）。
            tools: tools::with_dynamic(&config.tools, false),
            locks: PathLocks::new(),
            // 探针是无头的、没有应答者，所以配置那一档的 `ask`（默认）会拒掉写，而不是挂在一个谁也
            // 看不见的问题上。
            policy: Policy::for_mode(config.mode),
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

fn probe_dir(model_id: &str) -> PathBuf {
    let slug: String = model_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    std::env::temp_dir().join("fs-agent-probe").join(slug)
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
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_prune());
            return ExitCode::FAILURE;
        }
    };
    let cwd = match parsed.cwd {
        Some(path) => path,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_cwd(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        },
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!("fs-agent: {}", render::wording::startup_no_session_store());
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
                    "fs-agent: {}",
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
                "fs-agent: {}",
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
                return Err(render::wording::unknown_argument(other))
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
            let _ = writeln!(err, "fs-agent: {message}");
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
            let _ = writeln!(err, "fs-agent: {}", render::wording::sessions_needs_verb());
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
        other => {
            let _ = writeln!(
                err,
                "fs-agent: {}",
                render::wording::unknown_sessions_verb(other)
            );
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(err, "fs-agent: {message}");
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
        let _ = writeln!(err, "fs-agent: {}", render::wording::no_sessions());
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
    use super::{submission, Mode, Submission};

    /// 这些测试里一场会话知道的技能。
    fn has_skill(name: &str) -> bool {
        matches!(name, "ask-matt" | "review")
    }

    fn read(text: &str) -> Submission<'_> {
        submission(text, has_skill)
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
                task: "帮我看一下".to_owned(),
            }
        );
        assert_eq!(
            read("/ask-matt"),
            Submission::Skill {
                name: "ask-matt",
                task: String::new(),
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
                task: "第一行\n第二行".to_owned(),
            }
        );
        // 任务可以从技能那一行开始，再往下面续。
        assert_eq!(
            read("/ask-matt 第一行\n第二行"),
            Submission::Skill {
                name: "ask-matt",
                task: "第一行\n第二行".to_owned(),
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
    fn a_retired_command_is_now_an_unknown_one() {
        // 在它们所控制的那个模式退场之前，`plan` 与 `endplan` 都是内建命令
        // （`.scratch/todo-and-modes`）。现在没有东西去解析它们了，答案是未知命令那段文本 —— 那段
        // 文本会点名真正存在的内建命令。
        assert_eq!(read("/plan"), Submission::Unknown("/plan"));
        assert_eq!(read("/endplan"), Submission::Unknown("/endplan"));
    }

    #[test]
    fn the_mode_flag_parses_the_three_modes_and_refuses_the_rest() {
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
            ("auto", Mode::Auto),
        ] {
            assert_eq!(args(&["--mode", written]).unwrap().mode, Some(expected));
        }
        assert_eq!(args(&[]).unwrap().mode, None, "不写表示由文件决定");
        let error = args(&["--mode", "plan"]).unwrap_err();
        for word in ["plan", "readonly", "ask", "auto"] {
            assert!(error.contains(word), "`{word}` 没有出现在：{error}");
        }
    }

    #[test]
    fn the_mode_flag_overrides_the_configuration_and_its_absence_does_not() {
        use super::{effective_mode, InteractiveArgs};

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
                task: String::new(),
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

        assert!(parse_discuss_line("--nope x")
            .unwrap_err()
            .contains("--nope"));
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
        assert!(discuss_args(&["--debaters", "只有一个", "问题"])
            .unwrap_err()
            .contains("两个名字"));
    }

    // --- 讨论的参数（spec §15） ----------------------------------------------

    use super::{parse_discuss, question, DiscussArgs};

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
        assert!(discuss_args(&["--plain", "--tui", "q"])
            .unwrap_err()
            .contains("互斥"));
        // 一个不存在的旗标，以及一个缺了值的旗标。
        assert!(discuss_args(&["--model", "kimi-k3"])
            .unwrap_err()
            .contains("--model"));
        assert!(discuss_args(&["--cwd"]).unwrap_err().contains("--cwd"));
        // 没有 `--continue`：一场讨论是一个问题、一个 harness。
        assert!(discuss_args(&["--continue"])
            .unwrap_err()
            .contains("--continue"));
    }

    #[test]
    fn a_question_that_looks_like_a_flag_goes_after_a_double_dash() {
        let parsed = discuss_args(&["--", "--这段以横线开头"]).unwrap();
        assert_eq!(parsed.words, vec!["--这段以横线开头".to_owned()]);
    }
}
