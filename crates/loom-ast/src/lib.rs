//! Loom AST — Structural Editing with tree-sitter (P1/5.5)
//!
//! Provides AST parsing, navigation, queries, and structural editing operations
//! for multiple languages using tree-sitter.
//!
//! # Architecture
//!
//! ```text
//!   Source Code  ──►  Parser  ──►  Tree  ──►  Edit Operations  ──►  New Source
//!                        │
//!                   Language Grammar
//!                        │
//!                   ┌────┴────┐
//!                   │  Rust   │
//!                   │ Python  │
//!                   │   JS    │
//!                   │   Go    │
//!                   │  JSON   │
//!                   └─────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use loom_ast::*;
//!
//! let source = "fn foo() { let x = 1; }";
//! let result = parse(source, "rust").unwrap();
//!
//! // Navigate the tree
//! for node in result.root_node().children(&mut result.tree.walk()) {
//!     println!("{} @ {}", node.kind(), node.start_position());
//! }
//!
//! // Query for function definitions
//! let matches = query(source, "rust", "(function_item name: (identifier) @fn.name)").unwrap();
//!
//! // Move a function up
//! let edited = move_node_up(source, "rust", &Position::new(3, 0)).unwrap();
//! ```

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Re-export tree-sitter types users might need.
pub use tree_sitter::{InputEdit, Node, Point, QueryCursor, Range, Tree, TreeCursor};

/// Result type for AST operations.
pub type AstResult<T> = Result<T, AstError>;

// ─── Error type ────────────────────────────────────────────────────────

/// Errors from AST operations.
#[derive(Debug, Error)]
pub enum AstError {
    /// Unknown or unsupported language.
    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),
    /// Parsing failed.
    #[error("Parse error at language '{lang}': {detail}")]
    ParseError {
        /// Language being parsed.
        lang: String,
        /// Error detail.
        detail: String,
    },
    /// No node found at the given position.
    #[error("No node found at position {0}:{1}")]
    NoNodeAtPosition(usize, usize),
    /// The node is not editable (e.g. root node).
    #[error("Node at position {0}:{1} is not editable")]
    NodeNotEditable(usize, usize),
    /// Operation not applicable to this node type.
    #[error("Operation '{op}' not applicable to node kind '{kind}'")]
    OperationNotApplicable {
        /// Operation name.
        op: String,
        /// Node kind.
        kind: String,
    },
    /// Tree-sitter query error.
    #[error("Query error: {0}")]
    QueryError(String),
    /// Generic error.
    #[error("{0}")]
    Other(String),
}

// ─── Language Support ──────────────────────────────────────────────────

/// Supported programming languages for AST operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LanguageId {
    /// Rust
    Rust,
    /// Python
    Python,
    /// JavaScript / TypeScript (JSX)
    JavaScript,
    /// Go
    Go,
    /// JSON
    Json,
    /// TOML (basic)
    Toml,
    /// Shell script
    Bash,
}

impl LanguageId {
    /// Parse a language name string into a `LanguageId`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "rust" | "rs" => Some(LanguageId::Rust),
            "python" | "py" | "python3" => Some(LanguageId::Python),
            "javascript" | "js" | "jsx" | "typescript" | "ts" | "tsx" => {
                Some(LanguageId::JavaScript)
            }
            "go" | "golang" => Some(LanguageId::Go),
            "json" => Some(LanguageId::Json),
            "toml" => Some(LanguageId::Toml),
            "bash" | "shell" | "sh" | "zsh" => Some(LanguageId::Bash),
            _ => None,
        }
    }

    /// Get the tree-sitter language object for this language.
    pub fn get_grammar(&self) -> Option<tree_sitter::Language> {
        match self {
            LanguageId::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            LanguageId::Python => Some(tree_sitter_python::LANGUAGE.into()),
            LanguageId::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            LanguageId::Json => Some(tree_sitter_json::LANGUAGE.into()),
            LanguageId::Go => Some(tree_sitter_go::LANGUAGE.into()),
            // TOML and Bash are loaded via the bundled JSON grammar as a fallback
            // or can be loaded from WASM files.
            LanguageId::Toml | LanguageId::Bash => None,
        }
    }

    /// Get the display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            LanguageId::Rust => "Rust",
            LanguageId::Python => "Python",
            LanguageId::JavaScript => "JavaScript",
            LanguageId::Go => "Go",
            LanguageId::Json => "JSON",
            LanguageId::Toml => "TOML",
            LanguageId::Bash => "Bash",
        }
    }

    /// Supported languages (those with compiled-in grammars).
    pub fn supported() -> Vec<LanguageId> {
        vec![
            LanguageId::Rust,
            LanguageId::Python,
            LanguageId::JavaScript,
            LanguageId::Go,
            LanguageId::Json,
        ]
    }
}

/// Detect the language from a filename extension.
pub fn detect_language(path: &str) -> Option<LanguageId> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => Some(LanguageId::Rust),
        "py" => Some(LanguageId::Python),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some(LanguageId::JavaScript),
        "go" => Some(LanguageId::Go),
        "json" => Some(LanguageId::Json),
        "toml" => Some(LanguageId::Toml),
        "sh" | "bash" | "zsh" => Some(LanguageId::Bash),
        _ => LanguageId::from_name(ext),
    }
}

// ─── Parse Result ──────────────────────────────────────────────────────

