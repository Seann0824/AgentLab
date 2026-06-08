use std::path::Path;

use tokio::fs;

pub(super) async fn read_file_lines(file_path: &str) -> Result<(String, Vec<String>), String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("file not found: {}", file_path));
    }
    let content = fs::read_to_string(path)
        .await
        .map_err(|e| format!("failed to read file: {}", e))?;
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    Ok((content, lines))
}

/// 读取文件内容，如果文件不存在则返回空内容（自动创建模式）
///
/// 适用于 append / insert 等操作——文件不存在时视为空文件，后续写入会自动创建。
pub(super) async fn read_file_lines_or_create(
    file_path: &str,
) -> Result<(String, Vec<String>), String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Ok((String::new(), Vec::new()));
    }
    let content = fs::read_to_string(path)
        .await
        .map_err(|e| format!("failed to read file: {}", e))?;
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    Ok((content, lines))
}

/// 将行列表写回文件
pub(super) async fn write_file_lines(file_path: &str, lines: &[String]) -> Result<(), String> {
    let content = lines.join("\n");
    // 确保文件末尾有换行
    let content = if content.ends_with('\n') {
        content
    } else {
        content + "\n"
    };
    fs::write(file_path, &content)
        .await
        .map_err(|e| format!("failed to write file: {}", e))
}
