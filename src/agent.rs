// src/agent.rs
// Agent 核心 — 持有所有状态，运行主循环，支持多 Agent
//
// 设计文档: docs/designs/MULTI_AGENT_ARCHITECTURE.md

use anyhow;
use futures_util::{StreamExt, stream::FuturesUnordered};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cli::CommandRegistry;
use crate::context::{ContextManager, ContextStrategy, TokenEstimator};
use crate::model::{ChatMessage, ModelEvent, ToolCall};
use crate::model::{ModelManager, ModelConfig};
use crate::investigate::ErrorSnapshotManager;
use crate::session::SessionManager;
use crate::goal::GoalRegistry;
use crate::task::TaskManager;
use crate::memory::{MemoryManager, MemorySource};
use crate::tools::{ToolManager, hello_world::HelloWorld, shell::BashShell, tool_debug::DebugTool, edit::EditTool, read::ReadTool, search::SearchTool, subagent::SpawnAgent, investigate::InvestigateTool, generate_tool::GenerateTool};
use crate::tools::memory_tools::{MemorySaveTool, MemorySearchTool, MemoryForgetTool, MemoryStatsTool};

// =====================================================================
// AgentConfig — Agent 配置
// =====================================================================

/// Agent 配置：上下文策略和运行参数
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// 上下文 Token 上限
    pub token_limit: usize,
    /// 最大轮次（触发滑动窗口的轮数）
    pub max_turns: usize,
    /// 压缩触发比例（0.0 ~ 1.0）
    pub trigger_ratio: f64,
    /// 是否启用异步摘要
    pub enable_async_summary: bool,
    /// 是否启用工具调用修剪
    pub enable_tool_pruning: bool,
    /// 保留最近工具调用数
    pub tool_pruning_keep_recent: usize,
    /// 工具输出最大字符数（超过的被截断）
    pub tool_pruning_max_output_chars: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: true,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        }
    }
}

impl AgentConfig {
    /// 将配置转换为 ContextStrategy
    pub fn to_strategy(&self) -> ContextStrategy {
        ContextStrategy::Auto {
            token_limit: self.token_limit,
            max_turns: self.max_turns,
            trigger_ratio: self.trigger_ratio,
            enable_async_summary: self.enable_async_summary,
            enable_tool_pruning: self.enable_tool_pruning,
            tool_pruning_keep_recent: self.tool_pruning_keep_recent,
            tool_pruning_max_output_chars: self.tool_pruning_max_output_chars,
        }
    }
}

// =====================================================================
// AgentHandle — 多 Agent 运行句柄
// =====================================================================

/// AgentHandle — 多 Agent 运行的句柄，通过 tokio::task::spawn 管理
pub struct AgentHandle {
    /// Agent 名称
    pub name: String,
    /// tokio 任务句柄
    pub task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl AgentHandle {
    /// 获取 Agent 名称
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 等待 Agent 完成
    pub async fn join(self) -> anyhow::Result<()> {
        self.task.await.map_err(|e| anyhow::anyhow!("Agent '{}' panicked: {}", self.name, e))?
    }
}

// =====================================================================
// AgentBuilder — Agent 构建器（链式调用）
// =====================================================================

/// AgentBuilder — 链式构建 Agent
pub struct AgentBuilder {
    model_manager: Option<ModelManager>,
    tool_manager: Option<ToolManager>,
    config: AgentConfig,
    current_dir: String,
    memory_manager: Option<MemoryManager>,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self {
            model_manager: None,
            tool_manager: None,
            config: AgentConfig::default(),
            current_dir: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .display()
                .to_string(),
            memory_manager: None,
        }
    }
}

impl AgentBuilder {
    /// 创建新的 AgentBuilder
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置模型管理器（支持多模型注册与切换）
    pub fn model_manager(mut self, mm: ModelManager) -> Self {
        self.model_manager = Some(mm);
        self
    }

    /// 设置工具管理器
    pub fn tool_manager(mut self, tm: ToolManager) -> Self {
        self.tool_manager = Some(tm);
        self
    }

    /// 设置 Agent 配置
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// 设置当前工作目录
    pub fn current_dir(mut self, dir: impl Into<String>) -> Self {
        self.current_dir = dir.into();
        self
    }

    /// 设置 MemoryManager（持久化记忆）
    pub fn memory_manager(mut self, mm: MemoryManager) -> Self {
        self.memory_manager = Some(mm);
        self
    }

    /// 构建 Agent
    pub fn build(self) -> anyhow::Result<Agent> {
        let model_manager = self.model_manager.ok_or_else(|| anyhow::anyhow!("ModelManager is required. Call .model_manager(mm) to set it."))?;

        // ⭐ 初始化 MemoryManager（持久化记忆）
        let memory_manager = self.memory_manager.unwrap_or_else(|| MemoryManager::new_mock(std::path::PathBuf::from(&self.current_dir)));
        let memory_manager = Arc::new(Mutex::new(memory_manager));

        // ⭐ 构建工具管理器（注册 memory 工具）
        let mut tool_manager = self.tool_manager.unwrap_or_else(default_tool_manager);
        tool_manager.register_tool(Box::new(MemorySaveTool { memory_manager: memory_manager.clone() }));
        tool_manager.register_tool(Box::new(MemorySearchTool { memory_manager: memory_manager.clone() }));
        tool_manager.register_tool(Box::new(MemoryForgetTool { memory_manager: memory_manager.clone() }));
        tool_manager.register_tool(Box::new(MemoryStatsTool { memory_manager: memory_manager.clone() }));

        let strategy = self.config.to_strategy();

        // ⭐ 初始化 GoalRegistry
        let mut goal_manager = GoalRegistry::new(&self.current_dir);
        let _ = goal_manager.load_all();

        Ok(Agent {
            config: self.config,
            model_manager,
            tool_manager,
            context_manager: ContextManager::new("".to_string(), strategy),
            goal_manager,
            task_manager: TaskManager::new(&self.current_dir),
            session_manager: SessionManager::new(&self.current_dir, &self.current_dir),
            command_registry: CommandRegistry::new(),
            current_dir: self.current_dir,
            memory_manager,
        })
    }
}

// =====================================================================
// Agent — 核心结构体
// =====================================================================

/// Agent — 持有所有状态，运行主循环
pub struct Agent {
    config: AgentConfig,
    /// 模型管理器（支持多模型注册与动态切换）
    model_manager: ModelManager,
    tool_manager: ToolManager,
    context_manager: ContextManager,
    memory_manager: Arc<Mutex<MemoryManager>>,
    goal_manager: GoalRegistry,
    task_manager: TaskManager,
    session_manager: SessionManager,
    command_registry: CommandRegistry,
    current_dir: String,
}

