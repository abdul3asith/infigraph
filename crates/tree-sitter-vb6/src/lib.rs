use tree_sitter::Language;

extern "C" {
    fn tree_sitter_vb6() -> Language;
}

/// Get the tree-sitter [Language] for VB6.
pub fn language() -> Language {
    unsafe { tree_sitter_vb6() }
}

/// The content of the `node-types.json` file for this grammar.
pub const NODE_TYPES: &str = include_str!("node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::language())
            .expect("Error loading VB6 grammar");
    }
}
