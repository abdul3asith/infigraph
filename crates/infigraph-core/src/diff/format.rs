use super::SymbolDiff;

pub fn format_diff(diff: &SymbolDiff) -> String {
    if diff.changes.is_empty() {
        return format!(
            "No symbol-level changes between '{}' and '{}'.",
            diff.old_ref, diff.new_ref
        );
    }

    let added = diff.added().count();
    let removed = diff.removed().count();
    let modified = diff.modified().count();

    let mut out = format!(
        "Semantic diff {} → {}  [+{} added  -{} removed  ~{} modified]\n\n",
        diff.old_ref, diff.new_ref, added, removed, modified
    );

    let mut cur_file = String::new();
    for c in &diff.changes {
        if c.file != cur_file {
            out.push_str(&format!("  {}\n", c.file));
            cur_file = c.file.clone();
        }
        let callers = if c.caller_count > 0 {
            format!("  [{} callers]", c.caller_count)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "    {:>20}  {:<10} {}{}\n",
            c.change.to_string(),
            c.kind,
            c.name,
            callers
        ));
    }

    out
}
