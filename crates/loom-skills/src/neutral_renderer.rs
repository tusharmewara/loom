use serde::{Deserialize, Serialize};
use serde_yaml;
use std::collections::BTreeMap;
use std::fmt;

/// Represents the parsed structure of a skill file with YAML frontmatter
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SkillFrontmatter {
    /// Skill name
    pub name: Option<String>,
    /// Skill description
    pub description: Option<String>,
    /// Skill tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Ecosystem compatibility
    #[serde(default)]
    pub ecosystem_compatible: Vec<String>,
    /// Ecosystem requirements
    #[serde(default)]
    pub ecosystem_requires: Vec<String>,
    /// Skill version
    pub version: Option<String>,
    /// Any additional fields
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// Neutral renderer that produces byte-identical output
pub struct NeutralRenderer;

impl NeutralRenderer {
    /// Create a new neutral renderer
    pub fn new() -> Self {
        Self
    }
    
    /// Render a skill to neutral format (sorted keys, single quotes, LF endings)
    pub fn render_skill<C: AsRef<str>>(&self, content: C) -> anyhow::Result<String> {
        let content_str = content.as_ref();
        
        // Parse frontmatter and content
        let (frontmatter, body) = self.parse_frontmatter(content_str)?;
        
        // Render to neutral format
        let mut output = String::new();
        
        // Start with frontmatter delimiter
        output.push_str("---\n");
        
        // Serialize frontmatter with sorted keys
        let yaml_string = self.serialize_frontmatter_neutral(&frontmatter)?;
        output.push_str(&yaml_string);
        
        // End frontmatter delimiter
        output.push_str("---\n");
        
        // Add body (preserve original line endings but normalize to LF internally)
        if !body.is_empty() {
            // Ensure body ends with newline
            let normalized_body = body.replace("\r\n", "\n").replace('\r', "\n");
            if !normalized_body.ends_with('\n') {
                output.push_str(&normalized_body);
                output.push('\n');
            } else {
                output.push_str(&normalized_body);
            }
        }
        
        Ok(output)
    }
    
    /// Parse frontmatter from skill content
    fn parse_frontmatter(&self, content: &str) -> anyhow::Result<(SkillFrontmatter, String)> {
        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") {
            // No frontmatter, treat entire content as body
            return Ok((SkillFrontmatter::default(), content.to_string()));
        }
        
        // Find the end of frontmatter
        let mut lines = content.lines();
        // Skip first line (---)
        lines.next();
        
        let mut frontmatter_lines = Vec::new();
        let mut body_start = 0;
        
        for (i, line) in lines.enumerate() {
            if line == "---" {
                body_start = i + 1; // Line after the closing ---
                break;
            }
            frontmatter_lines.push(line);
        }
        
        // Parse frontmatter YAML
        let frontmatter_yaml = frontmatter_lines.join("\n");
        let frontmatter: SkillFrontmatter = if frontmatter_yaml.trim().is_empty() {
            SkillFrontmatter::default()
        } else {
            serde_yaml::from_str(&frontmatter_yaml)?
        };
        
        // Extract body (everything after the closing ---)
        let body_lines: Vec<&str> = lines.skip(body_start + 1).collect(); // +1 to skip the --- line
        let body = body_lines.join("\n");
        
        Ok((frontmatter, body))
    }
    
    /// Serialize frontmatter in neutral format (sorted keys, single quotes)
    fn serialize_frontmatter_neutral(&self, frontmatter: &SkillFrontmatter) -> anyhow::Result<String> {
        // Create a BTreeMap for sorted key serialization
        let mut map = BTreeMap::new();
        
        // Add standard fields in alphabetical order
        if let Some(ref name) = frontmatter.name {
            map.insert("name".to_string(), serde_yaml::Value::String(name.clone()));
        }
        
        if let Some(ref description) = frontmatter.description {
            map.insert("description".to_string(), serde_yaml::Value::String(description.clone()));
        }
        
        if !frontmatter.tags.is_empty() {
            let tags_value = serde_yaml::Value::Sequence(
                frontmatter.tags.iter().map(|t| serde_yaml::Value::String(t.clone())).collect()
            );
            map.insert("tags".to_string(), tags_value);
        }
        
        if !frontmatter.ecosystem_compatible.is_empty() {
            let compat_value = serde_yaml::Value::Sequence(
                frontmatter.ecosystem_compatible.iter()
                    .map(|c| serde_yaml::Value::String(c.clone()))
                    .collect()
            );
            map.insert("ecosystem_compatible".to_string(), compat_value);
        }
        
        if !frontmatter.ecosystem_requires.is_empty() {
            let requires_value = serde_yaml::Value::Sequence(
                frontmatter.ecosystem_requires.iter()
                    .map(|r| serde_yaml::Value::String(r.clone()))
                    .collect()
            );
            map.insert("ecosystem_requires".to_string(), requires_value);
        }
        
        if let Some(ref version) = frontmatter.version {
            map.insert("version".to_string(), serde_yaml::Value::String(version.clone()));
        }
        
        // Add extra fields (already sorted due to BTreeMap in flatten)
        for (key, value) in &frontmatter.extra {
            map.insert(key.clone(), value.clone());
        }
        
        // Custom YAML serialization to force single quotes
        let mut yaml_output = String::new();
        self.serialize_map_to_yaml_neutral(&mut yaml_output, &map, 0)?;
        
        Ok(yaml_output)
    }
    
    /// Serialize a BTreeMap to YAML with neutral formatting (single quotes, sorted keys)
    fn serialize_map_to_yaml_neutral<W: fmt::Write>(
        &self,
        writer: &mut W,
        map: &BTreeMap<String, serde_yaml::Value>,
        indent: usize,
    ) -> anyhow::Result<()> {
        let indent_str = " ".repeat(indent);
        
        for (key, value) in map {
            writer.write_str(&indent_str)?;
            writer.write_str(key)?;
            writer.write_str(": ")?;
            
            self.serialize_value_to_yaml_neutral(writer, value, indent + 2)?;
            writer.write_char('\n')?;
        }
        
        Ok(())
    }
    
    /// Serialize a YAML value to neutral format (single quotes for strings)
    fn serialize_value_to_yaml_neutral<W: fmt::Write>(
        &self,
        writer: &mut W,
        value: &serde_yaml::Value,
        indent: usize,
    ) -> anyhow::Result<()> {
        match value {
            serde_yaml::Value::String(s) => {
                // Use single quotes for strings
                writer.write_str(&format!('\'{}\'', s.escape_default()))?;
            }
            serde_yaml::Value::Sequence(seq) => {
                writer.write_char('[')?;
                if !seq.is_empty() {
                    writer.write_char(' ')?;
                    for (i, item) in seq.iter().enumerate() {
                        if i > 0 {
                            writer.write_str(", ")?;
                        }
                        self.serialize_value_to_yaml_neutral(writer, item, indent)?;
                    }
                    writer.write_char(' ')?;
                }
                writer.write_char(']')?;
            }
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    writer.write_str(&i.to_string())?;
                } else if let Some(f) = n.as_f64() {
                    writer.write_str(&f.to_string())?;
                }
            }
            serde_yaml::Value::Bool(b) => {
                writer.write_str(if *b { "true" } else { "false" })?;
            }
            serde_yaml::Value::Null => {
                writer.write_str("null")?;
            }
            // For mapping and other complex types, we could recurse but skills shouldn't have those
            _ => {
                writer.write_str(&value.to_string())?;
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_render_simple_skill() -> anyhow::Result<()> {
        let renderer = NeutralRenderer::new();
        
        let input = r#"---
name: test-skill
description: A test skill
tags:
  - rust
  - example
version: "1.0.0"
---
# Test Skill

This is a test skill body."#;
        
        let output = renderer.render_skill(input)?;
        
        // Check that output has the expected neutral format
        assert!(output.contains("name: 'test-skill'"));
        assert!(output.contains("description: 'A test skill'"));
        assert!(output.contains("tags:"));
        assert!(output.contains("- 'rust'"));
        assert!(output.contains("- 'example'"));
        assert!(output.contains("version: '1.0.0'"));
        
        // Check that it uses LF line endings (no CR)
        assert!(!output.contains("\r"));
        
        // Check that rendering twice produces identical output
        let output2 = renderer.render_skill(&output)?;
        assert_eq!(output, output2);
        
        Ok(())
    }
    
    #[test>
    fn test_render_alphabetical_order() -> anyhow::Result<()> {
        let renderer = NeutralRenderer::new();
        
        // Input with fields in random order
        let input = r#"---
version: "2.0.0"
name: zskill
description: Last alphabetically
tags: [z-tag, a-tag]
ecosystem_requires:
  - requirement-b
  - requirement-a
ecosystem_compatible:
  - compatible-z
  - compatible-a
---
# Body
"#;
        
        let output = renderer.render_skill(input)?;
        
        // Check that keys appear in alphabetical order
        let lines: Vec<&str> = output.lines().collect();
        let mut in_frontmatter = false;
        let mut key_order = Vec::new();
        
        for line in lines {
            if line.trim() == "---" {
                if !in_frontmatter {
                    in_frontmatter = true;
                } else {
                    break; // End of frontmatter
                }
                continue;
            }
            
            if in_frontmatter && line.contains(':') && !line.trim().starts_with('-') {
                // Extract key
                if let Some(key) = line.split(':').next() {
                    let key = key.trim();
                    if !key.is_empty() {
                        key_order.push(key);
                    }
                }
            }
        }
        
        // Expected alphabetical order
        let expected = vec![
            "description", "ecosystem_compatible", "ecosystem_requires",
            "name", "tags", "version"
        ];
        
        assert_eq!(key_order, expected);
        
        Ok(())
    }
    
    #[test>
    fn test_no_frontmatter() -> anyhow::Result<()> {
        let renderer = NeutralRenderer::new();
        
        let input = "# Just a markdown skill\n\nWith some content.";
        let output = renderer.render_skill(input)?;
        
        // Should still produce valid output with empty frontmatter
        assert!(output.starts_with("---\n---\n"));
        assert!(output.contains("# Just a markdown skill"));
        
        // Should be idempotent
        let output2 = renderer.render_skill(&output)?;
        assert_eq!(output, output2);
        
        Ok(())
    }
    
    #[test>
    fn test_single_quotes_and_special_chars() -> anyhow::Result<()> {
        let renderer = NeutralRenderer::new();
        
        let input = r#"---
name: "test'skill"
description: "Skill with \"quotes\" and \n newline"
tags: [tag-with-dashes, tag_with_underscores]
---
# Body
"#;
        
        let output = renderer.render_skill(input)?;
        
        // Check that special characters are properly escaped in single quotes
        assert!(output.contains("name: 'test\\''skill'")); // Escaped single quote
        assert!(output.contains("description: 'Skill with \\\"quotes\\\" and \\n newline'"));
        
        Ok(())
    }
}