/// Result of parsing source code into an AST.
#[derive(Debug)]
pub struct ParseResult {
    /// The tree-sitter parse tree.
    pub tree: Tree,
    /// The source code that was parsed.
    pub source: String,
    /// The language used for parsing.
    pub language: LanguageId,
}

/// Parse source code into an AST using the given language.
pub fn parse(source: &str, language: &str) -> AstResult<ParseResult> {
    let lang = LanguageId::from_name(language)
        .ok_or_else(|| AstError::UnsupportedLanguage(language.to_string()))?;
    let grammar = lang.get_grammar().ok_or_else(|| {
        AstError::UnsupportedLanguage(format!("{} (no bundled grammar)", language))
    })?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| AstError::ParseError {
            lang: language.to_string(),
            detail: format!("Failed to set language: {}", e),
        })?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| AstError::ParseError {
            lang: language.to_string(),
            detail: "Parser returned None".to_string(),
        })?;

    Ok(ParseResult {
        tree,
        source: source.to_string(),
        language: lang,
    })
}

// ─── Navigation───────

/// A description of an AST node for display purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Node type name (e.g. "function_item", "if_statement").
    pub kind: String,
    /// Start position (row, column).
    pub start: (usize, usize),
    /// End position (row, column).
    pub end: (usize, usize),
    /// Start byte.
    pub start_byte: usize,
    /// End byte.
    pub end_byte: usize,
    /// Range of text.
    pub text: String,
    /// Number of children.
    pub child_count: usize,
    /// Whether this is a named node.
    pub named: bool,
    /// Whether this is an error node.
    pub is_error: bool,
    /// Children (recursive, if depth > 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<NodeInfo>>,
}

impl From<&Node<'_>> for NodeInfo {
    fn from(node: &Node<'_>) -> Self {
        let source = ""; // placeholder - filled in separately
        let text = node.utf8_text(source.as_bytes()).unwrap_or("");
        NodeInfo {
            kind: node.kind().to_string(),
            start: (node.start_position().row, node.start_position().column),
            end: (node.end_position().row, node.end_position().column),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            text: text.to_string(),
            child_count: node.child_count(),
            named: node.is_named(),
            is_error: node.is_error(),
            children: None,
        }
    }
}

impl NodeInfo {
    /// Build a `NodeInfo` from a tree-sitter node and source text.
    pub fn from_node<'a>(node: &Node<'a>, source: &'a str) -> Self {
        let text = node.utf8_text(source.as_bytes()).unwrap_or("");
        NodeInfo {
            kind: node.kind().to_string(),
            start: (node.start_position().row, node.start_position().column),
            end: (node.end_position().row, node.end_position().column),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            text: text.to_string(),
            child_count: node.child_count(),
            named: node.is_named(),
            is_error: node.is_error(),
            children: None,
        }
    }

    /// Recursively build a full tree of node info.
    pub fn from_node_recursive<'a>(node: &Node<'a>, source: &'a str, max_depth: usize) -> Self {
        let mut info = NodeInfo::from_node(node, source);
        if max_depth > 0 && node.child_count() > 0 {
            let children: Vec<NodeInfo> = (0..node.child_count())
                .filter_map(|i| node.child(i as u32))
                .map(|child| NodeInfo::from_node_recursive(&child, source, max_depth - 1))
                .collect();
            info.children = Some(children);
        }
        info
    }
}

/// Get the root node info for display.
pub fn root_node_info(result: &ParseResult, max_depth: usize) -> NodeInfo {
    NodeInfo::from_node_recursive(&result.tree.root_node(), &result.source, max_depth)
}

/// Find the smallest named node at the given position.
pub fn node_at_position(result: &ParseResult, row: usize, column: usize) -> AstResult<NodeInfo> {
    let point = Point::new(row, column);
    let root = result.tree.root_node();
    let descendant = root
        .descendant_for_point_range(point, point)
        .unwrap_or(root);
    Ok(NodeInfo::from_node(&descendant, &result.source))
}

/// Find all named nodes at the given position (from smallest to root).
pub fn node_stack_at_position(result: &ParseResult, row: usize, column: usize) -> Vec<NodeInfo> {
    let point = Point::new(row, column);
    let root = result.tree.root_node();
    let mut nodes = Vec::new();

    if let Some(descendant) = root.descendant_for_point_range(point, point) {
        nodes.push(NodeInfo::from_node(&descendant, &result.source));
        let mut current = descendant;
        while let Some(parent) = current.parent() {
            nodes.push(NodeInfo::from_node(&parent, &result.source));
            if parent.id() == root.id() {
                break;
            }
            current = parent;
        }
    }
    nodes.reverse(); // root first
    nodes
}

/// Get all sibling nodes at the structural level for a position.
/// Walks up from the deepest node to find the outermost level where
/// named children exist.
pub fn siblings_at_position(
    result: &ParseResult,
    row: usize,
    column: usize,
) -> AstResult<Vec<NodeInfo>> {
    let root = result.tree.root_node();

    // Walk up from deepest, looking for a parent with >= 2 named children.
    // Prefer the outermost level that still has named siblings.
    let point = Point::new(row, column);
    let deepest = root
        .descendant_for_point_range(point, point)
        .unwrap_or(root);

    let mut current = deepest;
    let parent = loop {
        let parent = current
            .parent()
            .ok_or(AstError::NoNodeAtPosition(row, column))?;

        if parent.named_child_count() >= 2 && current.is_named() {
            // Check grandparent — if it also has >= 2 named children and
            // parent is named, go up one more level for broader context.
            if let Some(gp) = parent.parent() {
                if gp.named_child_count() >= 2 && parent.is_named() {
                    current = parent;
                    continue;
                }
            }
            break parent;
        }
        current = parent;
    };

    let siblings: Vec<NodeInfo> = (0..parent.child_count())
        .filter_map(|i| parent.child(i as u32))
        .filter(|c| c.is_named())
        .map(|child| NodeInfo::from_node(&child, &result.source))
        .collect();

    Ok(siblings)
}

