use std::path::PathBuf;

pub(super) async fn ensure_module_decl(
    project_root: &str,
    tool_name: &str,
) -> Result<bool, String> {
    let mod_path = PathBuf::from(project_root)
        .join("src")
        .join("tools")
        .join("mod.rs");
    let content = tokio::fs::read_to_string(&mod_path)
        .await
        .map_err(|e| e.to_string())?;
    let mod_decl = format!("pub mod {};", tool_name);
    if content.contains(&mod_decl) {
        return Ok(false);
    }

    let new_content = if let Some(pos) = content.rfind("pub mod ") {
        let insert_pos = content[pos..]
            .find('\n')
            .map(|p| pos + p + 1)
            .unwrap_or(content.len());
        let mut updated = content.clone();
        updated.insert_str(insert_pos, &format!("pub mod {};\n", tool_name));
        updated
    } else {
        format!("{}\npub mod {};\n", content, tool_name)
    };

    tokio::fs::write(&mod_path, new_content)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

pub(super) async fn ensure_default_tool_registration(
    project_root: &str,
    tool_name: &str,
    struct_name: &str,
) -> Result<bool, String> {
    let path = PathBuf::from(project_root)
        .join("src")
        .join("agent")
        .join("default_tools.rs");
    let mut content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;

    let import_entry = format!("{}::{},", tool_name, struct_name);
    let mut updated = false;
    if !content.contains(&import_entry) {
        if let Some(pos) = content.find("ToolManager,") {
            content.insert_str(pos + "ToolManager,".len(), &format!(" {}", import_entry));
            updated = true;
        } else {
            return Err("could not find ToolManager import anchor".to_string());
        }
    }

    let register_line = format!("    tool_manager.register_tool(Box::new({}));", struct_name);
    if !content.contains(&register_line) {
        if let Some(pos) = content.rfind("    tool_manager\n") {
            content.insert_str(pos, &format!("{}\n", register_line));
            updated = true;
        } else {
            return Err("could not find tool_manager return anchor".to_string());
        }
    }

    if updated {
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generated_tool_registration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("src/tools"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("src/agent"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("src/tools/mod.rs"),
            "pub mod shell;\npub mod types;\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join("src/agent/default_tools.rs"),
            r#"use crate::tools::{
    ToolManager, shell::BashShell,
};

pub(super) fn default_tool_manager() -> ToolManager {
    let mut tool_manager = ToolManager::new();
    tool_manager.register_tool(Box::new(BashShell));
    tool_manager
}
"#,
        )
        .await
        .unwrap();

        let project_root = root.to_string_lossy().to_string();
        assert!(
            ensure_module_decl(&project_root, "demo_tool")
                .await
                .unwrap()
        );
        assert!(
            !ensure_module_decl(&project_root, "demo_tool")
                .await
                .unwrap()
        );
        assert!(
            ensure_default_tool_registration(&project_root, "demo_tool", "DemoTool")
                .await
                .unwrap()
        );
        assert!(
            !ensure_default_tool_registration(&project_root, "demo_tool", "DemoTool")
                .await
                .unwrap()
        );

        let mod_rs = tokio::fs::read_to_string(root.join("src/tools/mod.rs"))
            .await
            .unwrap();
        assert!(mod_rs.contains("pub mod demo_tool;"));
        let default_tools = tokio::fs::read_to_string(root.join("src/agent/default_tools.rs"))
            .await
            .unwrap();
        assert!(default_tools.contains("demo_tool::DemoTool,"));
        assert!(default_tools.contains("tool_manager.register_tool(Box::new(DemoTool));"));
    }
}
