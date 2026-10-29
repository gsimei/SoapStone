use crate::{HttpResponse, Result, Store, StoreError};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub status: u16,
    pub time_ms: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestStep {
    pub request_id: i64,
    pub expected_status: Option<u16>,
    pub not_fault: bool,
    pub xpath: String,
    pub expected_value: Option<String>,
    pub extract_variable: String,
}

impl Store {
    pub fn history(&self, request_id: i64) -> Result<Vec<HistoryEntry>> {
        let mut stmt =
            self.conn.prepare("SELECT id, created_at, status, time_ms, size FROM history WHERE request_id = ?1 ORDER BY id DESC")?;
        Ok(stmt
            .query_map([request_id], |r| {
                Ok(HistoryEntry {
                    id: r.get(0)?,
                    created_at: r.get(1)?,
                    status: r.get(2)?,
                    time_ms: r.get::<_, i64>(3)? as u64,
                    size: r.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn history_response(&self, id: i64) -> Result<HttpResponse> {
        let json: String = self.conn.query_row("SELECT response FROM history WHERE id = ?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn clear_history(&self, request_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM history WHERE request_id = ?1", [request_id])?;
        Ok(())
    }

    pub fn test_plan(&self, project_id: i64) -> Result<Vec<TestStep>> {
        use rusqlite::OptionalExtension;
        let json: Option<String> =
            self.conn.query_row("SELECT steps FROM test_plans WHERE project_id = ?1", [project_id], |r| r.get(0)).optional()?;
        Ok(serde_json::from_str(json.as_deref().unwrap_or("[]"))?)
    }

    pub fn save_test_plan(&self, project_id: i64, steps: &[TestStep]) -> Result<()> {
        if steps.len() > 100 {
            return Err(StoreError::Invalid("A sequence supports up to 100 steps.".into()));
        }
        for step in steps {
            let belongs: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM requests r JOIN operations o ON o.id = r.operation_id JOIN services s ON s.id = o.service_id WHERE r.id = ?1 AND s.project_id = ?2)", params![step.request_id, project_id], |r| r.get(0))?;
            if !belongs {
                return Err(StoreError::Invalid("A sequence request does not belong to this project.".into()));
            }
            if !step.extract_variable.is_empty() && !valid_variable_name(&step.extract_variable) {
                return Err(StoreError::Invalid("Invalid extraction variable name.".into()));
            }
            if !step.extract_variable.is_empty() && step.xpath.trim().is_empty() {
                return Err(StoreError::Invalid("An extraction needs an XPath expression.".into()));
            }
        }
        self.conn.execute(
            "INSERT INTO test_plans (project_id, steps) VALUES (?1, ?2) ON CONFLICT(project_id) DO UPDATE SET steps = excluded.steps",
            params![project_id, serde_json::to_string(steps)?],
        )?;
        Ok(())
    }
}

pub fn valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}