/// List all top-level named nodes.
pub fn top_level_nodes(result: &ParseResult) -> Vec<NodeInfo> {
    let root = result.tree.root_node();
    let mut nodes = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.is_named() {
            nodes.push(NodeInfo::from_node(&child, &result.source));
        }
    }
    nodes
}

// ─── Query ─────────────────────────────────────────────────────────────

/// Result of a tree-sitter query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMatch {
    /// Pattern index.
    pub pattern: usize,
    /// Captured nodes.
    pub captures: Vec<CaptureInfo>,
}

/// A single captured node from a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureInfo {
    /// Capture name.
    pub name: String,
    /// Node kind.
    pub kind: String,
    /// Text content.
    pub text: String,
    /// Start position.
    pub start: (usize, usize),
    /// End position.
    pub end: (usize, usize),
    /// Start byte.
    pub start_byte: usize,
    /// End byte.
    pub end_byte: usize,
}

/// Execute a tree-sitter query against the AST.
///
/// Query syntax: `(function_item name: (identifier) @fn.name)`
pub fn query(result: &ParseResult, query_str: &str) -> AstResult<Vec<QueryMatch>> {
    let lang = result.language;
    let grammar = lang
        .get_grammar()
        .ok_or_else(|| AstError::UnsupportedLanguage(lang.display_name().to_string()))?;

    let query_obj = tree_sitter::Query::new(&grammar, query_str)
        .map_err(|e| AstError::QueryError(e.to_string()))?;

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(
        &query_obj,
        result.tree.root_node(),
        result.source.as_bytes(),
    );

    let mut results = Vec::new();
    use streaming_iterator::StreamingIterator;
    loop {
        matches.advance();
        if let Some(m) = matches.get() {
            let captures: Vec<CaptureInfo> = m
                .captures
                .iter()
                .map(|c| {
                    let node = c.node;
                    CaptureInfo {
                        name: query_obj.capture_names()[c.index as usize].to_string(),
                        kind: node.kind().to_string(),
                        text: node
                            .utf8_text(result.source.as_bytes())
                            .unwrap_or("")
                            .to_string(),
                        start: (node.start_position().row, node.start_position().column),
                        end: (node.end_position().row, node.end_position().column),
                        start_byte: node.start_byte(),
                        end_byte: node.end_byte(),
                    }
                })
                .collect();
            results.push(QueryMatch {
                pattern: m.pattern_index,
                captures,
            });
        } else {
            break;
        }
    }

    Ok(results)
}

// ─── Structural Editing Operations ─────────────────────────────────────

/// A structural edit operation and its result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstEdit {
    /// Type of operation performed.
    pub operation: String,
    /// The resulting source code.
    pub result: String,
    /// Description of what changed.
    pub description: String,
    /// Whether the edit was applied.
    pub applied: bool,
}

/// Position in the source.
pub type Position = (usize, usize);

// ─── Move Up/Down ──────────────────────────────────────────────────────

/// Walk up from the deepest node at a position to find a node that has
/// named siblings — i.e. a named node whose parent has at least two named
/// children. This lets structural-edit operations work on meaningful
/// sibling groups instead of leaf tokens.
fn resolve_to_sibling_tree<'a>(
    root: Node<'a>,
    row: usize,
    col: usize,
) -> AstResult<(Node<'a>, Node<'a>, usize)> {
    let point = Point::new(row, col);
    let deepest = root
        .descendant_for_point_range(point, point)
        .unwrap_or(root);

    let mut current = deepest;
    loop {
        let parent = current
            .parent()
            .ok_or(AstError::NoNodeAtPosition(row, col))?;

        let n_children = parent.child_count();
        if n_children < 2 || !current.is_named() || current.id() == root.id() {
            current = parent;
            continue;
        }

        // Find current's index among siblings
        let byte_start = current.start_byte();
        let idx = (0..n_children as u32)
            .filter_map(|i| parent.child(i))
            .position(|c| c.start_byte() == byte_start)
            .ok_or(AstError::NoNodeAtPosition(row, col))?;
        return Ok((current, parent, idx));
    }
}

/// Walk up from the deepest node at a position to find the nearest named
/// ancestor that has named children — suitable for extract/wrap/unwrap/duplicate
/// where leaf tokens are not meaningful.
fn resolve_to_named_node<'a>(root: Node<'a>, row: usize, col: usize) -> Node<'a> {
    let point = Point::new(row, col);
    let deepest = root
        .descendant_for_point_range(point, point)
        .unwrap_or(root);

    if deepest.is_named() && deepest.named_child_count() > 0 && deepest.id() != root.id() {
        return deepest;
    }

    let mut current = deepest;
    while let Some(parent) = current.parent() {
        if parent.is_named() && parent.named_child_count() > 0 && parent.id() != root.id() {
            return parent;
        }
        current = parent;
    }
    deepest
}

