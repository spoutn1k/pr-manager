use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum Mergeable {
    #[serde(rename = "MERGEABLE")]
    Ok,
    #[serde(rename = "CONFLICTING")]
    Conflict,
    #[serde(untagged)]
    Unknown,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PullRequest {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub number: i32,
    #[serde(default)]
    pub state: String, // "open", "closed"
    pub mergeable: Mergeable,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>, // Can be null
    #[serde(default)]
    pub created_at: String, // ISO 8601 format
    #[serde(default)]
    pub updated_at: String, // ISO 8601 format
    #[serde(default)]
    pub closed_at: Option<String>, // Can be null
    #[serde(default)]
    pub merged_at: Option<String>, // Can be null

    #[serde(default, rename = "headRefName")]
    pub branch: String,
    #[serde(default, rename = "baseRefName")]
    pub base_name: String,
    #[serde(default, rename = "baseRefOid")]
    pub base_commit: String,
    #[serde(default, rename = "isDraft")]
    pub draft: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Branch {
    pub name: String,
    pub commit: Commit,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Commit {
    pub sha: String,
    pub url: String,
}
