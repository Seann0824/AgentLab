use agent_lab_core::services::rag_service::IndexDocumentResult;
use tauri::State;

use crate::state::AppState;

/// 将 Markdown 内容索引到指定 namespace。
#[tauri::command]
pub async fn index_document_content(
    state: State<'_, AppState>,
    namespace: String,
    content: String,
    source: String,
) -> Result<IndexDocumentResult, String> {
    if namespace.trim().is_empty() {
        return Err("namespace 不能为空".to_string());
    }
    if content.trim().is_empty() {
        return Err("文档内容不能为空".to_string());
    }

    let source_label = if source.trim().is_empty() {
        "uploaded-document".to_string()
    } else {
        source
    };

    state
        .rag_service
        .index_document_content(&content, &source_label, &namespace, 512, 64)
        .await
        .map_err(|e| e.to_string())
}

/// 列出所有已索引的 namespace。
#[tauri::command]
pub async fn list_namespaces(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state
        .rag_service
        .list_namespaces()
        .await
        .map_err(|e| e.to_string())
}

/// 删除指定 namespace 及其索引数据。
#[tauri::command]
pub async fn delete_namespace(
    state: State<'_, AppState>,
    namespace: String,
) -> Result<bool, String> {
    let deleted = state
        .rag_service
        .delete_namespace(&namespace)
        .await
        .map_err(|e| e.to_string())?;
    Ok(deleted > 0)
}