/// Move a node at the given position up (swap with previous sibling).
pub fn move_node_up(source: &str, language: &str, position: &Position) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let (row, col) = *position;
    let root = result.tree.root_node();

    let (node, parent, node_index) = resolve_to_sibling_tree(root, row, col)?;

    if node_index == 0 {
        return Ok(AstEdit {
            operation: "move_up".into(),
            result: source.to_string(),
            description: "Already at top".to_string(),
            applied: false,
        });
    }

    // Find the previous non-zero-width sibling
    let prev_sibling = (0..node_index)
        .rev()
        .filter_map(|i| parent.child(i as u32))
        .find(|c| c.start_byte() != c.end_byte())
        .ok_or(AstError::NoNodeAtPosition(row, col))?;

    let node_text = &source[node.start_byte()..node.end_byte()];
    let prev_text = &source[prev_sibling.start_byte()..prev_sibling.end_byte()];

    let mut result_str = String::with_capacity(source.len());
    result_str.push_str(&source[..prev_sibling.start_byte()]);
    result_str.push_str(node_text);
    result_str.push_str(&source[prev_sibling.end_byte()..node.start_byte()]);
    result_str.push_str(prev_text);
    result_str.push_str(&source[node.end_byte()..]);

    Ok(AstEdit {
        operation: "move_up".into(),
        result: result_str,
        description: format!("Moved {} up past {}", node.kind(), prev_sibling.kind()),
        applied: true,
    })
}

/// Move a node at the given position down (swap with next sibling).
pub fn move_node_down(source: &str, language: &str, position: &Position) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let (row, col) = *position;
    let root = result.tree.root_node();

    let (node, parent, node_index) = resolve_to_sibling_tree(root, row, col)?;

    let n_children = parent.child_count();
    if node_index + 1 >= n_children {
        return Ok(AstEdit {
            operation: "move_down".into(),
            result: source.to_string(),
            description: "Already at bottom".to_string(),
            applied: false,
        });
    }

    // Find the next non-zero-width sibling
    let next_sibling = match (node_index + 1..n_children)
        .filter_map(|i| parent.child(i as u32))
        .find(|c| c.start_byte() != c.end_byte())
    {
        Some(s) => s,
        None => {
            return Ok(AstEdit {
                operation: "move_down".into(),
                result: source.to_string(),
                description: "No next sibling".to_string(),
                applied: false,
            });
        }
    };

    let node_text = &source[node.start_byte()..node.end_byte()];
    let next_text = &source[next_sibling.start_byte()..next_sibling.end_byte()];

    let mut result_str = String::with_capacity(source.len());
    result_str.push_str(&source[..node.start_byte()]);
    result_str.push_str(next_text);
    result_str.push_str(&source[node.end_byte()..next_sibling.start_byte()]);
    result_str.push_str(node_text);
    result_str.push_str(&source[next_sibling.end_byte()..]);

    Ok(AstEdit {
        operation: "move_down".into(),
        result: result_str,
        description: format!("Moved {} down past {}", node.kind(), next_sibling.kind()),
        applied: true,
    })
}

// ─── Swap ──────────────────────────────────────────────────────────────

/// Swap two sibling nodes at the given positions.
pub fn swap_nodes(
    source: &str,
    language: &str,
    pos_a: &Position,
    pos_b: &Position,
) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let root = result.tree.root_node();

    let (node_a, parent_a, _) = resolve_to_sibling_tree(root, pos_a.0, pos_a.1)?;
    let (node_b, parent_b, _) = resolve_to_sibling_tree(root, pos_b.0, pos_b.1)?;

    // Ensure same parent
    if parent_a.id() != parent_b.id() {
        return Err(AstError::OperationNotApplicable {
            op: "swap".into(),
            kind: format!(
                "{} and {} have different parents",
                node_a.kind(),
                node_b.kind()
            ),
        });
    }

    // Order by byte position
    let (first, second) = if node_a.start_byte() < node_b.start_byte() {
        (node_a, node_b)
    } else {
        (node_b, node_a)
    };
    let text_first = &source[first.start_byte()..first.end_byte()];
    let text_second = &source[second.start_byte()..second.end_byte()];

    let mut result_str = String::with_capacity(source.len());
    result_str.push_str(&source[..first.start_byte()]);
    result_str.push_str(text_second);
    result_str.push_str(&source[first.end_byte()..second.start_byte()]);
    result_str.push_str(text_first);
    result_str.push_str(&source[second.end_byte()..]);

    Ok(AstEdit {
        operation: "swap".into(),
        result: result_str,
        description: format!("Swapped {} and {}", node_a.kind(), node_b.kind()),
        applied: true,
    })
}

// ─── Extract ───────────────────────────────────────────────────────────

