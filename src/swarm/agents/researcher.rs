// src/swarm/agents/researcher.rs
// 🔬 Researcher Agent — 技术调研 Agent
//
// Researcher Agent 是一个非交互式 Agent，通过 UDS 与 Orchestrator 通信。
// 职责：
// 1. 代码库架构分析 — 分析项目结构、模块关系、关键组件
// 2. 技术调研 — 分析代码库中的技术栈、依赖、模式
// 3. 可行性分析 — 评估技术方案的可行性和风险
// 4. 调研报告生成 — 输出结构化的 markdown 调研文档
// 5. 技术方案对比 — 对比不同技术路径的优劣

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use tokio::sync::Mutex as TokioMutex;
use tokio::time::interval;

use crate::swarm::heartbeat::create_heartbeat_request;
use crate::swarm::rpc::JsonRpcRequest;
use crate::swarm::transport::{UdsClient, default_socket_path};

// ============================================================
// 简单时间工具（无 chrono 依赖）
// ============================================================

/// 获取当前时间的格式化字符串（YYYY-MM-DD HH:MM:SS 格式）
fn now_formatted() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day: u64 = 86400;
    let days = now / secs_per_day;
    let time_secs = now % secs_per_day;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i;
            break;
        }
        remaining_days -= md;
    }
    let d = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m + 1,
        d,
        hours,
        minutes,
        seconds
    )
}

/// 获取紧凑时间戳（YYYYMMDD-HHMMSS）
#[allow(dead_code)]
fn now_compact() -> String {
    let s = now_formatted();
    s.replace('-', "").replace(' ', "-").replace(':', "")
}

/// 判断闰年
fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ============================================================
// Researcher Agent
// ============================================================

/// Researcher Agent — 技术调研 Agent
pub struct ResearcherAgent {
    /// Agent ID
    agent_id: String,
    /// UDS 客户端（连接到 Orchestrator），用 Arc<Mutex> 共享给心跳任务
    client: Option<Arc<TokioMutex<UdsClient>>>,
    /// 是否正在运行
    running: bool,
    /// 项目路径
    project_path: PathBuf,
}

impl ResearcherAgent {
    /// 创建新的 Researcher Agent
    pub fn new(project_path: Option<PathBuf>) -> Self {
        Self {
            agent_id: format!("researcher-{}", std::process::id()),
            client: None,
            running: false,
            project_path: project_path.unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    /// 连接到 Orchestrator
    pub async fn connect(&mut self, orchestrator_socket: Option<PathBuf>) -> Result<()> {
        let socket = orchestrator_socket.unwrap_or_else(default_socket_path);
        eprintln!("🔬 Researcher Agent 连接到 Orchestrator @ {:?}", socket);

        let client = UdsClient::connect(&socket, &self.agent_id)
            .await
            .context(format!("无法连接到 Orchestrator (socket: {:?})", socket))?;

        eprintln!("🔬 Researcher Agent '{}' 已注册到蜂群", self.agent_id);

        self.client = Some(Arc::new(TokioMutex::new(client)));
        Ok(())
    }

    /// 运行 Researcher Agent 主循环
    pub async fn run(&mut self) -> Result<()> {
        self.running = true;
        eprintln!("🔬 Researcher Agent 主循环已启动");

        // 启动心跳任务
        let agent_id = self.agent_id.clone();
        let client_arc = self.client.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(15));
            loop {
                ticker.tick().await;
                if let Some(ref client) = client_arc {
                    let mut client = client.lock().await;
                    let hb = create_heartbeat_request(&agent_id);
                    if let Err(e) = client.send_request(&hb).await {
                        eprintln!("🔬 [Heartbeat] 发送失败: {}", e);
                    }
                }
            }
        });