impl Agent {
    /// 创建 AgentBuilder
    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }


    /// 将 Agent 派发到 tokio 任务中运行
    ///
    /// 返回 AgentHandle，可用于等待 Agent 完成
    pub fn spawn(name: impl Into<String>, mut agent: Agent) -> AgentHandle {
        let name = name.into();
        let task = tokio::task::spawn(async move {
            agent.run().await
        });
        AgentHandle { name, task }
    }

    // ==================== 主循环 ====================

    /// 运行 Agent 主循环
    ///
    /// 这是 main.rs 中原主循环的提取版，包含：
    /// - CLI 交互（--task 模式和 stdin 模式）
    /// - / 命令处理（/help, /clear, /session, /tools, /debug）
    /// - 上下文管理与压缩
    /// - LLM 流式对话 + 工具调用
    /// - 任务状态管理
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // ⭐ 获取配置中的 token_limit
        let token_limit = self.config.token_limit;

        // ⭐ 解析 CLI 参数：--task 用于子 agent 单次运行模式
        let args: Vec<String> = std::env::args().collect();
        let single_task = if args.len() > 1 && args[1] == "--task" {
            if args.len() > 2 {
                Some(args[2..].join(" "))
            } else {
                eprintln!("⚠️  --task 参数需要提供任务描述");
                None
            }
        } else {
            None
        };

        let policy_summary = String::new(); // 权限摘要（后续可从配置加载）

        // ⭐ 动态生成工具列表描述
        let tools_description = generate_tools_description(&self.tool_manager);

        // ⭐ 构建系统提示词
        let system_prompt = format!(
            r#"你当前工作的目录为 {current_dir}。这个目录是你模型的Agent架子，它构建你和外部世界沟通的 bridge。如果你需要什么能力自己修改agent代码补充。

{policy_summary}

【上下文管理说明】
- 为了管理上下文窗口，早期对话历史可能会被自动压缩为摘要。
- 摘要会按「目标 → 操作 → 决策 → 状态」的结构保留关键信息。
- 如果发现某些上下文缺失，请基于摘要信息继续工作。
- 重要的上下文信息请**写入文件**，而不是仅依赖对话历史。
- 系统状态信息（如 Token 使用率）会输出到 stderr，不会混入你的工具执行结果。

【工作原则】
- 读取文件内容后，关键信息应记录在文件中，不要仅依赖对话记忆。
- 如果需要在多轮对话中保持状态，请使用文件持久化。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【结构化工作流程】

当你接收到需要多步执行的复杂任务时，请遵循以下流程：

1. 【🧠 规划阶段】先输出分析，然后创建 PLAN.md 文件：
   - 目标描述
   - 执行步骤（编号列表，每步一个 checkbox：- [ ]）
   - 每个步骤的验证标准
   - 将当前步骤写入 AGENDA.md

2. 【🔧 执行阶段】按 PLAN.md 的步骤逐个执行：
   - 每完成一步，更新 PLAN.md 标记为 - [x]
   - 更新 AGENDA.md 反映最新进度
   - 遇到错误时，先分析原因再修复
   - 重要发现记录到 MEMORY.md

3. 【✅ 验证阶段】每次代码修改后必须验证：
   - 修改 Rust 代码后 → 运行 `cargo check 2>&1 | tail -30`
   - 修改配置文件后 → 检查语法完整性
   - 验证失败时：分析错误 → 修复 → 再次验证
   - 如果连续 3 次修复失败，重新规划方案

4. 【📝 总结阶段】所有步骤完成后向用户总结：
   - 完成了什么
   - 关键决策
   - 当前项目状态

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【记忆与状态管理】

项目根目录下维护以下状态文件：

📄 PLAN.md   — 当前执行计划（步骤列表 + 完成状态）
📄 AGENDA.md  — 当前议程精简版（任务名 + 进度 + 当前步骤）
📄 MEMORY.md  — 重要发现、关键决策、已知问题

规则：
- 每次开启新任务时，检查并读取这些文件恢复上下文
- 上下文被压缩后，通过读取这些文件重新理解当前状态
- 不要在对话中重复记录已写入文件的信息

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【自我进化】

你可以修改自身代码来增强能力：
1. 添加新工具 → 实现 Tool trait，注册到 ToolManager
2. 修改现有工具 → 增强已有工具的能力
3. 修复 bug → 修改对应 src/ 下的代码

规则：
- 修改代码后必须运行 cargo check 验证
- 验证通过后，新能力立即生效（下次调用时）
- 如果修改导致编译失败，分析错误并修复
- 重大改动建议分步进行（先加框架，再填实现）

当前可用工具：
{tools_description}

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【主从 Agent 架构】

你是 **Master Agent（主 Agent）**，可以通过 `spawn_agent` 工具派遣 **Sub-agent（子 Agent）** 为你工作。

### 架构模型
```
你 (Master Agent, 当前进程)
  └── spawn_agent(task="...")  →  编译并启动 Sub-agent (独立子进程)
        └── Sub-agent 独立完成任务后退出，结果返回给你
```

### 工作原理
1. **你（Master）** 决定需要做什么任务
2. 调用 `spawn_agent(task="任务描述", timeout_seconds=300)`
3. 系统自动 `cargo build` 编译当前代码，然后以 `--task` 模式启动子进程
4. **Sub-agent** 作为一个全新的 Agent 实例独立执行任务（拥有自己的上下文、工具）
5. Sub-agent 完成后返回完整输出，你分析结果并继续工作

### 适合派遣 Sub-agent 的场景
- **并行探索**：需要同时调查多个方向时，可以多次调用 spawn_agent 并行派遣多个子 agent
- **独立验证**：修改代码后派生子 agent 做端到端测试验证
- **分治执行**：复杂任务拆解后，将子任务分派给子 agent 并行处理
- **安全隔离**：需要在不影响当前上下文的环境中执行的实验性操作

### 注意
- Sub-agent 是你的副本，拥有和你一样的能力（工具、模型）
- Sub-agent 在独立进程中运行，它的输出不会污染你的上下文
- 一次可以派遣多个 Sub-agent 并行工作
- Sub-agent 完成任务后自动退出

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【🎯 目标驱动模式】

系统支持 Goal-Driven（目标驱动）模式，你可以遵循以下工作方式：

### 目标设定
- 用户通过 `/goal set <描述>` 设置目标后，系统会自动激活目标并持久化存储
- 目标激活后，每次压缩都会注入当前目标状态到你的上下文中

### 目标推进
- 你应当主动分解目标、制定步骤、逐步推进
- 重要决策和进度应记录到文件（如 PLAN.md / AGENDA.md / MEMORY.md）
- 每完成一步，可以通过 `edit` 工具更新 PLAN.md 记录进度

### 自评估与完成
当你认为目标已完成或无法完成时，在回复中输出以下命令来更新目标状态：
- 所有步骤完成且满足标准 → 输出 `/goal complete <目标ID>`
- 目标无法完成 → 输出 `/goal fail <目标ID> <原因>`
- 用户要求取消 → 输出 `/goal cancel <目标ID>`

### 查看目标
你也可以使用 `/goal list` 列出所有目标，或 `/goal status` 查看当前活跃目标。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【任务状态管理】

系统内置了 TaskManager，它会：
1. 自动维护 PLAN.md / docs/AGENDA.md / docs/MEMORY.md 中的任务状态
2. 当上下文被压缩后，自动将当前任务状态注入到你的上下文中
3. 你可以通过 edit 工具直接编辑这些文件来更新任务状态

关键文件：
- 📄 PLAN.md     — 当前执行计划（步骤列表 + 完成状态）
- 📄 AGENDA.md   — 当前议程（任务名 + 进度 + 当前步骤）
- 📄 MEMORY.md   — 重要发现、关键决策、已知问题

规则：
- 当你开始一个新任务时，先规划步骤写入 PLAN.md
- 每完成一步，更新 PLAN.md 和 AGENDA.md
- 重要发现写入 MEMORY.md
- 上下文被压缩后，检查注入的任务状态恢复上下文"#,
            tools_description = tools_description,
            current_dir = self.current_dir,
            policy_summary = policy_summary,
        );

        // ⭐ 初始化 ContextManager（用真正的系统提示词）
        self.context_manager = ContextManager::new(system_prompt, self.config.to_strategy());

        // ⭐ 启动异步摘要后台任务
        self.context_manager.setup_summary_channel(Some(
            self.model_manager.clone_active_adapter()
                .expect("当前模型适配器不可用"),
        ));

        // ⭐ 初始化任务管理器
        self.task_manager = TaskManager::new(&self.current_dir);

        // ⭐ 如果有活跃 Goal，启动时注入目标状态
        if self.goal_manager.has_active_goal() {
            if let Some(goal_msg) = self.goal_manager.get_inject_message() {
                self.context_manager.add_message(goal_msg);
                eprintln!("🎯 已发现活跃目标，目标状态已注入上下文");
            }
        }
        self.task_manager.load();

        let mut is_auto = false;
        let mut terminal_line_dirty = false;
        let mut single_task_used = false;

        // ⭐ 自动提取记忆计数器（每 N 轮非工具调用的对话保存重要信息到记忆）
        let mut auto_extract_counter: usize = 0;
        const AUTO_EXTRACT_INTERVAL: usize = 3;
        loop {
            if !is_auto {
                // ⭐ --task 模式：使用 CLI 参数作为首次输入
                if let Some(ref task) = single_task {
                    if !single_task_used {
                        single_task_used = true;
                        let input = task.clone();
                        eprintln!("[子 agent] 执行任务: {}", &input);
                        self.context_manager.add_message(ChatMessage::user(&input));
                        self.task_manager.on_user_input(&input);
                    } else {
                        // --task 模式：任务已完成，输出最终错误快照引用并退出
                        // 输出最后的错误快照 ID（如果存在）
                        let snapshot_manager = ErrorSnapshotManager::new(&self.current_dir);
                        if let Ok(snapshots) = snapshot_manager.list() {
                            if let Some(last) = snapshots.last() {
                                eprintln!("[SNAPSHOT] {}", last.id);
                            }
                        }
                        break Ok(());
                    }
                } else {
                    // 正常交互模式：从 stdin 读取
                    let mut user_input = String::new();
                    finish_terminal_line(&mut terminal_line_dirty);
                    print!(">");
                    std::io::Write::flush(&mut std::io::stdout())?;
                    if std::io::stdin().read_line(&mut user_input).is_err() {
                        continue;
                    }
                    if user_input.trim().is_empty() {
                        continue;
                    }
                    let input_str = user_input.trim().to_string();

                    // ⭐ 处理斜杠命令
                    if input_str.starts_with('/') {
                        let trimmed = input_str.trim();
                        let cmd_name = trimmed
                            .trim_start_matches('/')
                            .split_whitespace()
                            .next()
                            .unwrap_or("");

                        // 输入仅为 "/" 时显示可用命令
                        if trimmed == "/" {
                            self.command_registry.print_help_short();
                            continue;
                        }

                        // /help 命令
                        if cmd_name == "help" || cmd_name == "h" || cmd_name == "?" {
                            self.command_registry.print_help_full();
                            continue;
                        }

                        // /clear 命令
                        if cmd_name == "clear" {
                            self.context_manager.clear();
                            println!("\x1b[32m━━━ 🧹 历史消息已清空 ━━━\x1b[0m");
                            continue;
                        }

                        // /session 和 /sessions 命令
                        if cmd_name == "session" || cmd_name == "sessions" {
                            handle_session_command(&trimmed, &self.session_manager, &mut self.context_manager, &mut self.task_manager);
                            continue;
                        }

                        // /tools 命令：列出所有可用工具
                        if cmd_name == "tools" {
                            let tools = self.tool_manager.list_tools();
                            println!("\x1b[36m━━━ 🔧 可用工具 (共 {}) ━━━\x1b[0m", tools.len());
                            for t in &tools {
                                println!("  \x1b[33m{:<15}\x1b[0m {}", t.name, t.description);
                            }
                            println!("\x1b[90m  💡 工具详情由 LLM function calling schema 自动提供\x1b[0m");
                            continue;
                        }

                        // /debug 命令：控制全局 debug 模式
                        if cmd_name == "debug" {
                            let parts: Vec<&str> = trimmed.split_whitespace().collect();
                            let sub = parts.get(1).copied().unwrap_or("status");
                            match sub {
                                "on" | "enable" | "1" | "true" => {
                                    crate::debug::enable();
                                    println!("\x1b[32m━━━ 🐛 debug 模式已开启 ━━━\x1b[0m");
                                }
                                "off" | "disable" | "0" | "false" => {
                                    crate::debug::disable();
                                    println!("\x1b[33m━━━ 🐛 debug 模式已关闭 ━━━\x1b[0m");
                                }
                                "toggle" | "t" => {
                                    let new_state = crate::debug::toggle();
                                    if new_state {
                                        println!("\x1b[32m━━━ 🐛 debug 模式已切换为开启 ━━━\x1b[0m");
                                    } else {
                                        println!("\x1b[33m━━━ 🐛 debug 模式已切换为关闭 ━━━\x1b[0m");
                                    }
                                }
                                _ => {
                                    println!("\x1b[36m━━━ 🐛 Debug 状态 ━━━\x1b[0m");
                                    println!("  {}", crate::debug::status_text());
                                    println!("\x1b[90m  用法: /debug on|off|toggle|status\x1b[0m");
                                }
                            }
                            continue;
                        }

                        // /model 命令：模型管理与切换
                        if cmd_name == "model" {
                            handle_model_command(&trimmed, &mut self.model_manager);
                            continue;
                        }

                        // /goal 命令：目标管理
                        if cmd_name == "goal" {
                            handle_goal_command(&trimmed, &mut self.goal_manager);

                            // ⭐ 注入活跃目标状态到上下文，让 AI 能感知目标
                            if self.goal_manager.has_active_goal() {
                                if let Some(goal_msg) = self.goal_manager.get_inject_message() {
                                    self.context_manager.add_message(goal_msg);
                                    eprintln!("\r\x1b[2K🎯 目标已注入上下文，AI 将感知到当前目标");
                                }
                            }

                            continue;
                        }

                        if self.command_registry.is_known(cmd_name) {
                            if let Some(cmd) = self.command_registry.get(cmd_name) {
                                self.command_registry.print_command_help(cmd);
                            }
                            continue;
                        } else {
                            self.command_registry.print_unknown_command(&trimmed);
                            continue;
                        }
                    }

                    self.context_manager.add_message(ChatMessage::user(&input_str));
                    self.task_manager.on_user_input(&input_str);
                }
            }

            // ⭐ 检查是否有异步摘要结果需要注入
            let injected = self.context_manager.poll_summary_results();
            let compressed = injected > 0;
            if injected > 0 {
                eprintln!("\r\x1b[2K📋 异步摘要已生成并注入上下文 ({} 条)", injected);
            }

            // ⭐ 如果发生了压缩，注入当前任务状态
            if compressed || self.context_manager.stats().compressed {
                if let Some(task_msg) = self.task_manager.get_inject_message() {
                    self.context_manager.add_message(task_msg);
                    eprintln!("\r\x1b[2K📋 已注入当前任务状态（帮助模型恢复上下文）");
                }
            }

            // ⭐ 如果发生了压缩，注入活跃目标状态
            if compressed || self.context_manager.stats().compressed {
                if let Some(goal_msg) = self.goal_manager.get_inject_message() {
                    self.context_manager.add_message(goal_msg);
                    eprintln!("\r\x1b[2K🎯 已注入当前活跃目标状态（帮助模型持续朝着目标推进）");
                }
            }

            // ⭐ 如果发生了压缩，注入持久化记忆（与当前上下文最相关的记忆）
            if compressed || self.context_manager.stats().compressed {
                // 从最近几条消息提取关键词，搜索相关记忆
                let recent_messages: Vec<String> = self.context_manager.get_messages()
                    .iter()
                    .rev()
                    .take(6)
                    .filter_map(|m| match m {
                        ChatMessage::User { content, .. } => Some(content.clone()),
                        ChatMessage::Assistant { content, .. } if !content.is_empty() => Some(content.clone()),
                        _ => None,
                    })
                    .collect();
                let query = recent_messages.join(" ");
                if !query.is_empty() {
                    match self.memory_manager.lock().await.search_similar(&query, 3).await {
                        Ok(results) if !results.is_empty() => {
                            let mut mem_text = String::from("📌 【持久化记忆 — 检索结果】\n以下是与当前上下文相关的历史记忆：\n");
                            for (i, mem) in results.iter().enumerate() {
                                mem_text.push_str(&format!("{}. [相关性:{:.1}%] {}\n", i + 1, mem.score * 100.0, mem.record.content));
                            }
                            self.context_manager.add_message(ChatMessage::user(&mem_text));
                            eprintln!("\r\x1b[2K🧠 已注入 {} 条相关持久化记忆（帮助模型恢复上下文）", results.len());
                        }
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("\r\x1b[2K⚠️ 记忆检索失败: {}", e);
                        }
                    }
                }
            }

            // ⭐ 显示当前的 Token 使用状态
            let stats = self.context_manager.stats().clone();
            if stats.usage_ratio > 0.3 {
                eprint!(
                    "\r\x1b[2K[Token: {}/{} ({:.0}%) | 保留 {} 条重要消息] ",
                    TokenEstimator::format_tokens(stats.estimated_tokens),
                    TokenEstimator::format_tokens(token_limit),
                    stats.usage_ratio * 100.0,
                    stats.preserved_count,
                );
            }

            // ⭐ 检查上下文是否阻塞
            if self.context_manager.is_blocked() {
                eprintln!(
                    "\r\x1b[2K⚠️  上下文使用率 {:.0}%，触发强制压缩...",
                    self.context_manager.stats().usage_ratio * 100.0,
                );
                let result = self.context_manager.force_compress();
                eprintln!(
                    "\r\x1b[2K✅ 强制压缩完成: {} (tokens: {:.0}%)",
                    result.description(),
                    self.context_manager.stats().usage_ratio * 100.0,
                );
            } else if self.context_manager.is_critical() {
                let _ = self.context_manager.prune_tool_calls();
            }

            let current_adapter = self.model_manager.current_adapter()
                .expect("当前没有可用的模型适配器");
            let mut stream_chat = current_adapter.stream_chat(
                &self.context_manager.get_messages(),
                self.tool_manager.get_tools_scehma(),
            );
            let mut tool_tasks = FuturesUnordered::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut final_assistant_message = String::new();
            let mut has_tool_calls = false;

            while let Some(model_event) = stream_chat.next().await {
                match model_event {
                    ModelEvent::Text(content) => {
                        print!("{}", content);
                        terminal_line_dirty = !content.ends_with('\n');
                    }
                    ModelEvent::Thinking(content) => {
                        print!("\x1b[90m{}\x1b[0m", content);
                        terminal_line_dirty = !content.ends_with('\n');
                    }
                    ModelEvent::ToolCallBlock {
                        id,
                        name,
                        arguments,
                    } => {
                        finish_terminal_line(&mut terminal_line_dirty);

                        // ===== Tool call visualization =====
                        println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
                        if let Ok(args) = serde_json::from_str::<serde_json::Value>(&arguments) {
                            if name == "shell" {
                                if let Some(cmd) = args["command"].as_str() {
                                    println!("\x1b[33m  $ {}\x1b[0m", cmd);
                                }
                            } else {
                                println!(
                                    "\x1b[33m  {}\x1b[0m",
                                    serde_json::to_string_pretty(&args).unwrap_or_default()
                                );
                            }
                        }
                        print!("\x1b[33m⏳ 正在执行...\x1b[0m");
                        std::io::Write::flush(&mut std::io::stdout())?;
                        // ===================================

                        has_tool_calls = true;
                        let tool_call = ToolCall {
                            id,
                            name,
                            arguments,
                        };
                        tool_calls.push(tool_call.clone());
                        tool_tasks.push(self.tool_manager.run(tool_call));
                    }
                    ModelEvent::Done(assistant_message) => {
                        final_assistant_message = assistant_message;
                    }
                    ModelEvent::Error(err) => {
                        eprintln!("\r\x1b[2K\x1b[31m❌ 模型 API 错误: {}\x1b[0m", err);
                    }
                }
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            finish_terminal_line(&mut terminal_line_dirty);

            let tool_call_ids = tool_calls
                .iter()
                .map(|tool_call| tool_call.id.clone())
                .collect::<Vec<_>>();

            // ⭐ 活跃 Goal 停滞检查（连续无进展轮次超过阈值）
            if self.goal_manager.has_active_goal() {
                if let Some(goal) = self.goal_manager.active_goal_mut() {
                    let stalled = goal.is_stalled();
                    let goal_id = goal.id.clone();
                    let goal_clone = goal.clone();
                    if stalled {
                        eprintln!("\r\x1b[2K⚠️  目标 '{}' 已停滞（连续 {} 轮无进展），自动标记为失败", goal_id, goal.stall_count);
                        let _ = self.goal_manager.mark_failed(&goal_id, "stalled");
                    } else {
                        let _ = self.goal_manager.update(goal_clone);
                    }
                }
            }

            // ⭐ 检测 LLM 输出中的 Goal 完成信号
            if self.goal_manager.has_active_goal() && !final_assistant_message.is_empty() {
                if let Some((action, goal_id, reason)) = extract_goal_signal(&final_assistant_message) {
                    match action.as_str() {
                        "complete" => {
                            let _ = self.goal_manager.mark_complete(&goal_id);
                            println!("\n\x1b[32m━━━ ✅ LLM 自动标记目标 '{}' 为已完成 ━━━\x1b[0m 🎉", goal_id);
                        }
                        "fail" => {
                            let _ = self.goal_manager.mark_failed(&goal_id, &reason);
                            println!("\n\x1b[31m━━━ ❌ LLM 自动标记目标 '{}' 为失败 ━━━\x1b[0m", goal_id);
                        }
                        "cancel" => {
                            let _ = self.goal_manager.mark_cancelled(&goal_id);
                            println!("\n\x1b[33m━━━ 🚫 LLM 自动取消目标 '{}' ━━━\x1b[0m", goal_id);
                        }
                        _ => {}
                    }
                }
            }

            // ⭐ Goal 状态日志（实际自动循环控制在下方的 is_auto 设定后处理）
            if let Some(active_goal) = self.goal_manager.active_goal() {
                if active_goal.status.is_terminal() {
                    eprintln!("\r\x1b[2K🎯 目标 '{}' 已达终止状态 ({}), 停止自动执行", active_goal.id, active_goal.status);
                }
            }

            let mut tool_results = Vec::new();
            while let Some(tool_result) = tool_tasks.next().await {
                tool_results.push(tool_result);
            }

            // Clear loading line and render tool results
            if has_tool_calls {
                print!("\r\x1b[K");
                for tool_result in &tool_results {
                    render_tool_result_from_msg(tool_result);
                }
            }

            // ⭐ 自动捕获错误快照（在移动 tool_calls 之前）
            let mut last_snapshot_id: Option<String> = None;
            if has_tool_calls {
                let snapshot_manager = ErrorSnapshotManager::new(&self.current_dir);
                for tool_call in &tool_calls {
                    if let Some(tool_result) = tool_results.iter().find(|msg| {
                        msg.tool_call_id() == Some(tool_call.id.as_str())
                    }) {
                        if let ChatMessage::Tool { content, .. } = tool_result {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                                if val["ok"].as_bool() == Some(false) {
                                    let err_msg = val["error"]["message"]
                                        .as_str()
                                        .unwrap_or("unknown error");
                                    let tool_args: serde_json::Value =
                                        serde_json::from_str(&tool_call.arguments)
                                            .unwrap_or(serde_json::json!({}));
                                    let snapshot = snapshot_manager.capture(
                                        &self.context_manager.get_messages(),
                                        &tool_call.name,
                                        &tool_args,
                                        err_msg,
                                        None,
                                        0,
                                    );
                                    if let Ok(path) = snapshot_manager.save(&snapshot) {
                                        let id = snapshot.id.clone();
                                        eprintln!(
                                            "\r\x1b[2K\x1b[33m📸 错误快照已保存: {} -> {}\x1b[0m",
                                            id,
                                            path.display()
                                        );
                                        last_snapshot_id = Some(id);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // ⭐ 自动提取重要信息到持久化记忆（每 N 轮非工具调用的对话）
            let should_auto_extract = !has_tool_calls && !final_assistant_message.is_empty();
            let assistant_msg_clone = final_assistant_message.clone();

            // 将工具调用消息加入上下文
            if tool_calls.len() > 0 {
                self.context_manager.add_message(ChatMessage::assistant_tool_calls(
                    final_assistant_message,
                    tool_calls,
                ));
                is_auto = true;
            } else {
                self.context_manager.add_message(ChatMessage::assistant(final_assistant_message));
                is_auto = false;
            }

            // 将工具结果加入消息

            // ⭐ Goal 状态驱动的自动循环控制
            // 如果目标未完成（活跃状态），继续自动推进（不重复注入目标消息）
            if self.goal_manager.has_active_goal() {
                if let Some(active_goal) = self.goal_manager.active_goal() {
                    if !active_goal.status.is_terminal() {
                        // 目标未完成 → 继续自动推进
                        // ❗ 注意：目标状态不在每次循环重复注入，仅在：
                        //   1. 启动时（agent.rs:433-438）
                        //   2. 上下文压缩后（agent.rs:613-618）
                        // 这样可以防止上下文无限增长
                        is_auto = true;
                    } else {
                        // 目标已完成/失败/取消 → 停止自动循环
                        is_auto = false;
                    }
                }
            }

            // ⭐ 自动循环无限进行，直到目标进入终止状态（已完成/失败/取消）
            // 不再限制连续自动迭代次数

            // ⭐ 自动提取重要信息到持久化记忆（每 N 轮非工具调用的对话）
            if should_auto_extract {
                auto_extract_counter += 1;
                if auto_extract_counter >= AUTO_EXTRACT_INTERVAL {
                    auto_extract_counter = 0;
                    // 获取最近一轮的用户输入和助手回复
                    let recent_msgs = self.context_manager.get_messages();
                    let last_user = recent_msgs.iter().rev().find_map(|m| {
                        match m {
                            ChatMessage::User { content, .. } => Some(content.clone()),
                            _ => None,
                        }
                    });
                    if let Some(user_input) = last_user {
                        let memory_content = format!(
                            "[对话] 用户: {} | 助手: {}",
                            if user_input.len() > 200 { &user_input[..200] } else { &user_input },
                            if assistant_msg_clone.len() > 500 { &assistant_msg_clone[..500] } else { &assistant_msg_clone }
                        );
                        let tags = vec!["auto-extracted".to_string(), "conversation".to_string()];
                        match self.memory_manager.lock().await.save(
                            &memory_content,
                            &tags,
                            MemorySource::Conversation,
                            0.3,
                        ).await {
                            Ok(id) => {
                                eprintln!("\r\x1b[2K🧠 自动提取并保存了一条对话记忆 (id: {})", id);
                            }
                            Err(e) => {
                                eprintln!("\r\x1b[2K⚠️ 自动提取记忆失败: {}", e);
                            }
                        }
                    }
                }
            }
            for tool_call_id in tool_call_ids {
                if let Some(index) = tool_results
                    .iter()
                    .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
                {
                    let tool_result = tool_results.remove(index);

                    // ⭐ 如果是关键的工具结果，标记为重要
                    let is_important = is_important_tool_result(&tool_result);

                    self.context_manager.add_message(tool_result);

                    if is_important {
                        self.context_manager.preserve_last_message();
                    }
                }
            }
            // 剩余的 tool_results（没有对应 tool_call_id 的）
            for tool_result in tool_results {
                self.context_manager.add_message(tool_result);
            }
        }
    }
}

// =====================================================================
// 辅助函数（从 main.rs 迁移）
// =====================================================================

/// ⭐ 处理会话管理命令
fn handle_session_command(
    input: &str,
    session_manager: &SessionManager,
    ctx: &mut ContextManager,
    task_manager: &mut TaskManager,
) {
    let trimmed = input.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();

    if parts.is_empty() {
        print_session_help();
        return;
    }

    // /sessions 是 /session list 的快捷方式
    if parts[0] == "/sessions" {
        list_sessions(session_manager);
        return;
    }

    // /session 命令
    if parts.len() < 2 {
        print_session_help();
        return;
    }

    let subcommand = parts[1];

    match subcommand {
        "save" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /session save <名称>\x1b[0m");
                return;
            }
            let name = parts[2..].join(" ");
            match session_manager.save(&name, ctx) {
                Ok(session) => {
                    println!(
                        "\x1b[32m━━━ 💾 会话已保存 ━━━\x1b[0m\n  📁 名称: {}\n  💬 消息数: {}\n  🕐 时间: {}",
                        session.name,
                        session.messages.len(),
                        session.updated_at,
                    );
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 保存失败: {}\x1b[0m", e);
                }
            }
        }
        "load" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /session load <名称>\x1b[0m");
                return;
            }
            let name = parts[2..].join(" ");
            match session_manager.load(&name) {
                Ok(session) => {
                    // 保存当前上下文到自动快照
                    if ctx.get_messages().len() > 1 {
                        let auto_save_name = format!("_autosave_{}", chrono_now_simple());
                        let _ = session_manager.save(&auto_save_name, ctx);
                        println!("\x1b[90m  💾 当前上下文已自动保存为: {}\x1b[0m", auto_save_name);
                    }

                    // 生成恢复用的系统提示词
                    let restore_prompt = session_manager.default_system_prompt(&session);

                    // 重建 ContextManager
                    let restored_messages = session_manager.restore_messages(&session, &restore_prompt);
                    *ctx = ContextManager::new(restore_prompt, session.strategy.clone());

                    // 恢复消息
                    for msg in restored_messages.into_iter().skip(1) {
                        ctx.add_message(msg);
                    }

                    // 重置任务管理器
                    *task_manager = TaskManager::new(&session.current_dir);
                    task_manager.load();

                    println!(
                        "\x1b[32m━━━ 📂 会话已加载 ━━━\x1b[0m\n  📁 名称: {}\n  💬 消息数: {}\n  🕐 创建: {}\n  🕐 更新: {}",
                        session.name,
                        session.messages.len(),
                        session.created_at,
                        session.updated_at,
                    );
                    println!("\x1b[90m  💡 输入 /session list 查看所有会话\x1b[0m");
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 加载失败: {}\x1b[0m", e);
                    println!("\x1b[33m  💡 使用 /session list 查看可用会话\x1b[0m");
                }
            }
        }
        "list" => {
            list_sessions(session_manager);
        }
        "delete" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /session delete <名称>\x1b[0m");
                return;
            }
            let name = parts[2..].join(" ");
            match session_manager.delete(&name) {
                Ok(true) => {
                    println!("\x1b[32m━━━ 🗑️ 会话已删除: {}\x1b[0m", name);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  会话不存在: {}\x1b[0m", name);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 删除失败: {}\x1b[0m", e);
                }
            }
        }
        "rename" => {
            if parts.len() < 4 {
                println!("\x1b[33m⚠️  用法: /session rename <旧名称> <新名称>\x1b[0m");
                return;
            }
            let old_name = parts[2];
            let new_name = parts[3..].join(" ");
            match session_manager.rename(old_name, &new_name) {
                Ok(true) => {
                    println!("\x1b[32m━━━ ✏️ 会话已重命名: {} → {}\x1b[0m", old_name, new_name);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  会话不存在: {}\x1b[0m", old_name);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 重命名失败: {}\x1b[0m", e);
                }
            }
        }
        "help" | "-h" | "--help" => {
            print_session_help();
        }
        other => {
            println!("\x1b[33m⚠️  未知的子命令: {}\x1b[0m", other);
            print_session_help();
        }
    }
}

/// 列出所有会话
fn list_sessions(session_manager: &SessionManager) {
    match session_manager.list() {
        Ok(sessions) => {
            if sessions.is_empty() {
                println!("\x1b[33m📂 暂无保存的会话\x1b[0m");
                println!("\x1b[90m  💡 使用 /session save <名称> 保存当前对话\x1b[0m");
            } else {
                println!("\x1b[36m━━━ 📂 已保存的会话 (共 {}) ━━━\x1b[0m", sessions.len());
                for session in &sessions {
                    println!("{}", session);
                }
                println!("\x1b[90m  💡 使用 /session load <名称> 恢复对话\x1b[0m");
            }
        }
        Err(e) => {
            println!("\x1b[31m━━━ ❌ 列出会话失败: {}\x1b[0m", e);
        }
    }
}

/// 打印会话管理帮助
fn print_session_help() {
    println!("\x1b[36m━━━ 📋 会话管理命令 ━━━\x1b[0m");
    println!("  \x1b[33m/session save <名称>\x1b[0m    保存当前对话");
    println!("  \x1b[33m/session load <名称>\x1b[0m    加载已保存的对话");
    println!("  \x1b[33m/session list\x1b[0m           列出所有会话");
    println!("  \x1b[33m/session delete <名称>\x1b[0m  删除会话");
    println!("  \x1b[33m/session rename <旧> <新>\x1b[0m  重命名会话");
    println!("  \x1b[33m/sessions\x1b[0m                列出所有会话（快捷方式）");
    println!("  \x1b[33m/session help\x1b[0m            显示此帮助");
}

/// 获取简单的时间字符串（用于自动保存快照命名）
fn chrono_now_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let total_minutes = secs / 60;
    let hours = (total_minutes / 60) % 24;
    let minutes = total_minutes % 60;
    format!("{:02}{:02}", hours, minutes)
}

fn render_tool_result(content: &str) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let ok = value["ok"].as_bool().unwrap_or(false);
        if ok {
            if let Some(result) = value.get("result") {
                if result.is_object() {
                    let success = result["success"].as_bool().unwrap_or(true);
                    let status = result["status"].as_i64();
                    if success {
                        println!(
                            "\x1b[32m━━━ ✅ 执行成功 (exit: {}) ━━━\x1b[0m",
                            status.unwrap_or(0)
                        );
                    } else {
                        println!(
                            "\x1b[31m━━━ ❌ 执行失败 (exit: {}) ━━━\x1b[0m",
                            status.unwrap_or(-1)
                        );
                    }
                    if let Some(stdout) = result["stdout"].as_str() {
                        if !stdout.is_empty() {
                            print!("{}", stdout);
                            if !stdout.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                    if let Some(stderr) = result["stderr"].as_str() {
                        if !stderr.is_empty() {
                            print!("\x1b[31m{}\x1b[0m", stderr);
                            if !stderr.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(result).unwrap_or_default()
                    );
                }
            }
        } else {
            println!("\x1b[31m━━━ ❌ 工具调用失败 ━━━\x1b[0m");
            if let Some(error) = value.get("error") {
                println!(
                    "\x1b[31m  {}\x1b[0m",
                    error["message"].as_str().unwrap_or("unknown error")
                );
            }
        }
    }
}

/// 从 ChatMessage 中提取 content 并渲染工具结果
fn render_tool_result_from_msg(msg: &ChatMessage) {
    if let ChatMessage::Tool { content, .. } = msg {
        render_tool_result(content);
    }
}

/// 判断工具结果是否为重要上下文
fn is_important_tool_result(msg: &ChatMessage) -> bool {
    let ChatMessage::Tool { content, .. } = msg else { return false };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(content) else { return false };
    let Some(stdout) = val
        .get("result")
        .and_then(|r| r.get("stdout"))
        .and_then(|s| s.as_str())
    else { return false };

    crate::context::is_stdout_structural(stdout)
}

fn finish_terminal_line(terminal_line_dirty: &mut bool) {
    if *terminal_line_dirty {
        println!();
        *terminal_line_dirty = false;
    }
}

/// 从 ToolManager 动态生成「当前可用工具」的描述文本
fn generate_tools_description(tm: &ToolManager) -> String {
    let tools = tm.list_tools();
    let mut lines = Vec::new();
    for t in &tools {
        lines.push(format!("- {}: {}", t.name, t.description));
    }
    lines.join("\n")
}

/// ⭐ 处理 /model 命令：模型管理与切换
///
/// 支持子命令：
///   /model list    — 列出所有已注册的模型
///   /model current — 显示当前活跃的模型
///   /model switch <name> — 切换到指定模型
fn handle_model_command(input: &str, model_manager: &mut ModelManager) {
    let parts: Vec<&str> = input.trim().split_whitespace().collect();
    if parts.len() < 2 {
        print_model_help();
        return;
    }

    let subcommand = parts[1];
    match subcommand {
        "list" | "ls" => {
            let models = model_manager.list_models();
            let active = model_manager.active_name().to_string();
            println!("\x1b[36m━━━ 🤖 已注册模型 (共 {}) ━━━\x1b[0m", models.len());
            for cfg in &models {
                let indicator = if cfg.name == active { "→ " } else { "  " };
                let active_mark = if cfg.name == active { " \x1b[32m(当前)\x1b[0m" } else { "" };
                println!(
                    "  {}\x1b[33m{:<12}\x1b[0m {} {}{}",
                    indicator,
                    cfg.name,
                    cfg.model_name,
                    cfg.provider,
                    active_mark,
                );
            }
        }
        "current" | "cur" | "active" => {
            if let Some(cfg) = model_manager.current() {
                println!("\x1b[36m━━━ 🤖 当前活跃模型 ━━━\x1b[0m");
                println!("  \x1b[33m名称:\x1b[0m     {}", cfg.name);
                println!("  \x1b[33m模型:\x1b[0m     {}", cfg.model_name);
                println!("  \x1b[33m提供商:\x1b[0m   {}", cfg.provider);
                println!("  \x1b[33mAPI Base:\x1b[0m {}", cfg.base_url);
            } else {
                println!("\x1b[33m⚠️  当前没有活跃的模型\x1b[0m");
            }
        }
        "switch" | "use" | "set" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /model switch <模型名称>\x1b[0m");
                return;
            }
            let target = parts[2..].join(" ");
            match model_manager.switch(&target) {
                Ok(_) => {
                    println!("\x1b[32m━━━ ✅ 已切换到模型 '{}{}' ━━━\x1b[0m", '\'', &target);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 切换失败: {}\x1b[0m", e);
                }
            }
        }
        "help" | "-h" | "--help" => {
            print_model_help();
        }
        _ => {
            println!("\x1b[33m⚠️  未知的子命令: {}\x1b[0m", subcommand);
            print_model_help();
        }
    }
}

/// 打印 /model 命令帮助
fn print_model_help() {
    println!("\x1b[36m━━━ 🤖 模型管理命令 ━━━\x1b[0m");
    println!("  \x1b[33m/model list\x1b[0m              列出所有已注册模型");
    println!("  \x1b[33m/model current\x1b[0m           显示当前活跃模型");
    println!("  \x1b[33m/model switch <名称>\x1b[0m      切换到指定模型");
    println!("  \x1b[33m/model help\x1b[0m               显示此帮助");
}

/// ⭐ 处理 /goal 命令：目标管理
///
/// 支持子命令：
///   /goal list                         — 列出所有目标
///   /goal set <描述>                   — 设置新目标
///   /goal complete <id>                — 标记目标为已完成
///   /goal fail <id> [原因]             — 标记目标为失败
///   /goal cancel <id>                  — 取消目标
///   /goal status                       — 显示当前活跃目标
///   /goal history                      — 显示历史目标
fn handle_goal_command(input: &str, goal_manager: &mut GoalRegistry) {
    let parts: Vec<&str> = input.trim().split_whitespace().collect();
    if parts.len() < 2 {
        print_goal_help();
        return;
    }

    let subcommand = parts[1];
    match subcommand {
        "list" | "ls" => {
            let goals = goal_manager.list();
            println!("\x1b[36m━━━ 🎯 所有目标 (共 {}) ━━━\x1b[0m", goals.len());
            for goal in &goals {
                let status_str = goal.status.to_string();
                let status_icon = match status_str.to_lowercase().as_str() {
                    "active" => "\x1b[32m🟢\x1b[0m",
                    "completed" => "\x1b[34m✅\x1b[0m",
                    "failed" => "\x1b[31m❌\x1b[0m",
                    "cancelled" => "\x1b[33m🚫\x1b[0m",
                    _ => "\x1b[90m⚪\x1b[0m",
                };
                println!(
                    "  {} \x1b[33m{:<8}\x1b[0m {}",
                    status_icon,
                    goal.id,
                    goal.description
                );
            }
        }

        "set" | "add" | "new" | "create" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal set <目标描述>\x1b[0m");
                return;
            }
            let description = parts[2..].join(" ");
            match goal_manager.create_goal(description.clone()) {
                Ok(goal) => {
                    println!(
                        "\x1b[32m━━━ 🎯 新目标已创建 ━━━\x1b[0m\n  🆔 ID: \x1b[33m{}\x1b[0m\n  📝 描述: {}\n  📂 状态: \x1b[32mactive\x1b[0m",
                        goal.id, goal.description
                    );
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 创建目标失败: {}\x1b[0m", e);
                }
            }
        }
        "complete" | "done" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal complete <id>\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            match goal_manager.mark_complete(goal_id) {
                Ok(true) => {
                    println!("\x1b[32m━━━ ✅ 目标 '{}' 已完成 ━━━\x1b[0m 🎉", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 标记完成失败: {}\x1b[0m", e);
                }
            }
        }
        "fail" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal fail <id> [原因]\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            let reason = if parts.len() > 3 {
                parts[3..].join(" ")
            } else {
                "unexpected error".to_string()
            };
            match goal_manager.mark_failed(goal_id, &reason) {
                Ok(true) => {
                    println!("\x1b[31m━━━ ❌ 目标 '{}' 已标记为失败 ━━━\x1b[0m", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 标记失败失败: {}\x1b[0m", e);
                }
            }
        }
        "cancel" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal cancel <id>\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            match goal_manager.mark_cancelled(goal_id) {
                Ok(true) => {
                    println!("\x1b[33m━━━ 🚫 目标 '{}' 已取消 ━━━\x1b[0m", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 取消失败: {}\x1b[0m", e);
                }
            }
        }
        "status" | "cur" | "current" | "active" => {
            if let Some(goal) = goal_manager.active_goal() {
                println!("\x1b[36m━━━ 🎯 当前活跃目标 ━━━\x1b[0m");
                println!("  🆔 ID:     \x1b[33m{}\x1b[0m", goal.id);
                println!("  📝 描述:   {}", goal.description);
                println!("  📂 状态:   \x1b[32m{}\x1b[0m", goal.status.to_string());
                println!("  🕐 创建:   {}", goal.created_at);
            } else {
                println!("\x1b[33m⚠️  当前没有活跃目标\x1b[0m");
                println!("\x1b[90m  💡 使用 /goal set <描述> 创建新目标\x1b[0m");
            }
        }
        "history" | "hist" | "log" => {
            let goals = goal_manager.list();
            let completed: Vec<_> = goals.iter().filter(|g| g.status.to_string().to_lowercase() != "active").collect();
            if completed.is_empty() {
                println!("\x1b[33m📜 暂无历史目标记录\x1b[0m");
            } else {
                println!("\x1b[36m━━━ 📜 历史目标 (共 {}) ━━━\x1b[0m", completed.len());
                for goal in completed {
                    let status_str = goal.status.to_string();
                    let status_icon = match status_str.to_lowercase().as_str() {
                        "completed" => "\x1b[34m✅\x1b[0m",
                        "failed" => "\x1b[31m❌\x1b[0m",
                        "cancelled" => "\x1b[33m🚫\x1b[0m",
                        _ => "\x1b[90m⚪\x1b[0m",
                    };
                    println!(
                        "  {} \x1b[33m{:<8}\x1b[0m {}",
                        status_icon,
                        goal.id,
                        goal.description,
                    );
                }
            }
        }
        "delete" | "rm" | "remove" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal delete <id>\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            match goal_manager.delete(goal_id) {
                Ok(true) => {
                    println!("\x1b[32m━━━ 🗑️ 目标 '{}' 已删除 ━━━\x1b[0m", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 删除失败: {}\x1b[0m", e);
                }
            }
        }
        "clear" | "clean" | "purge" => {
            let count = goal_manager.list().len();
            if count == 0 {
                println!("\x1b[33m⚠️  当前没有目标需要清理\x1b[0m");
                return;
            }
            // 要求确认
            if parts.len() > 2 && (parts[2] == "--force" || parts[2] == "-f") {
                match goal_manager.clear_all() {
                    Ok(n) => {
                        println!("\x1b[32m━━━ 🧹 已清空 {} 个目标 ━━━\x1b[0m", n);
                    }
                    Err(e) => {
                        println!("\x1b[31m━━━ ❌ 清空失败: {}\x1b[0m", e);
                    }
                }
            } else {
                println!("\x1b[33m⚠️  确定要清空所有 {} 个目标吗？\x1b[0m", count);
                println!("  \x1b[33m使用 /goal clear --force 确认执行\x1b[0m");
            }
        }
        "help" | "-h" | "--help" => {
            print_goal_help();
        }
        _ => {
            println!("\x1b[33m⚠️  未知的子命令: {}\x1b[0m", subcommand);
            print_goal_help();
        }
    }
}

/// 打印 /goal 命令帮助
fn print_goal_help() {
    println!("\x1b[36m━━━ 🎯 目标管理命令 ━━━\x1b[0m");
    println!("  \x1b[33m/goal set <描述>\x1b[0m          创建新目标（设定后 agent 会持续推进直到完成）");
    println!("  \x1b[33m/goal list\x1b[0m                列出所有目标");
    println!("  \x1b[33m/goal status\x1b[0m              显示当前活跃目标");
    println!("  \x1b[33m/goal complete <id>\x1b[0m       标记目标为已完成");
    println!("  \x1b[33m/goal fail <id> [原因]\x1b[0m    标记目标为失败");
    println!("  \x1b[33m/goal cancel <id>\x1b[0m         取消目标");
    println!("  \x1b[33m/goal delete <id>\x1b[0m         删除目标（从磁盘彻底移除）");
    println!("  \x1b[33m/goal clear --force\x1b[0m       清空所有目标（需确认）");
    println!("  \x1b[33m/goal history\x1b[0m             查看历史目标");
    println!("  \x1b[33m/goal help\x1b[0m                显示此帮助");
}

/// 创建默认的工具管理器
fn default_tool_manager() -> ToolManager {
    let mut tool_manager = ToolManager::new();
    tool_manager.register_tool(Box::new(BashShell));
    tool_manager.register_tool(Box::new(DebugTool));
    tool_manager.register_tool(Box::new(EditTool));
    tool_manager.register_tool(Box::new(ReadTool));
    tool_manager.register_tool(Box::new(SearchTool));
    tool_manager.register_tool(Box::new(SpawnAgent));
    tool_manager.register_tool(Box::new(InvestigateTool::new(".")));
    tool_manager.register_tool(Box::new(GenerateTool::new(".")));
    tool_manager.register_tool(Box::new(HelloWorld));
    tool_manager
}


/// ⭐ 从 LLM 输出文本中提取 Goal 完成信号
///
/// 解析 `/goal complete <id>`、`/goal fail <id> [reason]`、`/goal cancel <id>` 模式。
/// 返回 (action, id, reason) 三元组。
fn extract_goal_signal(text: &str) -> Option<(String, String, String)> {
    // 匹配模式：/goal complete <id>
    //            /goal fail <id> [reason]
    //            /goal cancel <id>
    let re = regex::Regex::new(r"/goal\s+(complete|fail|cancel)\s+(\S+)(?:\s+(.*))?").ok()?;
    if let Some(caps) = re.captures(text) {
        let action = caps.get(1)?.as_str().to_string();
        let goal_id = caps.get(2)?.as_str().to_string();
        let reason = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
        Some((action, goal_id, reason))
    } else {
        None
    }
}