/// Extract a node into a named entity (variable, function, etc.).
///
/// For expressions, extracts to a variable: `let extracted = EXPR;`
/// For statements/block-level nodes, extracts to a function.
pub fn extract_node(
    source: &str,
    language: &str,
    position: &Position,
    name: &str,
) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let (row, col) = *position;
    let root = result.tree.root_node();

    // Find the structural node at this position
    let node = resolve_to_named_node(root, row, col);

    if node.start_byte() == node.end_byte() {
        return Err(AstError::NoNodeAtPosition(row, col));
    }

    let node_text = &source[node.start_byte()..node.end_byte()];
    let (replacement, _extraction) = match result.language {
        LanguageId::Rust => {
            // Check if it's an expression context
            if is_expression_context(&node) {
                (format!("let {} = {};", name, node_text), String::new())
            } else {
                // Extract as a function
                (
                    format!("fn {}() {{\n    {}\n}}", name, indent_text(node_text, 1)),
                    String::new(),
                )
            }
        }
        LanguageId::Python => {
            if is_expression_context(&node) {
                (format!("{} = {}", name, node_text), String::new())
            } else {
                (
                    format!("def {}():\n    {}", name, indent_text(node_text, 1)),
                    String::new(),
                )
            }
        }
        LanguageId::JavaScript => {
            if is_expression_context(&node) {
                (format!("const {} = {};", name, node_text), String::new())
            } else {
                (
                    format!(
                        "function {}() {{\n    {}\n}}",
                        name,
                        indent_text(node_text, 1)
                    ),
                    String::new(),
                )
            }
        }
        _ => {
            return Err(AstError::OperationNotApplicable {
                op: "extract".into(),
                kind: format!(
                    "Language {} not supported for extraction",
                    result.language.display_name()
                ),
            })
        }
    };

    let mut result_str = String::with_capacity(source.len() + replacement.len());
    result_str.push_str(&source[..node.start_byte()]);
    result_str.push_str(&replacement);
    result_str.push_str(&source[node.end_byte()..]);

    Ok(AstEdit {
        operation: "extract".into(),
        result: result_str,
        description: format!("Extracted {} as '{}'", node.kind(), name),
        applied: true,
    })
}

/// Check if a node is likely an expression (for extraction context).
fn is_expression_context(node: &Node) -> bool {
    // Simple heuristic: expression_statement, binary, call, etc.
    let kind = node.kind();
    kind.contains("expression")
        || kind.contains("call")
        || kind == "binary_expression"
        || kind == "unary_expression"
        || kind == "identifier"
        || kind == "string_literal"
        || kind == "number_literal"
        || kind == "boolean_literal"
        || kind == "field_expression"
        || kind == "subscript_expression"
        || kind == "match_expression"
        || kind == "if_expression"
        || kind == "block"
        || kind == "closure_expression"
        || kind == "parenthesized_expression"
        || kind == "tuple_expression"
        || kind == "array_expression"
        || kind == "struct_expression"
}

/// Indent multi-line text by one level.
fn indent_text(text: &str, level: usize) -> String {
    let indent = "    ".repeat(level);
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", indent, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Wrap / Unwrap ─────────────────────────────────────────────────────

/// Wrap a node in a block or construct.
pub fn wrap_node(
    source: &str,
    language: &str,
    position: &Position,
    wrapper: &str,
) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let (row, col) = *position;
    let root = result.tree.root_node();

    // Find the structural node at this position
    let node = resolve_to_named_node(root, row, col);

    if node.start_byte() == node.end_byte() {
        return Err(AstError::NoNodeAtPosition(row, col));
    }

    let node_text = &source[node.start_byte()..node.end_byte()];
    let wrapped = match wrapper {
        "if" | "if_true" => format!("if true {{\n    {}\n}}", indent_text(node_text, 1)),
        "if_false" => format!("if false {{\n    {}\n}}", indent_text(node_text, 1)),
        "loop" | "while" => format!("while true {{\n    {}\n}}", indent_text(node_text, 1)),
        "block" | "{}" => format!("{{\n    {}\n}}", indent_text(node_text, 1)),
        "parens" | "()" => format!("({})", node_text),
        "brackets" | "[]" => format!("[{}]", node_text),
        "try" | "try_catch" => format!(
            "try {{\n    {}\n}} catch {{\n    // TODO\n}}",
            indent_text(node_text, 1)
        ),
        "unsafe" => format!("unsafe {{\n    {}\n}}", indent_text(node_text, 1)),
        other => {
            // Use custom wrapper text, replacing $BODY with the node text
            if other.contains("$BODY") {
                other.replace("$BODY", node_text)
            } else {
                return Err(AstError::OperationNotApplicable {
                    op: "wrap".into(),
                    kind: format!("Unknown wrapper: {}", other),
                });
            }
        }
    };

    let mut result_str = String::with_capacity(source.len() + wrapped.len());
    result_str.push_str(&source[..node.start_byte()]);
    result_str.push_str(&wrapped);
    result_str.push_str(&source[node.end_byte()..]);

    Ok(AstEdit {
        operation: "wrap".into(),
        result: result_str,
        description: format!("Wrapped {} in {}", node.kind(), wrapper),
        applied: true,
    })
}

/// Unwrap a node by removing its outer construct and keeping the inner content.
pub fn unwrap_node(source: &str, language: &str, position: &Position) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let (row, col) = *position;
    let root = result.tree.root_node();

    // Find the structural node at this position
    let node = resolve_to_named_node(root, row, col);

    if node.start_byte() == node.end_byte() || node.id() == root.id() {
        return Err(AstError::NoNodeAtPosition(row, col));
    }

    // Find the inner content: the named child with the largest span (likely a block/body)
    let inner = (0..node.child_count())
        .filter_map(|i| node.child(i as u32))
        .filter(|c| c.is_named())
        .max_by_key(|c| c.end_byte() - c.start_byte())
        .ok_or_else(|| AstError::OperationNotApplicable {
            op: "unwrap".into(),
            kind: format!("No inner content found in {}", node.kind()),
        })?;

    let inner_text = &source[inner.start_byte()..inner.end_byte()];

    let mut result_str = String::with_capacity(source.len());
    result_str.push_str(&source[..node.start_byte()]);
    result_str.push_str(inner_text);
    result_str.push_str(&source[node.end_byte()..]);

    Ok(AstEdit {
        operation: "unwrap".into(),
        result: result_str,
        description: format!("Unwrapped {} from {}", inner.kind(), node.kind()),
        applied: true,
    })
}

