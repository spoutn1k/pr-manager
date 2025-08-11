use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum Mergeable {
    #[serde(rename = "MERGEABLE")]
    Ok,
    #[serde(rename = "CONFLICTING")]
    Conflict,
    #[serde(rename = "UNKNOWN")]
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
    #[serde(default, rename = "statusCheckRollup")]
    pub checks: Vec<CheckData>,
}

pub enum CheckProgress {
    Pending,
    Success,
    Failure,
}

impl PullRequest {
    pub fn check_status(&self) -> CheckProgress {
        let mut pending = 0;

        for check in &self.checks {
            if check.verdict() == CheckConclusion::Failure {
                return CheckProgress::Failure;
            }

            if let CheckStatus::InProgress | CheckStatus::Queued = check.state() {
                pending += 1;
            }
        }

        if pending > 0 {
            CheckProgress::Pending
        } else {
            CheckProgress::Success
        }
    }

    pub fn checks_passing(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.verdict() == CheckConclusion::Success)
            .count()
    }

    pub fn checks_scheduled(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.verdict() != CheckConclusion::Skipped)
            .count()
    }
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Owner {
    pub login: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Repo {
    pub name: String,
    pub owner: Owner,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum CheckConclusion {
    #[serde(rename = "SUCCESS")]
    Success,
    #[serde(rename = "FAILURE")]
    Failure,
    #[serde(rename = "SKIPPED")]
    Skipped,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    #[serde(rename = "")]
    Unknown,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum CheckStatus {
    #[serde(rename = "COMPLETED")]
    Completed,
    #[serde(rename = "IN_PROGRESS")]
    InProgress,
    #[serde(rename = "QUEUED")]
    Queued,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "__typename")]
pub enum CheckData {
    CheckRun {
        name: String,
        conclusion: CheckConclusion,
        status: CheckStatus,
    },
    StatusContext {
        context: String,
        state: CheckConclusion,
        #[serde(rename = "targetUrl")]
        target_url: String,
    },
}

impl CheckData {
    pub fn name(&self) -> &str {
        match self {
            CheckData::CheckRun { name, .. } => name,
            CheckData::StatusContext { context, .. } => context,
        }
    }

    pub fn verdict(&self) -> CheckConclusion {
        match self {
            CheckData::CheckRun { conclusion, .. } => conclusion.clone(),
            CheckData::StatusContext { state, .. } => state.clone(),
        }
    }

    pub fn state(&self) -> CheckStatus {
        match self {
            CheckData::CheckRun { status, .. } => status.clone(),
            CheckData::StatusContext { .. } => CheckStatus::Completed,
        }
    }
}

#[test]
fn test_parse_json() {
    let prs: Vec<PullRequest> = serde_json::from_str(include_str!("fixtures/full.json")).unwrap();

    insta::assert_debug_snapshot!(prs);
}
