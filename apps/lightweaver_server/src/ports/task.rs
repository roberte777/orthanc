use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    LibraryScan,
    MetadataRefresh,
    Transcode,
    ThumbnailGeneration,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: i64,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub priority: i64,
    pub library_id: Option<i64>,
    pub media_item_id: Option<i64>,
    pub transcoding_profile_id: Option<i64>,
    pub progress_percentage: Option<i64>,
    pub current_step: Option<String>,
    pub error_message: Option<String>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTask {
    pub task_type: TaskType,
    pub priority: i64,
    pub library_id: Option<i64>,
    pub media_item_id: Option<i64>,
    pub transcoding_profile_id: Option<i64>,
    pub scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub progress_percentage: i64,
    pub current_step: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskListFilter {
    pub status: Option<TaskStatus>,
    pub task_type: Option<TaskType>,
    pub library_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create(&self, input: NewTask) -> Result<Task>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Task>>;
    async fn list(&self, filter: TaskListFilter) -> Result<Vec<Task>>;
    async fn claim_next_pending(&self, task_type: Option<TaskType>) -> Result<Option<Task>>;

    async fn mark_running(&self, id: i64) -> Result<Task>;
    async fn mark_completed(&self, id: i64) -> Result<Task>;
    async fn mark_failed(&self, id: i64, error_message: &str) -> Result<Task>;
    async fn mark_cancelled(&self, id: i64) -> Result<Task>;
    async fn update_progress(&self, id: i64, progress: TaskProgress) -> Result<()>;

    async fn delete(&self, id: i64) -> Result<bool>;
    async fn delete_completed_before(&self, before: DateTime<Utc>) -> Result<u64>;
}
