use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStage {
    Idle,
    SyncingRepo,
    LoadingPrs,
    ReviewingPr,
    FixingPr,
    PushingChanges,
    Completed,
    Failed,
}

impl ExecutionStage {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::SyncingRepo => "Syncing repository",
            Self::LoadingPrs => "Loading PR list",
            Self::ReviewingPr => "Reviewing PR",
            Self::FixingPr => "Auto fixing",
            Self::PushingChanges => "Pushing changes",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub repo_path: String,
    pub repo_clone_url: String,
    pub default_branch: String,
    pub max_prs_per_run: usize,
    pub max_command_retries: u8,
    pub retry_delay_seconds: u64,
    pub review_command_template: String,
    pub fix_command_template: String,
    pub auto_push_enabled: bool,
}

pub fn default_review_template() -> String {
    "codex review --base {{DEFAULT_BRANCH}}".to_string()
}

pub fn default_fix_template() -> String {
    "codex exec \"You are in a checked-out PR branch. Read findings and fix issues for PR #{{PR_NUMBER}} ({{PR_TITLE}}). Use report context at {{REPORT_PATH}} when relevant. Make minimal safe changes and update tests if needed.\"".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            repo_path: String::new(),
            repo_clone_url: String::new(),
            default_branch: "main".to_string(),
            max_prs_per_run: 20,
            max_command_retries: 2,
            retry_delay_seconds: 15,
            review_command_template: default_review_template(),
            fix_command_template: default_fix_template(),
            auto_push_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EngineState {
    pub processed_pr_numbers: Vec<u64>,
    pub last_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrAuthor {
    pub login: String,
    pub name: Option<String>,
}

impl Default for PrAuthor {
    fn default() -> Self {
        Self {
            login: "unknown".to_string(),
            name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenPr {
    pub number: u64,
    pub title: String,
    #[serde(rename = "headRefName")]
    pub head_ref_name: String,
    pub url: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub author: PrAuthor,
    pub assignees: serde_json::Value,
    pub reviews: serde_json::Value,
    #[serde(rename = "reviewRequests")]
    pub review_requests: serde_json::Value,
    pub comments: serde_json::Value,
    #[serde(rename = "latestReviews")]
    pub latest_reviews: serde_json::Value,
}

impl Default for OpenPr {
    fn default() -> Self {
        Self {
            number: 0,
            title: String::new(),
            head_ref_name: String::new(),
            url: String::new(),
            updated_at: String::new(),
            author: PrAuthor::default(),
            assignees: serde_json::Value::Null,
            reviews: serde_json::Value::Null,
            review_requests: serde_json::Value::Null,
            comments: serde_json::Value::Null,
            latest_reviews: serde_json::Value::Null,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrExecutionResult {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub review_exit_code: i32,
    pub fix_exit_code: i32,
    pub pushed: bool,
    pub report_path: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RunSnapshot {
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub stage: ExecutionStage,
    pub total_prs: usize,
    pub current_index: usize,
    pub current_pr_number: Option<u64>,
    pub current_pr_title: Option<String>,
    pub error_message: Option<String>,
    pub report: Vec<PrExecutionResult>,
    pub log_lines: Vec<String>,
}

impl Default for RunSnapshot {
    fn default() -> Self {
        Self {
            started_at: None,
            finished_at: None,
            status: RunStatus::Idle,
            stage: ExecutionStage::Idle,
            total_prs: 0,
            current_index: 0,
            current_pr_number: None,
            current_pr_title: None,
            error_message: None,
            report: Vec::new(),
            log_lines: Vec::new(),
        }
    }
}
