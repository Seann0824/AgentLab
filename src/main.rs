use std::path::PathBuf;

use anyhow;
use clap::Parser;

use std::sync::Arc;

use agent_lab::{
    agent::Agent,
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

    match agent_type {
        AgentType::Orchestrator => run_orchestrator(socket_path).await,
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
async fn run_orchestrator(socket_path: PathBuf) -> anyhow::Result<()> {
    eprintln!("🐝 启动 Orchestrator Agent (socket: {:?})", socket_path);

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

    // 自动启动 Memory Agent 子进程
    spawn_memory_agent(&socket_path);

    // 从 SwarmOrchestrator 获取注册表共享引用，并获取快照
    let registry = {
        let orch = orch_arc.lock().await;
        orch.get_registry_snapshot().await
    };
    eprintln!(
        "🐝 [Orchestrator] 蜂群注册表已就绪: {} 个 Agent 已注册",
        registry.online_count()
    );

    // 启动交互式 Agent（Orchestrator 模式 = 主 Agent）
    let mut agent = Agent::builder()
        .model_manager(model_manager)
        .swarm_registry(registry)
        .swarm_orchestrator(orch_arc.clone())
        .build()?;

    agent.run().await
}

/// 启动 Memory Agent 子进程
fn spawn_memory_agent(orchestrator_socket: &std::path::Path) {
    let socket_str = orchestrator_socket.to_string_lossy().to_string();
    let binary = std::env::current_exe().ok();
    let binary_path = binary
        .as_deref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "agent-lab".to_string());

    eprintln!("🐝 [Orchestrator] 启动 Memory Agent 子进程...");

    match std::process::Command::new(&binary_path)
        .arg("--agent-type")
        .arg("memory")
        .arg("--orchestrator-socket")
        .arg(&socket_str)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(child) => {
            eprintln!(
                "🐝 [Orchestrator] Memory Agent 子进程已启动 (PID: {})",
                child.id()
            );
            // 不等待子进程——它独立运行
            std::mem::drop(child);
        }
        Err(e) => {
            eprintln!("🐝 [Orchestrator] 启动 Memory Agent 失败: {}", e);
        }
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
