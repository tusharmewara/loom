//! `loom ast` — AST Structural Editing CLI.
//!
//! Parse, navigate, query, and edit source code using tree-sitter ASTs.

use crate::cli::AstAction;
use loom_ast::{
    detect_language, duplicate_node, extract_node, find_nodes_of_kind, format_source, insert_at,
    move_node_down, move_node_up, node_at_position, node_kind_at, parse, replace_range,
    root_node_info, stats, swap_nodes, unwrap_node, wrap_node, AstEdit,
};
use std::path::Path;

pub async fn run(action: &AstAction) -> anyhow::Result<()> {
    match action {
        AstAction::Parse {
            file,
            language,
            max_depth,
            json,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let result = parse(&source, &lang).map_err(|e| anyhow::anyhow!("Parse error: {e}"))?;
            let info = root_node_info(&result, *max_depth as usize);
            if *json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                print_node_tree(&info, 0);
            }
        }
        AstAction::Query {
            file,
            language,
            query,
            json,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let result = parse(&source, &lang).map_err(|e| anyhow::anyhow!("Parse error: {e}"))?;
            let matches = loom_ast::query(&result, query)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&matches)?);
            } else {
                println!("Query matches ({}):", matches.len());
                for m in &matches {
                    for c in &m.captures {
                        println!(
                            "  @{} {} = \"{}\" ({}:{})",
                            c.name, c.kind, c.text, c.start.0, c.start.1
                        );
                    }
                }
            }
        }
        AstAction::NodeAt {
            file,
            language,
            line,
            column,
            json,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let result = parse(&source, &lang).map_err(|e| anyhow::anyhow!("Parse error: {e}"))?;
            let info = node_at_position(&result, *line, *column)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!("Node at {}:{}:", line, column);
                println!("  kind: {}", info.kind);
                println!(
                    "  range: {}:{} .. {}:{}",
                    info.start.0, info.start.1, info.end.0, info.end.1
                );
                println!("  text: {}", info.text);
            }
        }
        AstAction::MoveUp {
            file,
            language,
            line,
            column,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| move_node_up(&source, &lang, &(*line, *column)),
            )
            .await?;
        }
        AstAction::MoveDown {
            file,
            language,
            line,
            column,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| move_node_down(&source, &lang, &(*line, *column)),
            )
            .await?;
        }
        AstAction::Swap {
            file,
            language,
            line_a,
            column_a,
            line_b,
            column_b,
            dry_run,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let edit = swap_nodes(&source, &lang, &(*line_a, *column_a), &(*line_b, *column_b))?;
            print_or_apply_edit(file, &edit, *dry_run).await?;
        }
        AstAction::Extract {
            file,
            language,
            line,
            column,
            name,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| extract_node(&source, &lang, &(*line, *column), name),
            )
            .await?;
        }
        AstAction::Wrap {
            file,
            language,
            line,
            column,
            wrapper,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| wrap_node(&source, &lang, &(*line, *column), wrapper),
            )
            .await?;
        }
        AstAction::Unwrap {
            file,
            language,
            line,
            column,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| unwrap_node(&source, &lang, &(*line, *column)),
            )
            .await?;
        }
        AstAction::Duplicate {
            file,
            language,
            line,
            column,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| duplicate_node(&source, &lang, &(*line, *column)),
            )
            .await?;
        }
        AstAction::Insert {
            file,
            language,
            line,
            column,
            text,
            dry_run,
        } => {
            apply_ast_edit(
                file,
                language.as_deref(),
                *line,
                *column,
                *dry_run,
                |source, lang| insert_at(&source, &lang, &(*line, *column), text),
            )
            .await?;
        }
        AstAction::Replace {
            file,
            language,
            start_line,
            start_column,
            end_line,
            end_column,
            text,
            dry_run,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let edit = replace_range(
                &source,
                &lang,
                &(*start_line, *start_column),
                &(*end_line, *end_column),
                text,
            )?;
            print_or_apply_edit(file, &edit, *dry_run).await?;
        }
        AstAction::KindAt {
            file,
            language,
            line,
            column,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let kind = node_kind_at(&source, &lang, &(*line, *column))?;
            println!("{}", kind);
        }
        AstAction::FindKind {
            file,
            language,
            kind,
            json,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let nodes = find_nodes_of_kind(&source, &lang, kind)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&nodes)?);
            } else {
                println!("Nodes of kind '{}' ({}):", kind, nodes.len());
                for n in &nodes {
                    println!(
                        "  {}:{}..{}:{}  {}",
                        n.start.0, n.start.1, n.end.0, n.end.1, n.text
                    );
                }
            }
        }
        AstAction::Format {
            file,
            language,
            dry_run,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let formatted = format_source(&source, &lang)?;
            if *dry_run {
                println!("{}", formatted);
            } else {
                let path = resolve_path(file);
                std::fs::write(&path, &formatted)?;
                println!("✓ Formatted {}", path.display());
            }
        }
        AstAction::Stats {
            file,
            language,
            json,
        } => {
            let (source, lang) = load_source(file, language.as_deref())?;
            let s = stats(&source, &lang)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                println!("AST Stats for {}:", file);
                println!("  Language:     {}", s.language);
                println!("  Size:         {} bytes", s.source_size);
                println!("  Total nodes:  {}", s.total_nodes);
                println!("  Named nodes:  {}", s.named_nodes);
                println!("  Max depth:    {}", s.max_depth);
                println!("  Error nodes:  {}", s.error_nodes);
                println!("  Node kinds:");
                let mut kinds: Vec<_> = s.node_kinds.into_iter().collect();
                kinds.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
                for (kind, count) in kinds.iter().take(15) {
                    println!("    {}: {}", kind, count);
                }
            }
        }
    }
    Ok(())
}

