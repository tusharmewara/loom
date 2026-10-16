//! Linear issue tracker backend (GraphQL API).

use crate::models::{Issue, IssueQuery, TrackerError};
use crate::tracker::IssueTracker;
use reqwest::Client;

/// The Linear GraphQL API endpoint.
const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

/// Linear issue tracker backend.
#[derive(Debug)]
pub struct LinearTracker {
    client: Client,
    api_key: String,
}

impl LinearTracker {
    /// Create a new Linear tracker.
    ///
    /// Reads `LINEAR_API_KEY` from the environment if `key_override` is `None`.
    pub fn new(key_override: Option<String>) -> Result<Self, TrackerError> {
        let api_key = match key_override {
            Some(k) => k,
            None => std::env::var("LINEAR_API_KEY").map_err(|_| {
                TrackerError::Config("LINEAR_API_KEY must be set, or pass --token".into())
            })?,
        };

        let client = Client::builder()
            .user_agent("loom-trackers/0.1")
            .build()
            .map_err(|e| TrackerError::Other(e.into()))?;

        Ok(Self { client, api_key })
    }

    /// Execute a GraphQL query and return the JSON response data.
    async fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value, TrackerError> {
        let payload = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let resp = self
            .client
            .post(LINEAR_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| TrackerError::GitHubApi(e.to_string()))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TrackerError::GitHubApi(format!("Failed to parse response: {e}")))?;

        if !status.is_success() {
            let msg = body["error"].as_str().unwrap_or("unknown error");
            return match status.as_u16() {
                401 | 403 => Err(TrackerError::Auth(msg.to_string())),
                404 => Err(TrackerError::NotFound(msg.to_string())),
                429 => Err(TrackerError::RateLimited),
                _ => Err(TrackerError::Http(status, msg.to_string())),
            };
        }

        // Check for GraphQL errors
        if let Some(errors) = body["errors"].as_array() {
            if let Some(first) = errors.first() {
                let msg = first["message"].as_str().unwrap_or("GraphQL error");
                return Err(TrackerError::GitHubApi(msg.to_string()));
            }
        }

        Ok(body["data"].clone())
    }
}

#[async_trait::async_trait]
impl IssueTracker for LinearTracker {
    fn name(&self) -> &str {
        "linear"
    }

    async fn get_issue(&self, id: &str) -> Result<Issue, TrackerError> {
        let query = r#"
            query Issue($id: String!) {
                issue(id: $id) {
                    id
                    title
                    description
                    state { name }
                    labels { nodes { name } }
                    assignee { name }
                    url
                    createdAt
                    team { id name }
                }
            }
        "#;

        let data = self.graphql(query, serde_json::json!({ "id": id })).await?;
        let issue = &data["issue"];

        if issue.is_null() {
            return Err(TrackerError::NotFound(format!("Issue '{id}' not found")));
        }

        Ok(linear_issue_to_model(issue))
    }

    async fn list_issues(&self, query: &IssueQuery) -> Result<Vec<Issue>, TrackerError> {
        // Use the repo field as the team ID
        let team_id = &query.repo;
        let first = query.limit.unwrap_or(30).min(100);

        let gql_query = r#"
            query Issues($teamId: String!, $first: Int) {
                team(id: $teamId) {
                    issues(first: $first) {
                        nodes {
                            id
                            title
                            description
                            state { name }
                            labels { nodes { name } }
                            assignee { name }
                            url
                            createdAt
                        }
                    }
                }
            }
        "#;

        let data = self
            .graphql(
                gql_query,
                serde_json::json!({
                    "teamId": team_id,
                    "first": first,
                }),
            )
            .await?;

        let nodes = data["team"]["issues"]["nodes"].as_array().ok_or_else(|| {
            TrackerError::GitHubApi("expected team.issues.nodes in response".into())
        })?;

        let issues: Vec<Issue> = nodes.iter().map(linear_issue_to_model).collect();
        Ok(issues)
    }

    async fn add_comment(&self, id: &str, body: &str) -> Result<(), TrackerError> {
        let query = r#"
            mutation CommentCreate($issueId: String!, $body: String!) {
                commentCreate(input: { issueId: $issueId, body: $body }) {
                    success
                    comment { id }
                }
            }
        "#;

        let data = self
            .graphql(
                query,
                serde_json::json!({
                    "issueId": id,
                    "body": body,
                }),
            )
            .await?;

        let success = data["commentCreate"]["success"].as_bool().unwrap_or(false);
        if !success {
            return Err(TrackerError::GitHubApi("Failed to create comment".into()));
        }

        Ok(())
    }
}

fn linear_issue_to_model(val: &serde_json::Value) -> Issue {
    let labels: Vec<String> = val["labels"]["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let state = val["state"]["name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let assignee = val["assignee"]["name"].as_str().map(|s| s.to_string());
    let html_url = val["url"].as_str().map(|s| s.to_string());
    let created_at = val["createdAt"].as_str().map(|s| s.to_string());
    let title = val["title"].as_str().unwrap_or("(untitled)").to_string();
    let description = val["description"].as_str().map(|s| s.to_string());
    let id = val["id"].as_str().unwrap_or("").to_string();

    let mut metadata = std::collections::HashMap::new();
    if let Some(team) = val["team"]["name"].as_str() {
        metadata.insert("team".into(), team.to_string());
    }

    Issue {
        id,
        title,
        state,
        body: description,
        labels,
        assignee,
        html_url,
        metadata,
        created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_missing_api_key() {
        // Without LINEAR_API_KEY set, should return Config error
        std::env::remove_var("LINEAR_API_KEY");
        let result = LinearTracker::new(None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TrackerError::Config(_)));
    }

    #[test]
    fn test_new_with_key_override() {
        let tracker = LinearTracker::new(Some("lin-api-key".into()));
        assert!(tracker.is_ok());
        assert_eq!(tracker.unwrap().name(), "linear");
    }
}
