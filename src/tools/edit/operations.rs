use tokio::fs;

use super::diff::{format_diff_lines, generate_diff_summary};
use super::file_io::{read_file_lines, read_file_lines_or_create, write_file_lines};
use super::types::EditResult;

pub(super) async fn execute_edit(args: serde_json::Value) -> Result<EditResult, String> {
    let file_path = args["file_path"]
        .as_str()
        .ok_or_else(|| "file_path is required".to_string())?
        .to_string();

    let operation = args["operation"]
        .as_str()
        .ok_or_else(|| "operation is required".to_string())?
        .to_string();

    let dry_run = args["dry_run"].as_bool().unwrap_or(false);

    match operation.as_str() {
        "search_replace" => search_replace(&file_path, &args, dry_run).await,
        "insert" => insert_content(&file_path, &args, dry_run).await,
        "delete" => delete_content(&file_path, &args, dry_run).await,
        "append" => append_content(&file_path, &args, dry_run).await,
        _ => Err(format!("unknown operation: {}", operation)),
    }
}

/// 读取文件内容，返回 (内容, 行列表)
///

async fn search_replace(
    file_path: &str,
    args: &serde_json::Value,
    dry_run: bool,
) -> Result<EditResult, String> {
    let search = args["search"]
        .as_str()
        .ok_or_else(|| "search is required for search_replace operation".to_string())?;

    let replace = args["replace"]
        .as_str()
        .ok_or_else(|| "replace is required for search_replace operation".to_string())?;

    let (content, _) = read_file_lines(file_path).await?;

    // 计算搜索文本在内容中出现的次数
    let matches: Vec<_> = content.match_indices(search).collect();
    let match_count = matches.len();

    if match_count == 0 {
        return Err(format!(
            "搜索文本未在文件中找到。\n搜索文本:\n```\n{}\n```\n\n\
             请检查搜索文本是否与文件中内容完全一致（包括空格、缩进和换行）。",
            search
        ));
    }

    if match_count > 1 {
        return Err(format!(
            "搜索文本在文件中出现 {} 次，匹配不唯一。请提供更多上下文以确保唯一匹配。\n搜索文本:\n```\n{}\n```",
            match_count, search
        ));
    }

    // 唯一匹配，执行替换
    let new_content = content.replace(search, replace);

    let old_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let new_lines: Vec<String> = new_content.lines().map(|l| l.to_string()).collect();
    let diff_summary = generate_diff_summary(&new_lines, &old_lines);
    let diff = format_diff_lines(&old_lines, &new_lines);

    if !dry_run {
        fs::write(file_path, &new_content)
            .await
            .map_err(|e| format!("failed to write file: {}", e))?;
    }

    Ok(EditResult {
        operation: "search_replace".to_string(),
        file_path: file_path.to_string(),
        applied: !dry_run,
        dry_run,
        diff: Some(diff),
        summary: Some(diff_summary),
    })
}

/// 操作2: insert - 在指定位置插入内容
///
/// 文件不存在时自动创建（视为空文件），行号 1 有效。
async fn insert_content(
    file_path: &str,
    args: &serde_json::Value,
    dry_run: bool,
) -> Result<EditResult, String> {
    let content_str = args["content"]
        .as_str()
        .ok_or_else(|| "content is required for insert operation".to_string())?;

    let mode = args["mode"].as_str().unwrap_or("after");

    // ⭐ 使用 read_file_lines_or_create：文件不存在时视为空文件
    let (_, lines) = read_file_lines_or_create(file_path).await?;
    let original_lines = lines.clone();

    // 确定插入位置
    let insert_line: usize;

    if let Some(line_num) = args["line"].as_u64() {
        // 使用行号定位
        let line_num = line_num as usize;
        // ⭐ 允许 line_num == lines.len() + 1（在末尾插入），以及空文件时 line_num == 1
        if line_num == 0 || line_num > lines.len() + 1 {
            return Err(format!(
                "行号超出范围：文件共 {} 行，指定行号 {}（最大允许 {})",
                lines.len(),
                line_num,
                lines.len() + 1
            ));
        }
        insert_line = line_num;
    } else if let Some(search) = args["search"].as_str() {
        // 使用搜索文本定位
        if lines.is_empty() {
            return Err(format!(
                "文件为空，无法通过搜索文本定位插入位置：{}",
                search
            ));
        }
        let found = lines.iter().position(|l| l.contains(search));
        match found {
            Some(idx) => {
                insert_line = idx + 1; // 1-based
            }
            None => {
                return Err(format!("未找到包含指定文本的行：{}", search));
            }
        }
    } else {
        return Err("insert 操作需要提供 line 或 search 参数来定位插入位置".to_string());
    }

    // 执行插入（1-based 转 0-based）
    let insert_idx = if mode == "before" {
        insert_line - 1
    } else {
        insert_line // 在行后插入
    };

    let insert_lines: Vec<String> = content_str.lines().map(|l| l.to_string()).collect();
    let mut new_lines = Vec::with_capacity(lines.len() + insert_lines.len());

    for (i, line) in lines.iter().enumerate() {
        if i == insert_idx {
            if mode == "before" {
                new_lines.extend(insert_lines.clone());
                new_lines.push(line.clone());
            } else {
                new_lines.push(line.clone());
                new_lines.extend(insert_lines.clone());
            }
        } else {
            new_lines.push(line.clone());
        }
    }

    // ⭐ 如果是插入到末尾或更后（insert_idx >= lines.len()），直接追加
    if insert_idx >= lines.len() {
        new_lines.extend(insert_lines);
    }

    let diff_summary = generate_diff_summary(&new_lines, &original_lines);
    let diff = format_diff_lines(&original_lines, &new_lines);

    if !dry_run {
        write_file_lines(file_path, &new_lines).await?;
    }

    Ok(EditResult {
        operation: "insert".to_string(),
        file_path: file_path.to_string(),
        applied: !dry_run,
        dry_run,
        diff: Some(diff),
        summary: Some(diff_summary),
    })
}

