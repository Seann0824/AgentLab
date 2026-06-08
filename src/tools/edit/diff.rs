pub(super) fn generate_diff_summary(lines: &[String], old_lines: &[String]) -> String {
    let added = lines.len() as isize - old_lines.len() as isize;
    let _changed = if added >= 0 { added } else { -added };
    let change_type = if added > 0 {
        format!("+{} 行", added)
    } else if added < 0 {
        format!("-{} 行", -added)
    } else {
        format!("{} 行（内容变更）", old_lines.len())
    };
    change_type
}

pub(super) fn format_diff_lines(old_lines: &[String], new_lines: &[String]) -> String {
    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n", "file"));
    diff.push_str(&format!("+++ b/{}\n", "file"));

    // 简单地用行号范围标识
    let min_len = old_lines.len().min(new_lines.len());
    let max_len = old_lines.len().max(new_lines.len());
    let start = 1;
    let _end = max_len;

    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        start,
        old_lines.len(),
        start,
        new_lines.len()
    ));

    for i in 0..max_len {
        let old_line = if i < old_lines.len() {
            &old_lines[i]
        } else {
            ""
        };
        let new_line = if i < new_lines.len() {
            &new_lines[i]
        } else {
            ""
        };

        if i < min_len {
            if old_line != new_line {
                diff.push_str(&format!("-{}\n", old_line));
                diff.push_str(&format!("+{}\n", new_line));
            } else {
                diff.push_str(&format!(" {}\n", old_line));
            }
        } else if i >= old_lines.len() {
            // Added line
            diff.push_str(&format!("+{}\n", new_line));
        } else {
            // Removed line
            diff.push_str(&format!("-{}\n", old_line));
        }
    }

    diff
}