/// Read file and detect language.
fn load_source(file: &str, language_override: Option<&str>) -> anyhow::Result<(String, String)> {
    let path = resolve_path(file);
    let source = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {e}", path.display()))?;

    let lang = match language_override {
        Some(l) => l.to_string(),
        None => {
            match detect_language(file) {
                Some(l) => l.display_name().to_lowercase(),
                None => {
                    // Try to detect from extension
                    let ext = Path::new(file)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    match ext {
                        "rs" => "rust".to_string(),
                        "py" => "python".to_string(),
                        "js" | "jsx" | "ts" | "tsx" => "javascript".to_string(),
                        "go" => "go".to_string(),
                        "json" => "json".to_string(),
                        _ => anyhow::bail!("Cannot detect language for '{file}'. Use --language."),
                    }
                }
            }
        }
    };
    Ok((source, lang))
}

fn resolve_path(file: &str) -> std::path::PathBuf {
    if file.starts_with('/') {
        std::path::PathBuf::from(file)
    } else {
        std::env::current_dir().unwrap_or_default().join(file)
    }
}

/// Apply an AST edit operation.
async fn apply_ast_edit<F>(
    file: &str,
    language: Option<&str>,
    _line: usize,
    _column: usize,
    dry_run: bool,
    op: F,
) -> anyhow::Result<()>
where
    F: FnOnce(String, String) -> Result<AstEdit, loom_ast::AstError>,
{
    let (source, lang) = load_source(file, language)?;
    let edit = op(source, lang).map_err(|e| anyhow::anyhow!("AST edit failed: {e}"))?;
    print_or_apply_edit(file, &edit, dry_run).await
}

/// Print edit result or write to file.
async fn print_or_apply_edit(file: &str, edit: &AstEdit, dry_run: bool) -> anyhow::Result<()> {
    if dry_run {
        println!("=== Dry-run: {} === (use --write to apply)", file);
        println!("Operation: {}", edit.operation);
        println!("Description: {}", edit.description);
        if edit.applied {
            println!("{}", edit.result);
        } else {
            println!("(no change: {})", edit.description);
        }
    } else if edit.applied {
        let path = resolve_path(file);
        std::fs::write(&path, &edit.result)?;
        println!("✓ {}: {} (applied)", path.display(), edit.description);
    } else {
        println!("(no change: {})", edit.description);
    }
    Ok(())
}

/// Print a tree of node info.
fn print_node_tree(info: &loom_ast::NodeInfo, depth: usize) {
    let indent = "  ".repeat(depth);
    let marker = if info.is_error { "⚠ " } else { "" };
    println!(
        "{}{}{}  {}:{}..{}:{}",
        indent, marker, info.kind, info.start.0, info.start.1, info.end.0, info.end.1
    );
    if let Some(ref children) = info.children {
        for child in children {
            print_node_tree(child, depth + 1);
        }
    }
}
