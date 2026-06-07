// src/tools/search_tool/mod.rs
//
// SearchTool - 目录文本搜索工具
//
// 功能：
//   - 在目录中搜索文本（支持递归）
//   - 支持正则表达式搜索
//   - 支持文件扩展名过滤
//   - 支持大小写敏感/不敏感
//   - 限制最大匹配行数
//   - 显示匹配行号及上下文

use std::path::Path;

use regex::Regex;
use tokio::{sync::mpsc, task};
use tokio_stream::wrappers::ReceiverStream;
use walkdir::WalkDir;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct SearchTool;

const DEFAULT_MAX_RESULTS: usize = 50;

impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "在目录中搜索文本内容，支持正则表达式、文件扩展名过滤、大小写控制。适合代码搜索和问题定位。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search",
                "description": "在目录中搜索文本。支持多文件递归搜索，返回匹配的文件路径、行号和内容。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "搜索模式。默认作为普通文本搜索，设置 regex=true 时作为正则表达式"
                        },
                        "path": {
                            "type": "string",
                            "description": "搜索起始目录，默认为当前工作目录",
                            "default": "."
                        },
                        "include_ext": {
                            "type": "string",
                            "description": "只搜索指定扩展名的文件，逗号分隔。例如：\"rs,toml,md\"。不指定则搜索所有文件"
                        },
                        "exclude_dirs": {
                            "type": "string",
                            "description": "要排除的目录名，逗号分隔。默认排除 target,.git,.agent,node_modules",
                            "default": "target,.git,.agent,node_modules"
                        },
                        "regex": {
                            "type": "boolean",
                            "description": "是否将 pattern 作为正则表达式处理，默认为 false",
                            "default": false
                        },
                        "ignore_case": {
                            "type": "boolean",
                            "description": "是否忽略大小写，默认为 true",
                            "default": true
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "最大匹配行数，超出的会被截断。默认 50",
                            "default": 50,
                            "minimum": 1,
                            "maximum": 500
                        },
                        "context_lines": {
                            "type": "integer",
                            "description": "匹配行上下文的行数，默认为 0（只显示匹配行）",
                            "default": 0,
                            "minimum": 0,
                            "maximum": 10
                        }
                    },
                    "required": ["pattern"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let result = execute_search(args).await;
            let event = match result {
                Ok(output) => ToolEvent::Done(serde_json::json!(output)),
                Err(err) => ToolEvent::Err(err),
            };
            let _ = tx.send(event).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}