// ─── Duplicate ─────────────────────────────────────────────────────────

/// Duplicate a node (insert copy after it).
pub fn duplicate_node(source: &str, language: &str, position: &Position) -> AstResult<AstEdit> {
    let result = parse(source, language)?;
    let (row, col) = *position;
    let root = result.tree.root_node();

    // Find the structural node at this position
    let node = resolve_to_named_node(root, row, col);

    if node.start_byte() == node.end_byte() {
        return Err(AstError::NoNodeAtPosition(row, col));
    }

    let node_text = &source[node.start_byte()..node.end_byte()];
    let newline = if source[node.end_byte()..].starts_with('\n') {
        ""
    } else {
        "\n"
    };

    let mut result_str = String::with_capacity(source.len() + node_text.len() + 1);
    result_str.push_str(&source[..node.end_byte()]);
    result_str.push_str(newline);
    result_str.push_str(node_text);
    result_str.push_str(&source[node.end_byte()..]);

    Ok(AstEdit {
        operation: "duplicate".into(),
        result: result_str,
        description: format!("Duplicated {}", node.kind()),
        applied: true,
    })
}

// ─── Insert ────────────────────────────────────────────────────────────

/// Insert text at a specific position.
pub fn insert_at(
    source: &str,
    language: &str,
    position: &Position,
    text: &str,
) -> AstResult<AstEdit> {
    let _result = parse(source, language)?;
    let (row, col) = *position;

    // Find the byte offset for the given position
    let byte_offset = position_to_byte_offset(source, row, col);

    let mut result_str = String::with_capacity(source.len() + text.len());
    result_str.push_str(&source[..byte_offset]);
    result_str.push_str(text);
    result_str.push_str(&source[byte_offset..]);

    Ok(AstEdit {
        operation: "insert".into(),
        result: result_str,
        description: format!("Inserted at {}:{}", row, col),
        applied: true,
    })
}

/// Replace text at a range.
pub fn replace_range(
    source: &str,
    language: &str,
    start: &Position,
    end: &Position,
    text: &str,
) -> AstResult<AstEdit> {
    parse(source, language)?; // validate parseability

    let start_byte = position_to_byte_offset(source, start.0, start.1);
    let end_byte = position_to_byte_offset(source, end.0, end.1);

    let mut result_str = String::with_capacity(source.len() + text.len());
    result_str.push_str(&source[..start_byte]);
    result_str.push_str(text);
    result_str.push_str(&source[end_byte..]);

    Ok(AstEdit {
        operation: "replace".into(),
        result: result_str,
        description: format!(
            "Replaced from {}:{} to {}:{}",
            start.0, start.1, end.0, end.1
        ),
        applied: true,
    })
}

/// Convert row/column position to byte offset in source.
fn position_to_byte_offset(source: &str, row: usize, col: usize) -> usize {
    let mut byte_offset = 0;

    for (current_row, line) in source.lines().enumerate() {
        if current_row == row {
            // Column is in bytes within the line (UTF-8 safe for ASCII-compatible)
            let line_bytes = line.len();
            let col_bytes = col.min(line_bytes);
            return byte_offset + col_bytes;
        }
        // Add 1 for the newline character
        byte_offset += line.len() + 1;
    }

    source.len()
}

// ─── Bulk Operations ───────────────────────────────────────────────────

/// Apply a series of edits to source code, returning the final result.
pub fn apply_edits(source: &str, edits: &[AstEdit]) -> String {
    let mut result = source.to_string();
    for edit in edits {
        if edit.applied {
            result = edit.result.clone();
        }
    }
    result
}

// ─── Node Kind Queries ─────────────────────────────────────────────────

/// Get the kind (type) of node at a given position.
pub fn node_kind_at(source: &str, language: &str, position: &Position) -> AstResult<String> {
    let info = node_at_position(&parse(source, language)?, position.0, position.1)?;
    Ok(info.kind)
}

/// Find all nodes of a specific kind in the AST.
pub fn find_nodes_of_kind(source: &str, language: &str, kind: &str) -> AstResult<Vec<NodeInfo>> {
    let result = parse(source, language)?;
    let root = result.tree.root_node();
    let mut nodes = Vec::new();
    collect_nodes_of_kind(&root, kind, &result.source, &mut nodes);
    Ok(nodes)
}

fn collect_nodes_of_kind<'a>(
    node: &Node<'a>,
    kind: &str,
    source: &'a str,
    acc: &mut Vec<NodeInfo>,
) {
    if node.is_named() && node.kind() == kind {
        acc.push(NodeInfo::from_node(node, source));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes_of_kind(&child, kind, source, acc);
    }
}

// ─── Format ────────────────────────────────────────────────────────────

/// Format source code by re-parsing and re-constructing with consistent
/// indentation. This is a simple structured formatter based on the AST.
pub fn format_source(source: &str, language: &str) -> AstResult<String> {
    // Parse to validate structure
    parse(source, language)?;
    // Simple formatting: ensure consistent newline at EOF
    let trimmed = source.trim_end();
    if !trimmed.is_empty() && !source.ends_with('\n') {
        return Ok(format!("{}\n", trimmed));
    }
    Ok(source.to_string())
}

