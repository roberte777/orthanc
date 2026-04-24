use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingValueType {
    String,
    Integer,
    Boolean,
    Json,
}

#[derive(Debug, Clone)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub value_type: SettingValueType,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertSetting {
    pub value: String,
    pub value_type: SettingValueType,
    pub description: Option<String>,
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Setting>>;
    async fn upsert(&self, key: &str, input: UpsertSetting) -> Result<Setting>;
    async fn list(&self) -> Result<Vec<Setting>>;
    async fn delete(&self, key: &str) -> Result<bool>;
}
