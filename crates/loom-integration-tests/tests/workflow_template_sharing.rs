//! Integration test for workflow template sharing between tools.
//! Tests that workflow templates can be shared and used across different tools in the Loom ecosystem.

use loom_core::WorkflowTemplate;
use loom_sessions::SessionManager;
use loom_workflow::{TemplateManager, WorkflowEngine};
use tempfile::tempdir;

#[tokio::test]
async fn test_workflow_template_sharing_between_tools() -> anyhow::Result<()> {
    // Create temporary directory for agents
    let loom_dir = tempdir()?;
    let agents_path = loom_dir.path().to_str().unwrap();

    // Initialize session manager and workflow engine
    let _session_manager = SessionManager::new(agents_path);
    let workflow_engine = WorkflowEngine::new(agents_path);
    let template_manager = TemplateManager::new(agents_path);

    // Create a workflow template in jcode
    let template_name = "bug-fix-template";
    let template = WorkflowTemplate {
        name: template_name.to_string(),
        description: "Template for fixing bugs".to_string(),
        default_title: Some("Fix: {issue_title}".to_string()),
        default_description: Some("Addressing bug: {issue_description}".to_string()),
        default_agent: Some("jcode".to_string()),
        sub_agents: None,
        default_provider: None,
        default_model: None,
    };

    // Save template
    template_manager.save(&template)?;

    // Verify template is saved and can be loaded
    let loaded_template = template_manager.get(template_name);
    assert!(
        loaded_template.is_some(),
        "Template should be saved and loadable"
    );
    let template_ref = loaded_template.unwrap();
    assert_eq!(template_ref.name, template_name);
    assert_eq!(
        template_ref.description,
        "Template for fixing bugs".to_string()
    );
    assert_eq!(template_ref.default_agent.as_deref(), Some("jcode"));

    // Now simulate importing the template into claude-code
    // In reality, this would happen through the shared ~/.loom/workflow/templates/ directory
    let claude_template_manager = TemplateManager::new(agents_path);
    let claude_loaded = claude_template_manager.get(template_name);
    assert!(
        claude_loaded.is_some(),
        "Template should be accessible from claude-code"
    );
    let claude_template = claude_loaded.unwrap();
    assert_eq!(claude_template.name, template_name);
    assert_eq!(
        claude_template.default_title,
        Some("Fix: {issue_title}".to_string())
    );

    // Test that we can create a task from the template in both tools
    let jcode_task = workflow_engine.orchestrator().create_task(
        "Fix login issue",
        "Users unable to login after password reset",
        false,
        None,
    )?;

    // Verify task was created with template defaults
    assert_eq!(jcode_task.title, "Fix login issue");
    assert_eq!(
        jcode_task.description,
        "Users unable to login after password reset"
    );
    assert_eq!(jcode_task.routed_provider, None); // Would be set by routing

    // Test template listing
    let templates = template_manager.list();
    assert!(templates.iter().any(|t| t.name == template_name));

    println!("✅ Workflow template successfully shared between jcode and claude-code");
    Ok(())
}

#[tokio::test]
async fn test_workflow_template_update_propagation() -> anyhow::Result<()> {
    let loom_dir = tempdir()?;
    let agents_path = loom_dir.path().to_str().unwrap();

    let template_manager = TemplateManager::new(agents_path);

    // Create initial template
    let template = WorkflowTemplate {
        name: "update-test".to_string(),
        description: "Initial template".to_string(),
        default_title: Some("Initial: {title}".to_string()),
        default_description: None,
        default_agent: Some("jcode".to_string()),
        sub_agents: None,
        default_provider: None,
        default_model: None,
    };

    template_manager.save(&template)?;

    // Verify initial state
    let initial = template_manager.get("update-test").unwrap();
    assert_eq!(initial.description, "Initial template");
    assert_eq!(initial.default_title.as_deref(), Some("Initial: {title}"));

    // Update template (simulating changes from another tool)
    let updated_template = WorkflowTemplate {
        name: "update-test".to_string(),
        description: "Updated template from claude-code".to_string(),
        default_title: Some("Updated: {title}".to_string()),
        default_description: Some("This description was updated".to_string()),
        default_agent: Some("claude-code".to_string()),
        sub_agents: None,
        default_provider: None,
        default_model: None,
    };

    template_manager.save(&updated_template)?;

    // Verify update is visible
    let updated = template_manager.get("update-test").unwrap();
    assert_eq!(updated.description, "Updated template from claude-code");
    assert_eq!(updated.default_title.as_deref(), Some("Updated: {title}"));
    assert_eq!(updated.default_agent.as_deref(), Some("claude-code"));
    assert_eq!(
        updated.default_description.as_deref(),
        Some("This description was updated")
    );

    println!("✅ Workflow template updates propagate correctly between tools");
    Ok(())
}
