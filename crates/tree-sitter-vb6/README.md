# tree-sitter-vb6

VB6 (Visual Basic 6) grammar for [tree-sitter](https://tree-sitter.github.io/).

Based on the grammar from [andersonm3ai/tree-sitter-vb6](https://github.com/andersonm3ai/tree-sitter-vb6) (MIT licensed).

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&tree_sitter_vb6::language()).unwrap();
let tree = parser.parse("Public Sub Main()\nEnd Sub", None).unwrap();
```