/// 操作3: delete - 删除内容
async fn delete_content(
    file_path: &str,
    args: &serde_json::Value,
    dry_run: bool,
) -> Result<EditResult, String> {
    let (_, mut lines) = read_file_lines(file_path).await?;
    let original_lines = lines.clone();

    // 方式A: 通过 search 文本匹配删除
    if let Some(search) = args["search"].as_str() {
        let found = lines.iter().position(|l| l.contains(search));
        match found {
            Some(idx) => {
                let removed_line = lines.remove(idx);
                let diff_summary = format!("删除 1 行: {}", removed_line.trim());
                let diff = format_diff_lines(&original_lines, &lines);

                if !dry_run {
                    write_file_lines(file_path, &lines).await?;
                }

                return Ok(EditResult {
                    operation: "delete".to_string(),
                    file_path: file_path.to_string(),
                    applied: !dry_run,
                    dry_run,
                    diff: Some(diff),
                    summary: Some(diff_summary),
                });
            }
            None => {
                return Err(format!("未找到包含指定文本的行：{}", search));
            }
        }
    }

    // 方式B: 通过行范围删除
    let line_start = args["line_start"]
        .as_u64()
        .ok_or_else(|| "delete 操作需要提供 search 或 line_start/line_end 参数".to_string())?
        as usize;
    let line_end = args["line_end"].as_u64().unwrap_or(line_start as u64) as usize;

    if line_start < 1 || line_start > lines.len() {
        return Err(format!(
            "line_start 超出范围：文件共 {} 行，指定起始行 {}",
            lines.len(),
            line_start
        ));
    }
    if line_end < line_start || line_end > lines.len() {
        return Err(format!(
            "line_end 超出范围：文件共 {} 行，指定结束行 {}",
            lines.len(),
            line_end
        ));
    }

    // 删除行范围（1-based 转 0-based）
    let drain: Vec<String> = lines.drain((line_start - 1)..line_end).collect();
    let removed_count = drain.len();
    let diff_summary = format!(
        "删除 {} 行（第 {}-{} 行）",
        removed_count, line_start, line_end
    );
    let diff = format_diff_lines(&original_lines, &lines);

    if !dry_run {
        write_file_lines(file_path, &lines).await?;
    }

    Ok(EditResult {
        operation: "delete".to_string(),
        file_path: file_path.to_string(),
        applied: !dry_run,
        dry_run,
        diff: Some(diff),
        summary: Some(diff_summary),
    })
}

/// 操作4: append - 在文件末尾追加内容
///
/// 文件不存在时自动创建。
async fn append_content(
    file_path: &str,
    args: &serde_json::Value,
    dry_run: bool,
) -> Result<EditResult, String> {
    let content_str = args["content"]
        .as_str()
        .ok_or_else(|| "content is required for append operation".to_string())?;

    // ⭐ 使用 read_file_lines_or_create：文件不存在时视为空文件，自动创建
    let (_, mut lines) = read_file_lines_or_create(file_path).await?;
    let original_lines = lines.clone();

    let append_lines: Vec<String> = content_str.lines().map(|l| l.to_string()).collect();
    lines.extend(append_lines);

    let diff_summary = format!("追加 {} 行", content_str.lines().count());
    let diff = format_diff_lines(&original_lines, &lines);

    if !dry_run {
        write_file_lines(file_path, &lines).await?;
    }

    Ok(EditResult {
        operation: "append".to_string(),
        file_path: file_path.to_string(),
        applied: !dry_run,
        dry_run,
        diff: Some(diff),
        summary: Some(diff_summary),
    })
}
