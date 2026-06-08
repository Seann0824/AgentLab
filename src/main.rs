use std::path::PathBuf;

use anyhow;
use clap::{Parser, ValueEnum};

use std::sync::Arc;

use agent_lab::{
    agent::{Agent, AgentConfig, OutputMode},
    memory::manager::MemoryManager,
    model::ModelManager,
    swarm::{
        agents::{CoderAgent, GeneralAgent, MemoryAgent, ResearcherAgent, VerifierAgent},
        orchestrator::SwarmOrchestrator,
        registry::AgentType,
        transport::default_socket_path,
    },
};
use tokio::sync::Mutex as TokioMutex;

/// Agent Lab — 自我进化的 AI Agent 框架
#[derive(Parser, Debug)]
#[command(name = "agent-lab", version, about)]
struct Cli {
    /// Agent 类型: orchestrator(默认) | memory | general | verifier | coder | researcher
    #[arg(long, default_value = "orchestrator")]
    agent_type: String,

    /// UDS Socket 路径 (蜂群通信)
    #[arg(long)]
    socket_path: Option<String>,

    /// Orchestrator 的 Socket 路径 (子 Agent 连接用)
    #[arg(long)]
    orchestrator_socket: Option<String>,

    /// 输出模式: concise(默认，隐藏冗长工具输出) | full(完整调试输出) | json(NDJSON 事件)
    #[arg(long, value_enum, default_value_t = CliOutputMode::Concise)]
    output: CliOutputMode,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliOutputMode {
    Concise,
    Full,
    Json,
}

impl From<CliOutputMode> for OutputMode {
    fn from(value: CliOutputMode) -> Self {
        match value {
            CliOutputMode::Concise => OutputMode::Concise,
            CliOutputMode::Full => OutputMode::Full,
            CliOutputMode::Json => OutputMode::Json,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let agent_type = parse_agent_type(&cli.agent_type);
    let socket_path = cli
        .socket_path
        .map(PathBuf::from)
        .unwrap_or_else(default_socket_path);

    let output_mode = OutputMode::from(cli.output);

    match agent_type {
        AgentType::Orchestrator => run_orchestrator(socket_path, output_mode).await,
        AgentType::Memory => run_memory_agent(socket_path, cli.orchestrator_socket).await,
        AgentType::General => run_general_agent(socket_path, cli.orchestrator_socket).await,
        AgentType::Verifier => run_verifier_agent(socket_path, cli.orchestrator_socket).await,
        AgentType::Coder => run_coder_agent(socket_path, cli.orchestrator_socket).await,
        AgentType::Researcher => run_researcher_agent(socket_path, cli.orchestrator_socket).await,
        _ => {
            anyhow::bail!("不支持的 Agent 类型: {}", cli.agent_type);
        }
    }
}

/// 解析 Agent 类型字符串
fn parse_agent_type(s: &str) -> AgentType {
    match s.to_lowercase().as_str() {
        "orchestrator" => AgentType::Orchestrator,
        "memory" => AgentType::Memory,
        "general" => AgentType::General,
        "verifier" => AgentType::Verifier,
        "coder" => AgentType::Coder,
        "researcher" => AgentType::Researcher,
        other => AgentType::Custom(other.to_string()),
    }
}

/// 启动 Orchestrator Agent（默认模式）= 蜂群编排器 + 交互式 Agent
async fn run_orchestrator(socket_path: PathBuf, output_mode: OutputMode) -> anyhow::Result<()> {
    render_startup(output_mode, &socket_path);

    let model_manager = ModelManager::from_env();
    if !model_manager.has_models() {
        anyhow::bail!(
            "未从环境变量发现任何模型配置。请设置 <PREFIX>_API_KEY 和 <PREFIX>_BASE_URL 环境变量。"
        );
    }

    // 创建 SwarmOrchestrator（绑定 UDS Server + 启动心跳监控）
    let orchestrator = SwarmOrchestrator::bind(Some(socket_path.clone())).await?;
    let orch_arc = Arc::new(TokioMutex::new(orchestrator));

    // 启动后台接受循环（接收子 Agent 连接）
    let _accept_handle = SwarmOrchestrator::start_accept_loop(orch_arc.clone());

    // 自动启动核心子 Agent 子进程，让 Orchestrator 默认具备可委派对象
    spawn_core_agents(&socket_path, output_mode);
    wait_for_core_agents(&orch_arc, output_mode).await;

    // 从 SwarmOrchestrator 获取注册表共享引用，并获取快照
    let registry = {
        let orch = orch_arc.lock().await;
        orch.get_registry_snapshot().await
    };
    if output_mode.is_full() {
        eprintln!(
            "🐝 [Orchestrator] 蜂群注册表已就绪: {} 个 Agent 已注册",
            registry.online_count()
        );
    }

    // 启动交互式 Agent（Orchestrator 模式 = 主 Agent）
    let mut agent = Agent::builder()
        .model_manager(model_manager)
        .config(AgentConfig {
            output_mode,
            ..AgentConfig::default()
        })
        .swarm_registry(registry)
        .swarm_orchestrator(orch_arc.clone())
        .build()?;

    agent.run().await
}

fn render_startup(output_mode: OutputMode, socket_path: &std::path::Path) {
    match output_mode {
        OutputMode::Concise => {
            println!(
                "\x1b[36mAgent Lab\x1b[0m \x1b[90mready · /help 查看命令 · --output full 查看详细执行日志\x1b[0m"
            );
        }
        OutputMode::Full => {
            eprintln!("🐝 启动 Orchestrator Agent (socket: {:?})", socket_path);
        }
        OutputMode::Json => {
            print_cli_event(
                agent_lab::agent::events::RunEventKind::AgentNotice,
                "orchestrator",
                serde_json::json!({
                    "message": "orchestrator_started",
                    "socket_path": socket_path.display().to_string()
                }),
            );
        }
    }
}

fn render_notice(output_mode: OutputMode, subject: &str, message: String) {
    if output_mode.is_json() {
        print_cli_event(
            agent_lab::agent::events::RunEventKind::AgentNotice,
            subject,
            serde_json::json!({ "message": message }),
        );
    } else {
        eprintln!("\x1b[90m{}\x1b[0m", message);
    }
}

fn print_cli_event(
    kind: agent_lab::agent::events::RunEventKind,
    subject: impl Into<String>,
    attributes: serde_json::Value,
) {
    let event = agent_lab::agent::events::RunEvent::new(kind, subject, attributes);
    if let Ok(line) = serde_json::to_string(&event) {
        println!("{}", line);
    }
}

/// 启动核心子 Agent 子进程
fn spawn_core_agents(orchestrator_socket: &std::path::Path, output_mode: OutputMode) {
    for agent_type in ["memory", "general", "verifier", "coder", "researcher"] {
        spawn_agent_process(orchestrator_socket, agent_type, output_mode);
    }
}

fn spawn_agent_process(
    orchestrator_socket: &std::path::Path,
    agent_type: &str,
    output_mode: OutputMode,
) {
    let socket_str = orchestrator_socket.to_string_lossy().to_string();
    let binary = std::env::current_exe().ok();
    let binary_path = binary
        .as_deref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "agent-lab".to_string());

    if output_mode.is_full() {
        eprintln!("🐝 [Orchestrator] 启动 {} Agent 子进程...", agent_type);
    }

    let mut command = std::process::Command::new(&binary_path);
    command
        .arg("--agent-type")
        .arg(agent_type)
        .arg("--orchestrator-socket")
        .arg(&socket_str)
        .arg("--output")
        .arg(output_mode.as_str());

    if output_mode.is_full() {
        command
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
    } else {
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }

    match command.spawn() {
        Ok(child) => {
            if output_mode.is_full() {
                eprintln!(
                    "🐝 [Orchestrator] {} Agent 子进程已启动 (PID: {})",
                    agent_type,
                    child.id()
                );
            }
            // 不等待子进程——它独立运行
            std::mem::drop(child);
        }
        Err(e) => {
            render_notice(
                output_mode,
                "orchestrator",
                format!("启动 {} Agent 失败: {}", agent_type, e),
            );
        }
    }
}

async fn wait_for_core_agents(
    orch_arc: &Arc<TokioMutex<SwarmOrchestrator>>,
    output_mode: OutputMode,
) {
    let required = [
        AgentType::Memory,
        AgentType::General,
        AgentType::Verifier,
        AgentType::Coder,
        AgentType::Researcher,
    ];

    for _ in 0..20 {
        let registry = {
            let orch = orch_arc.lock().await;
            orch.get_registry_snapshot().await
        };
        let ready = required
            .iter()
            .filter(|agent_type| !registry.query_by_type(agent_type).is_empty())
            .count();
        if ready == required.len() {
            if output_mode.is_full() {
                eprintln!("🐝 [Orchestrator] 核心子 Agent 已全部注册");
            }
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let registry = {
        let orch = orch_arc.lock().await;
        orch.get_registry_snapshot().await
    };
    let missing: Vec<&str> = required
        .iter()
        .filter(|agent_type| registry.query_by_type(agent_type).is_empty())
        .map(|agent_type| agent_type.as_str())
        .collect();
    if !missing.is_empty() {
        render_notice(
            output_mode,
            "orchestrator",
            format!("部分核心子 Agent 尚未注册: {}", missing.join(", ")),
        );
    }
}

/// 启动 Memory Agent（非交互式，通过 UDS 通信）
async fn run_memory_agent(
    socket_path: PathBuf,
    orchestrator_socket: Option<String>,
) -> anyhow::Result<()> {
    eprintln!("🧠 启动 Memory Agent (socket: {:?})", socket_path);

    // 初始化 MemoryManager
    let store_dir = PathBuf::from("/tmp/agent-lab/memory");
    tokio::fs::create_dir_all(&store_dir).await.ok();
    let memory_manager = MemoryManager::new_mock(store_dir);

    // 创建并连接 Memory Agent
    let mut memory_agent = MemoryAgent::new(memory_manager);
    memory_agent
        .connect(orchestrator_socket.map(PathBuf::from))
        .await?;
    memory_agent.run().await
}

/// 启动 General Agent（非交互式，通过 UDS 通信）
async fn run_general_agent(
    _socket_path: PathBuf,
    orchestrator_socket: Option<String>,
) -> anyhow::Result<()> {
    eprintln!("🔧 启动 General Agent");

    let mut general_agent = GeneralAgent::new();
    general_agent
        .connect(orchestrator_socket.map(PathBuf::from))
        .await?;
    general_agent.run().await
}

/// 启动 Verifier Agent（非交互式，通过 UDS 通信）
async fn run_verifier_agent(
    _socket_path: PathBuf,
    orchestrator_socket: Option<String>,
) -> anyhow::Result<()> {
    eprintln!("✅ 启动 Verifier Agent");

    let project_path = PathBuf::from(".");
    let mut verifier_agent = VerifierAgent::new(Some(project_path));
    verifier_agent
        .connect(orchestrator_socket.map(PathBuf::from))
        .await?;
    verifier_agent.run().await
}

/// 启动 Code Agent（非交互式，通过 UDS 通信）
async fn run_coder_agent(
    _socket_path: PathBuf,
    orchestrator_socket: Option<String>,
) -> anyhow::Result<()> {
    eprintln!("💻 启动 Code Agent");

    let project_path = PathBuf::from(".");
    let mut coder_agent = CoderAgent::new(Some(project_path));
    coder_agent
        .connect(orchestrator_socket.map(PathBuf::from))
        .await?;
    coder_agent.run().await
}

/// 启动 Researcher Agent（非交互式，通过 UDS 通信）
async fn run_researcher_agent(
    _socket_path: PathBuf,
    orchestrator_socket: Option<String>,
) -> anyhow::Result<()> {
    eprintln!("🔬 启动 Researcher Agent");

    let project_path = PathBuf::from(".");
    let mut researcher_agent = ResearcherAgent::new(Some(project_path));
    researcher_agent
        .connect(orchestrator_socket.map(PathBuf::from))
        .await?;
    researcher_agent.run().await
}
