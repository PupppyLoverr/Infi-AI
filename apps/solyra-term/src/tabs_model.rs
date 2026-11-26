//! Tab labelling, kept free of UI so it is unit-tested.

/// A tab's label: the running program or title, else "Terminal N".
pub fn label(id: &str, name: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.chars().take(24).collect();
    }
    let n = id.strip_prefix("tab-").unwrap_or(id);
    format!("Terminal {n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_a_numbered_name() {
        assert_eq!(label("tab-3", ""), "Terminal 3");
        assert_eq!(label("tab-3", "   "), "Terminal 3");
    }

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(label("tab-1", &"x".repeat(80)).chars().count(), 24);
        assert_eq!(label("tab-1", "nvim"), "nvim");
    }
}