#[derive(Debug, serde::Serialize)]
struct SearchMatch {
    file: String,
    line: usize,
    content: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct SearchOutput {
    pattern: String,
    path: String,
    file_count: usize,
    match_count: usize,
    truncated: bool,
    matches: Vec<SearchMatch>,
}

async fn execute_search(args: serde_json::Value) -> Result<SearchOutput, String> {
    let pattern_str = args["pattern"]
        .as_str()
        .ok_or_else(|| "pattern is required".to_string())?
        .to_string();

    let search_path = args["path"].as_str().unwrap_or(".").to_string();
    let is_regex = args["regex"].as_bool().unwrap_or(false);
    let ignore_case = args["ignore_case"].as_bool().unwrap_or(true);
    let max_results = args["max_results"].as_u64().unwrap_or(DEFAULT_MAX_RESULTS as u64) as usize;
    let context_lines = args["context_lines"].as_u64().unwrap_or(0) as usize;

    // 解析包含的扩展名
    let include_ext: Option<Vec<String>> = args.get("include_ext").and_then(|v| v.as_str()).map(|s| {
        s.split(',')
            .map(|ext| ext.trim().trim_start_matches('.').to_lowercase())
            .collect()
    });

    // 解析排除的目录
    let exclude_dirs: Vec<String> = args
        .get("exclude_dirs")
        .and_then(|v| v.as_str())
        .unwrap_or("target,.git,.agent,node_modules")
        .split(',')
        .map(|d| d.trim().to_string())
        .collect();

    // 编译搜索模式
    let search_pattern: Box<dyn Fn(&str) -> bool + Send + Sync> = if is_regex {
        let mut regex_str = pattern_str.clone();
        let re = if ignore_case {
            Regex::new(&format!("(?i){}", regex_str)).map_err(|e| format!("正则表达式无效: {}", e))?
        } else {
            Regex::new(&regex_str).map_err(|e| format!("正则表达式无效: {}", e))?
        };
        Box::new(move |text: &str| -> bool { re.is_match(text) })
    } else {
        let lower_pattern = pattern_str.to_lowercase();
        if ignore_case {
            Box::new(move |text: &str| -> bool { text.to_lowercase().contains(&lower_pattern) })
        } else {
            let p = pattern_str.clone();
            Box::new(move |text: &str| -> bool { text.contains(&p) })
        }
    };

    let search_path_buf = std::path::PathBuf::from(&search_path);
    if !search_path_buf.exists() {
        return Err(format!("搜索路径不存在: {}", search_path));
    }

    // 使用 spawn_blocking 进行文件系统扫描（避免阻塞异步运行时）
    let include_ext_clone = include_ext.clone();
    let exclude_dirs_clone = exclude_dirs.clone();

    let result = task::spawn_blocking(move || {
        let mut matches: Vec<SearchMatch> = Vec::new();
        let mut files_with_matches: std::collections::HashSet<String> = std::collections::HashSet::new();

        for entry in WalkDir::new(&search_path_buf)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let dir_name = e.file_name().to_string_lossy().to_string();
                    !exclude_dirs_clone.contains(&dir_name)
                } else {
                    true
                }
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if matches.len() >= max_results {
                break;
            }

            let file_path = entry.path();

            // 扩展名过滤
            if let Some(ref exts) = include_ext_clone {
                let ext = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_default();
                if !exts.contains(&ext) {
                    continue;
                }
            }

            // 跳过二进制文件和大文件
            let metadata = match std::fs::metadata(file_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.len() > 1024 * 1024 {
                // 跳过超过 1MB 的文件
                continue;
            }

            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue, // 跳过二进制文件
            };

            let lines: Vec<&str> = content.lines().collect();
            let file_path_str = file_path.to_string_lossy().to_string();

            for (i, line) in lines.iter().enumerate() {
                if matches.len() >= max_results {
                    break;
                }

                if search_pattern(line) {
                    let line_num = i + 1; // 1-based

                    // 上下文行
                    let before: Vec<String> = if context_lines > 0 {
                        let start = if i >= context_lines { i - context_lines } else { 0 };
                        lines[start..i]
                            .iter()
                            .enumerate()
                            .map(|(j, l)| format!("{:>6} | {}", start + j + 1, l))
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let after: Vec<String> = if context_lines > 0 {
                        let end = (i + context_lines + 1).min(lines.len());
                        lines[(i + 1)..end]
                            .iter()
                            .enumerate()
                            .map(|(j, l)| format!("{:>6} | {}", i + 2 + j, l))
                            .collect()
                    } else {
                        Vec::new()
                    };

                    matches.push(SearchMatch {
                        file: file_path_str.clone(),
                        line: line_num,
                        content: line.to_string(),
                        context_before: before,
                        context_after: after,
                    });
                    files_with_matches.insert(file_path_str.clone());
                }
            }
        }

        (matches, files_with_matches)
    })
    .await
    .map_err(|e| format!("搜索任务失败: {}", e))?;

    let (matches, files_with_matches) = result;
    let truncated = matches.len() >= max_results;

    Ok(SearchOutput {
        pattern: pattern_str,
        path: search_path,
        file_count: files_with_matches.len(),
        match_count: matches.len(),
        truncated,
        matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_search_no_pattern() {
        let args = serde_json::json!({
            "path": "."
        });
        let result = execute_search(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_search_invalid_path() {
        let args = serde_json::json!({
            "pattern": "test",
            "path": "/tmp/nonexistent_dir_xyz"
        });
        let result = execute_search(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不存在"));
    }

    #[test]
    fn test_search_pattern_plain() {
        let pattern_text = "hello".to_string();
        let search = |text: &str| -> bool { text.contains(&pattern_text) };
        assert!(search("hello world"));
        assert!(!search("Hello World"));
    }

    #[test]
    fn test_search_pattern_case_insensitive() {
        let lower = "hello".to_string();
        let search = |text: &str| -> bool { text.to_lowercase().contains(&lower) };
        assert!(search("Hello World"));
        assert!(search("hello world"));
        assert!(!search("hi world"));
    }
}
