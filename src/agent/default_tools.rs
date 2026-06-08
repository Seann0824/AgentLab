use crate::tools::{
    ToolManager, edit::EditTool, generate_tool::GenerateTool, hello_world::HelloWorld,
    investigate::InvestigateTool, read::ReadTool, search::SearchTool, shell::BashShell,
    subagent::SpawnAgent, swarm_ctl::SwarmCtl, tool_debug::DebugTool,
};

/// 创建默认的工具管理器
pub(super) fn default_tool_manager() -> ToolManager {
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
    tool_manager.register_tool(Box::new(SwarmCtl::new(None)));
    tool_manager
}