        // 主循环：等待处理任务
        while self.running {
            if let Some(ref client_arc) = self.client {
                let mut client = client_arc.lock().await;
                match client.read_request().await {
                    Ok(request) => {
                        let _method = request.method.clone();
                        drop(client);
                        self.handle_request(request).await;
                    }
                    Err(e) => {
                        drop(client);
                        eprintln!("🔬 读取请求失败: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        Ok(())
    }

    /// 处理收到的请求
    async fn handle_request(&mut self, request: JsonRpcRequest) {
        match request.method.as_str() {
            "read_file" => {
                let file_path = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("🔬 读取文件: {}", file_path);
                let result = self.read_file(file_path).await;
                self.send_response(&request.id, result).await;
            }
            "search_code" => {
                let pattern = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("pattern"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let include_ext = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("include_ext"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("🔬 搜索代码: {} (ext: {})", pattern, include_ext);
                let result = self.search_code(pattern, include_ext).await;
                self.send_response(&request.id, result).await;
            }
            "analyze_codebase" => {
                let include_patterns = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("include_patterns"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("🔬 分析代码库结构");
                let result = self.analyze_codebase(include_patterns).await;
                self.send_response(&request.id, result).await;
            }
            "generate_report" => {
                let title = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("title"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("技术调研报告");
                let content = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let output_path = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("output_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("🔬 生成调研报告: {}", title);
                let result = self.generate_report(title, content, output_path).await;
                self.send_response(&request.id, result).await;
            }
            "analyze_dependencies" => {
                let dep_file = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("dep_file"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Cargo.toml");
                eprintln!("🔬 分析依赖: {}", dep_file);
                let result = self.analyze_dependencies(dep_file).await;
                self.send_response(&request.id, result).await;
            }
            "compare_approaches" => {
                let approaches = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("approaches"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let criteria = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("criteria"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("🔬 对比技术方案");
                let result = self.compare_approaches(approaches, criteria).await;
                self.send_response(&request.id, result).await;
            }
            "ping" => {
                if let Some(ref client_arc) = self.client {
                    let mut client = client_arc.lock().await;
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": request.id,
                        "result": {
                            "success": true,
                            "status": "alive",
                            "agent_id": self.agent_id,
                        }
                    });
                    let _ = client
                        .send_raw(&serde_json::to_string(&resp).unwrap())
                        .await;
                }
            }
            "shutdown" => {
                eprintln!("🔬 Researcher Agent 收到关闭信号");
                self.running = false;
            }
            other => {
                eprintln!("🔬 未知方法: {}", other);
            }
        }
    }

    /// 发送 JSON-RPC 响应
    async fn send_response(&self, id: &str, result: serde_json::Value) {
        if let Some(ref client_arc) = self.client {
            let mut client = client_arc.lock().await;
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "success": true,
                    "result": result,
                }
            });
            let _ = client
                .send_raw(&serde_json::to_string(&resp).unwrap())
                .await;
        }
    }

    // ============================================================
    // 调研方法
    // ============================================================

    /// 读取文件内容
    async fn read_file(&self, file_path: &str) -> serde_json::Value {
        if file_path.is_empty() {
            return json!({
                "success": false,
                "error": "file_path 不能为空",
            });
        }

        let path = self.project_path.join(file_path);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let line_count = content.lines().count();
                let char_count = content.chars().count();
                json!({
                    "success": true,
                    "content": content,
                    "line_count": line_count,
                    "char_count": char_count,
                    "path": path.to_string_lossy().to_string(),
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("读取文件失败: {}", e),
                    "path": path.to_string_lossy().to_string(),
                })
            }
        }
    }

    /// 搜索代码（基于文件扩展名过滤）
    async fn search_code(&self, pattern: &str, include_ext: &str) -> serde_json::Value {
        if pattern.is_empty() {
            return json!({
                "success": false,
                "error": "pattern 不能为空",
            });
        }

        let path = self.project_path.clone();

        // 使用 tokio::process::Command 运行 grep 搜索
        let mut cmd = tokio::process::Command::new("grep");
        cmd.arg("-rn")
            .arg("--include=*.rs")
            .arg(&pattern)
            .arg(&path);

        if !include_ext.is_empty() {
            for ext in include_ext.split(',') {
                let ext = ext.trim();
                if !ext.is_empty() {
                    cmd.arg(format!("--include=*.{}", ext));
                }
            }
        }

        // 排除目录
        cmd.arg("--exclude-dir=target")
            .arg("--exclude-dir=.git")
            .arg("--exclude-dir=node_modules");

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut matches = Vec::new();
                for line in stdout.lines().take(100) {
                    matches.push(line.to_string());
                }

                json!({
                    "success": output.status.success() || !matches.is_empty(),
                    "match_count": matches.len(),
                    "matches": matches,
                    "pattern": pattern,
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("搜索失败: {}", e),
                })
            }
        }
    }

    /// 分析代码库结构
    async fn analyze_codebase(&self, include_patterns: &str) -> serde_json::Value {
        let path = self.project_path.clone();

        // 使用 find 命令获取文件列表
        let mut cmd = tokio::process::Command::new("find");
        cmd.arg(&path)
            .arg("-type").arg("f")
            .arg("-not").arg("-path").arg("*/target/*")
            .arg("-not").arg("-path").arg("*/.git/*")
            .arg("-not").arg("-path").arg("*/node_modules/*");

        if !include_patterns.is_empty() {
            cmd.arg("|").arg("grep").arg("-E").arg(&include_patterns);
        }

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let files: Vec<&str> = stdout.lines().collect();

                // 按扩展名分组统计
                let mut ext_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                let mut top_dirs: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

                for file in &files {
                    if let Some(ext) = PathBuf::from(file).extension() {
                        let ext_str = ext.to_string_lossy().to_string();
                        *ext_count.entry(ext_str).or_insert(0) += 1;
                    }

                    let rel = file.strip_prefix(path.to_string_lossy().as_ref()).unwrap_or(file);
                    let trimmed = rel.trim_start_matches('/');
                    if let Some(first) = trimmed.split('/').next() {
                        if !first.is_empty() {
                            *top_dirs.entry(first.to_string()).or_insert(0) += 1;
                        }
                    }
                }

                json!({
                    "success": true,
                    "total_files": files.len(),
                    "top_files": files.iter().take(50).map(|f| f.to_string()).collect::<Vec<_>>(),
                    "extension_summary": ext_count,
                    "directory_summary": top_dirs,
                    "project_path": path.to_string_lossy().to_string(),
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("分析失败: {}", e),
                })
            }
        }
    }

    /// 生成调研报告（写入 markdown 文件）
    async fn generate_report(&self, title: &str, content: &str, output_path: &str) -> serde_json::Value {
        if content.is_empty() {
            return json!({
                "success": false,
                "error": "content 不能为空",
            });
        }

        let path = if output_path.is_empty() {
            self.project_path.join("docs/analyses").join(format!(
                "research-{}.md",
                now_compact()
            ))
        } else {
            self.project_path.join(output_path)
        };

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let timestamp = now_formatted();
        let report = format!(
            "# {}\n\n> **生成时间**: {}\n> **Agent**: {} ({})\n\n---\n\n{}",
            title,
            timestamp,
            self.agent_id,
            std::process::id(),
            content,
        );

        match tokio::fs::write(&path, &report).await {
            Ok(()) => {
                json!({
                    "success": true,
                    "path": path.to_string_lossy().to_string(),
                    "title": title,
                    "bytes_written": report.len(),
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("写入报告失败: {}", e),
                })
            }
        }
    }

    /// 分析依赖（解析 Cargo.toml 或类似依赖文件）
    async fn analyze_dependencies(&self, dep_file: &str) -> serde_json::Value {
        let path = self.project_path.join(dep_file);

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                // 尝试解析 TOML 格式的依赖
                let mut dependencies: Vec<serde_json::Value> = Vec::new();
                let mut dev_dependencies: Vec<serde_json::Value> = Vec::new();
                let mut build_dependencies: Vec<serde_json::Value> = Vec::new();

                let mut in_deps = false;
                let mut in_dev_deps = false;
                let mut in_build_deps = false;

                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("[dependencies]") {
                        in_deps = true;
                        in_dev_deps = false;
                        in_build_deps = false;
                        continue;
                    }
                    if trimmed.starts_with("[dev-dependencies]") {
                        in_deps = false;
                        in_dev_deps = true;
                        in_build_deps = false;
                        continue;
                    }
                    if trimmed.starts_with("[build-dependencies]") {
                        in_deps = false;
                        in_dev_deps = false;
                        in_build_deps = true;
                        continue;
                    }
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        in_deps = false;
                        in_dev_deps = false;
                        in_build_deps = false;
                        continue;
                    }

                    if let Some(eq_pos) = trimmed.find('=') {
                        let name = trimmed[..eq_pos].trim();
                        if name.contains('"') || name.contains('\'') || name.is_empty() {
                            continue;
                        }
                        let version_info = trimmed[eq_pos + 1..].trim();

                        let dep = json!({
                            "name": name,
                            "version_info": version_info,
                            "raw": trimmed,
                        });

                        if in_deps {
                            dependencies.push(dep);
                        } else if in_dev_deps {
                            dev_dependencies.push(dep);
                        } else if in_build_deps {
                            build_dependencies.push(dep);
                        }
                    }
                }

