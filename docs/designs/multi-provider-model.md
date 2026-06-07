# Multi-Provider Model 接入方案

## 1. 目标

支持接入多个 LLM 提供商（DeepSeek、OpenAI、Anthropic 等），并支持通过 `/model` 命令在运行时动态切换当前使用的模型，无需重启 Agent。

## 2. 当前状态分析

| 组件 | 当前能力 | 限制 |
|------|---------|------|
| `ModelAdapter` trait | 定义 `stream_chat()` 和 `clone_box()` 接口 | 仅有 1 个实现 |
| `OpenAiCompatibleAdapter` | OpenAI 兼容 API 的流式调用 | 只读单个 env 变量 |
| `Agent.model` | `Box<dyn ModelAdapter>` | 硬编码，运行时不可变 |
| 主循环 env 加载 | 读 `DEEPSEEK_API_KEY` / `DEEPSEEK_BASE_URL` | 只支持一个提供商 |

## 3. 架构设计

### 3.1 核心概念

```
┌──────────────────────────────────────────────────────────┐
│                     ModelManager                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐               │
│  │ deepseek  │  │  openai  │  │ claude   │  ← providers  │
│  │  (active) │  │          │  │          │               │
│  └──────────┘  └──────────┘  └──────────┘               │
│       │              │             │                     │
│       ▼              ▼             ▼                     │
│  OpenAiCompat   OpenAiCompat  AnthropicAdapter           │
│  ibleAdapter    ibleAdapter  (future)                    │
└──────────────────────────────────────────────────────────┘
```

### 3.2 ModelConfig — 模型配置

每个模型配置包含：

```rust
pub struct ModelConfig {
    pub name: String,           // 唯一标识名，如 "deepseek", "openai-gpt4"
    pub provider: String,       // 提供商类型: "openai-compatible", "anthropic" 等
    pub base_url: String,       // API 基础 URL
    pub api_key: String,        // API 密钥
    pub model_name: String,     // 模型名，如 "deepseek-v4-flash", "gpt-4o"
}
```

### 3.3 配置加载来源

**环境变量命名约定**（通过前缀区分多提供商）：

```bash
# DeepSeek
DEEPSEEK_API_KEY=sk-xxx
DEEPSEEK_BASE_URL=https://api.deepseek.com
DEEPSEEK_MODEL=deepseek-v4-flash

# OpenAI  
OPENAI_API_KEY=sk-xxx
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_MODEL=gpt-4o

# 自定义
CUSTOM_API_KEY=sk-xxx
CUSTOM_BASE_URL=https://xxx.com/v1
CUSTOM_MODEL=claude-3-opus
```

自动检测规则：
- 扫描所有环境变量，寻找 `{PREFIX}_API_KEY` + `{PREFIX}_BASE_URL` 配对
- 每对注册为一个 ModelConfig
- `{PREFIX}_MODEL` 可选，默认为 `gpt-4o` 或 `default`

### 3.4 ModelManager API

```rust
impl ModelManager {
    fn from_env() -> Self;                          // 从环境变量加载所有模型
    fn from_config(configs: Vec<ModelConfig>) -> Self;
    
    fn list_models(&self) -> Vec<&ModelConfig>;     // 列出所有注册的模型
    fn get_model(&self, name: &str) -> Option<&ModelConfig>;
    fn current(&self) -> &ModelConfig;              // 当前活跃模型
    fn current_adapter(&self) -> &Box<dyn ModelAdapter>;
    
    fn switch(&mut self, name: &str) -> Result<()>; // 切换当前模型
    fn add_model(&mut self, config: ModelConfig);   // 动态注册新模型
}
```

### 3.5 动态切换机制

`Agent` 中持有 `ModelManager`，而不是直接持有 `Box<dyn ModelAdapter>`：

```rust
pub struct Agent {
    model_manager: ModelManager,  // 替代 model: Box<dyn ModelAdapter>
    // ... 其他字段不变
}
```

切换时：
1. `Agent.switch_model("openai")` → 调用 `model_manager.switch("openai")`
2. 后续的 `stream_chat` 调用从 `model_manager.current_adapter()` 获取

### 3.6 `/model` 命令

```
/model              → 显示当前模型信息
/model list         → 列出所有可用模型
/model switch <名>  → 切换到指定模型（例如 /model switch openai）
/model current      → 显示当前模型详情
```

## 4. 实现步骤

### Phase 1: ModelConfig + ModelManager（本实现）

1. 创建 `src/model/config.rs` — `ModelConfig` 结构体
2. 创建 `src/model/manager.rs` — `ModelManager` 结构体
3. 创建 `src/model/providers.rs` — 统一 `build_adapter()` 工厂函数
4. 更新 `src/model/mod.rs` — 导出新模块

### Phase 2: Agent 集成

5. 修改 `Agent` 结构体：`model_manager` 替代 `model`
6. 实现 `Agent.switch_model()` 方法
7. AgentBuilder 也做对应适配

### Phase 3: CLI 命令

8. 在 `cli/mod.rs` 注册 `/model` 命令
9. 在 `agent.rs` 主循环添加 `/model` 命令处理

## 5. 验证标准

- [x] `cargo check` 编译通过
- [x] `/model list` 列出从 env 加载的所有模型
- [x] `/model switch <name>` 切换到指定模型
- [x] `/model current` 显示当前模型信息
- [x] 切换后 LLM 调用使用新模型
- [x] 向后兼容：仅配一个 provider 时行为不变