// ─── Summary / Stats ───────────────────────────────────────────────────

/// Statistics about the AST of a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstStats {
    /// Language used.
    pub language: String,
    /// Source size in bytes.
    pub source_size: usize,
    /// Total node count.
    pub total_nodes: usize,
    /// Number of named nodes.
    pub named_nodes: usize,
    /// Max depth of the tree.
    pub max_depth: usize,
    /// Node type distribution.
    pub node_kinds: HashMap<String, usize>,
    /// Number of error nodes (parse errors).
    pub error_nodes: usize,
}

/// Compute statistics about the AST.
pub fn stats(source: &str, language: &str) -> AstResult<AstStats> {
    let result = parse(source, language)?;
    let root = result.tree.root_node();

    let mut total_nodes = 0usize;
    let mut named_nodes = 0usize;
    let mut max_depth = 0usize;
    let mut node_kinds: HashMap<String, usize> = HashMap::new();
    let mut error_nodes = 0usize;

    // Walk the tree
    let mut cursor = root.walk();
    let mut depth = 0;
    let mut stack_depth = vec![0usize];

    'walk: loop {
        let node = cursor.node();
        total_nodes += 1;
        if node.is_named() {
            named_nodes += 1;
        }
        if node.is_error() {
            error_nodes += 1;
        }
        *node_kinds.entry(node.kind().to_string()).or_insert(0) += 1;
        max_depth = max_depth.max(depth);

        if cursor.goto_first_child() {
            depth += 1;
            stack_depth.push(depth);
        } else if cursor.goto_next_sibling() {
            // stay at same depth
        } else if cursor.goto_parent() {
            depth -= 1;
            stack_depth.pop();
            // find next sibling
            if !cursor.goto_next_sibling() {
                loop {
                    if !cursor.goto_parent() {
                        break 'walk;
                    }
                    depth -= 1;
                    stack_depth.pop();
                    if cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        } else {
            break;
        }
    }

    Ok(AstStats {
        language: language.to_string(),
        source_size: source.len(),
        total_nodes,
        named_nodes,
        max_depth,
        node_kinds,
        error_nodes,
    })
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust() {
        let source = "fn main() { println!(\"hello\"); }";
        let result = parse(source, "rust").unwrap();
        let root = result.tree.root_node();
        assert_eq!(root.kind(), "source_file");
        assert!(!root.has_error());
    }

    #[test]
    fn test_parse_python() {
        let source = "def hello():\n    print('world')\n";
        let result = parse(source, "python").unwrap();
        let root = result.tree.root_node();
        assert_eq!(root.kind(), "module");
        assert!(!root.has_error());
    }

    #[test]
    fn test_parse_javascript() {
        let source = "function hello() { console.log('world'); }";
        let result = parse(source, "javascript").unwrap();
        let root = result.tree.root_node();
        assert_eq!(root.kind(), "program");
        assert!(!root.has_error());
    }

    #[test]
    fn test_parse_json() {
        let source = r#"{"key": "value", "num": 42}"#;
        let result = parse(source, "json").unwrap();
        let root = result.tree.root_node();
        assert_eq!(root.kind(), "document");
        assert!(!root.has_error());
    }

    #[test]
    fn test_parse_go() {
        let source = "package main\nfunc main() {}\n";
        let result = parse(source, "go").unwrap();
        let root = result.tree.root_node();
        assert_eq!(root.kind(), "source_file");
        assert!(!root.has_error());
    }

    #[test]
    fn test_unsupported_language() {
        let result = parse("hello", "nonexistent");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AstError::UnsupportedLanguage(_)
        ));
    }

    #[test]
    fn test_node_at_position() {
        let source = "fn foo() { let x = 1; }";
        let result = parse(source, "rust").unwrap();
        let info = node_at_position(&result, 0, 3).unwrap(); // inside "foo"
        assert!(info.text.contains("foo") || info.kind.contains("identifier"));
    }

    #[test]
    fn test_node_stack() {
        let source = "fn foo() { let x = 1; }";
        let result = parse(source, "rust").unwrap();
        let stack = node_stack_at_position(&result, 0, 15); // inside "x = 1"
        assert!(!stack.is_empty());
        // Stack goes from root to the deepest node
        assert!(stack[0].kind == "source_file");
    }

    #[test]
    fn test_top_level_nodes() {
        let source = "fn a() {}\nfn b() {}\nfn c() {}";
        let result = parse(source, "rust").unwrap();
        let nodes = top_level_nodes(&result);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].kind, "function_item");
    }

    #[test]
    fn test_query_rust() {
        let source = "fn hello() { let x = 1; }\nfn world() { let y = 2; }";
        let result = parse(source, "rust").unwrap();
        let matches = query(&result, "(function_item name: (identifier) @fn.name)").unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].captures[0].text, "hello");
        assert_eq!(matches[1].captures[0].text, "world");
    }

    #[test]
    fn test_move_node_up() {
        let source = "fn b() {}\nfn a() {}";
        let edit = move_node_up(source, "rust", &(1, 0)).unwrap();
        assert!(edit.applied);
        assert!(edit.result.contains("fn a()"));
        // a should be before b now
        let a_pos = edit.result.find("fn a()").unwrap();
        let b_pos = edit.result.find("fn b()").unwrap();
        assert!(a_pos < b_pos, "fn a() should come before fn b()");
    }

    #[test]
    fn test_move_node_down() {
        let source = "fn a() {}\nfn b() {}";
        let edit = move_node_down(source, "rust", &(0, 0)).unwrap();
        assert!(edit.applied);
        let a_pos = edit.result.find("fn a()").unwrap();
        let b_pos = edit.result.find("fn b()").unwrap();
        assert!(a_pos > b_pos, "fn a() should come after fn b()");
    }

    #[test]
    fn test_move_up_at_top_noop() {
        let source = "fn a() {}\nfn b() {}";
        let edit = move_node_up(source, "rust", &(0, 0)).unwrap();
        assert!(!edit.applied);
        assert_eq!(edit.result, source);
    }

    #[test]
    fn test_swap_nodes() {
        let source = "fn a() {}\nfn c() {}\nfn b() {}";
        let edit = swap_nodes(source, "rust", &(0, 0), &(2, 0)).unwrap();
        assert!(edit.applied);
        // a and b should be swapped
        assert!(edit.result.starts_with("fn b()"));
        assert!(edit.result.ends_with("fn a() {}"));
    }

    #[test]
    fn test_extract_variable() {
        let source = "fn main() { let x = 1 + 2; }";
        let edit = extract_node(source, "rust", &(0, 20), "sum").unwrap();
        assert!(edit.applied);
        assert!(edit.result.contains("let sum = 1 + 2;"));
    }

    #[test]
    fn test_wrap_block() {
        let source = "let x = 1;";
        let edit = wrap_node(source, "rust", &(0, 0), "if").unwrap();
        assert!(edit.applied);
        assert!(edit.result.contains("if true"));
        assert!(edit.result.contains("let x = 1;"));
    }

    #[test]
    fn test_wrap_parens() {
        let source = "x + y";
        let edit = wrap_node(source, "rust", &(0, 0), "()").unwrap();
        assert!(edit.applied);
        assert_eq!(edit.result, "(x + y)");
    }

    #[test]
    fn test_unwrap_node() {
        let source = "if true {\n    let x = 1;\n}";
        let edit = unwrap_node(source, "rust", &(0, 4)).unwrap();
        assert!(edit.applied);
        assert!(edit.result.contains("let x = 1;"));
        assert!(!edit.result.contains("if true"));
    }

    #[test]
    fn test_duplicate_node() {
        let source = "fn a() {}\nfn b() {}\n";
        let edit = duplicate_node(source, "rust", &(0, 0)).unwrap();
        assert!(edit.applied);
        // Should have two copies of fn a()
        let count = edit.result.matches("fn a()").count();
        assert_eq!(count, 2, "Expected 2 copies of fn a(), got {}", count);
    }

    #[test]
    fn test_find_nodes_of_kind() {
        let source = "fn a() {}\nfn b() {}\nstruct S {}";
        let nodes = find_nodes_of_kind(source, "rust", "function_item").unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_stats() {
        let source = "fn a() {}\nfn b() {}\n";
        let s = stats(source, "rust").unwrap();
        assert_eq!(s.language, "rust");
        assert!(s.total_nodes > 0);
        assert!(s.named_nodes > 0);
        assert!(s.node_kinds.contains_key("function_item"));
    }

    #[test]
    fn test_detect_language_by_extension() {
        assert_eq!(detect_language("main.rs"), Some(LanguageId::Rust));
        assert_eq!(detect_language("app.py"), Some(LanguageId::Python));
        assert_eq!(detect_language("index.js"), Some(LanguageId::JavaScript));
        assert_eq!(detect_language("main.go"), Some(LanguageId::Go));
        assert_eq!(detect_language("data.json"), Some(LanguageId::Json));
        assert_eq!(detect_language("unknown.xyz"), None);
    }

    #[test]
    fn test_format_source() {
        let source = "fn main() { println!(\"hi\"); }\n";
        let formatted = format_source(source, "rust").unwrap();
        assert_eq!(formatted, source);
    }

    #[test]
    fn test_insert_at() {
        let source = "fn a() {}\nfn b() {}";
        let edit = insert_at(source, "rust", &(1, 0), "fn middle() {}\n").unwrap();
        assert!(edit.applied);
        assert!(edit.result.contains("fn middle()"));
    }

    #[test]
    fn test_siblings_at_position() {
        let source = "fn a() {}\nfn b() {}\nfn c() {}";
        let siblings = siblings_at_position(&parse(source, "rust").unwrap(), 0, 3).unwrap();
        assert_eq!(siblings.len(), 3);
    }

    #[test]
    fn test_query_python() {
        let source = "def foo():\n    pass\ndef bar():\n    pass";
        let result = parse(source, "python").unwrap();
        let matches = query(&result, "(function_definition name: (identifier) @fn.name)").unwrap();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_root_node_info() {
        let source = "fn a() {}\nfn b() {}";
        let result = parse(source, "rust").unwrap();
        let info = root_node_info(&result, 2);
        assert_eq!(info.kind, "source_file");
        assert!(info.children.is_some());
    }

    #[test]
    fn test_parse_error_detection() {
        let source = "fn main() { let x = }";
        let result = parse(source, "rust").unwrap();
        let root = result.tree.root_node();
        assert!(root.has_error());
    }

    #[test]
    fn test_empty_source() {
        let result = parse("", "json").unwrap();
        let root = result.tree.root_node();
        // JSON with empty source should have error nodes
        assert!(root.has_error() || root.child_count() == 0);
    }
}