                json!({
                    "success": true,
                    "file": dep_file,
                    "dependencies": dependencies,
                    "dev_dependencies": dev_dependencies,
                    "build_dependencies": build_dependencies,
                    "total_prod": dependencies.len(),
                    "total_dev": dev_dependencies.len(),
                    "total_build": build_dependencies.len(),
                    "raw_content": content,
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("读取依赖文件失败: {}", e),
                    "file": dep_file,
                })
            }
        }
    }

    /// 对比技术方案（生成结构化对比文档框架）
    async fn compare_approaches(&self, approaches: &str, criteria: &str) -> serde_json::Value {
        if approaches.is_empty() {
            return json!({
                "success": false,
                "error": "approaches 不能为空。应提供要对比的技术方案描述。",
            });
        }

        let criteria_list: Vec<&str> = if criteria.is_empty() {
            vec![
                "实现复杂度",
                "性能",
                "可维护性",
                "可扩展性",
                "社区支持",
                "学习成本",
            ]
        } else {
            criteria.split(',').map(|s| s.trim()).collect()
        };

        // 生成对比文档框架
        let timestamp = now_formatted();
        let mut comparison = String::from("# 技术方案对比报告\n\n");
        comparison.push_str(&format!("> **分析时间**: {}\n\n", timestamp));
        comparison.push_str("## 方案描述\n\n");
        comparison.push_str(&format!("{}\n\n", approaches));
        comparison.push_str("## 对比维度\n\n");
        comparison.push_str("| 维度 | 方案 A | 方案 B | 方案 C |\n");
        comparison.push_str("|---|---|---|---|\n");
        for criterion in &criteria_list {
            comparison.push_str(&format!("| {} | | | |\n", criterion));
        }
        comparison.push_str("\n## 总体评估\n\n");
        comparison.push_str("- 优势: \n- 劣势: \n- 推荐: \n\n");
        comparison.push_str("## 决策建议\n\n");

        json!({
            "success": true,
            "approaches": approaches,
            "criteria": criteria_list,
            "comparison_framework": comparison,
            "criteria_count": criteria_list.len(),
            "message": "对比框架已生成，请基于实际技术方案填写各维度的评估内容",
        })
    }

    /// 停止 Researcher Agent
    pub fn stop(&mut self) {
        self.running = false;
    }
}
