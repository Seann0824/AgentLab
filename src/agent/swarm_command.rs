use futures_util::StreamExt;

use crate::swarm::registry::SwarmRegistry;
use crate::tools::swarm_ctl::SwarmCtl;
use crate::tools::types::{Tool, ToolEvent};

pub(super) async fn handle_swarm_command(input: &str, registry: Option<SwarmRegistry>) {
    let tool = SwarmCtl::new(registry);
    let parts: Vec<&str> = input.split_whitespace().collect();
    let action = parts.get(1).copied().unwrap_or("status");

    let mut args = serde_json::json!({ "action": action });
    if let Some(agent_type) = parts.get(2) {
        args["agent_type"] = serde_json::json!(agent_type);
    }

    let mut stream = tool.execute(args);
    while let Some(event) = stream.next().await {
        match event {
            ToolEvent::Done(result) => render_swarm_result(&result),
            ToolEvent::Err(msg) => println!("\x1b[31m❌ {}\x1b[0m", msg),
            _ => {}
        }
    }
}

fn render_swarm_result(result: &serde_json::Value) {
    let msg = result["message"].as_str().unwrap_or("");
    if !msg.is_empty() {
        println!("\x1b[36m{}\x1b[0m", msg);
    }

    if let Some(status) = result.get("swarm_status") {
        let total = status["total_agents"].as_i64().unwrap_or(0);
        let online = status["online"].as_i64().unwrap_or(0);
        let offline = status["offline"].as_i64().unwrap_or(0);
        println!("  🐝 总计: {} 个 Agent", total);
        println!("  ✅ 在线: {}", online);
        println!("  ❌ 离线: {}", offline);
        if let Some(agents) = status["agents"].as_array() {
            render_status_agents(agents);
        }
    }

    if let Some(agents) = result.get("agents").and_then(|a| a.as_array()) {
        render_query_agents(agents);
    }
    if let Some(hint) = result.get("hint") {
        println!("  💡 {}", hint.as_str().unwrap_or(""));
    }
    if let Some(mods) = result.get("available_modules").and_then(|m| m.as_array()) {
        println!(
            "  📦 可用模块: {}",
            mods.iter()
                .filter_map(|m| m.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn render_status_agents(agents: &[serde_json::Value]) {
    if agents.is_empty() {
        return;
    }
    println!("\x1b[90m  Agent 列表:\x1b[0m");
    for agent in agents {
        let id = agent["id"].as_str().unwrap_or("?");
        let atype = agent["type"].as_str().unwrap_or("?");
        let status = agent["status"].as_str().unwrap_or("?");
        println!("    🆔 {} ({} — {})", id, atype, status);
    }
}

fn render_query_agents(agents: &[serde_json::Value]) {
    if agents.is_empty() {
        println!("  📭 没有匹配的 Agent");
        return;
    }
    println!("\x1b[90m  Agent 列表:\x1b[0m");
    for agent in agents {
        let id = agent["agent_id"].as_str().unwrap_or("?");
        let atype = agent["agent_type"].as_str().unwrap_or("?");
        let status = agent["status"].as_str().unwrap_or("?");
        println!("    🆔 {} ({} — {})", id, atype, status);
    }
}
