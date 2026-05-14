use super::{is_terminal_run_status, is_terminal_step_status, parse_model_ref, parse_usage, SqliteSessionRepository};
use anyhow::{Context, Result};
use sqlx::Row;

impl SqliteSessionRepository {
    pub async fn create_run(&self, run: &crate::conversation::model::RunRecord) -> Result<()> {
        let agent_id: String = sqlx::query("SELECT agent_id FROM sessions WHERE id = ?")
            .bind(&run.session_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to load session agent_id for run record")?
            .get("agent_id");
        let orchestration_model = run
            .orchestration_model
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let execution_model = run.execution_model.as_ref().map(serde_json::to_string).transpose()?;

        let usage_json = run.usage.as_ref().map(serde_json::to_string).transpose()?;
        sqlx::query(
            "INSERT INTO runs (run_id, session_id, turn_id, agent_id, status, started_at, finished_at, duration_ms, orchestration_model, execution_model, usage, error_summary, waiting_reason) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run.id)
        .bind(&run.session_id)
        .bind(&run.id)
        .bind(agent_id)
        .bind(&run.status)
        .bind(run.created_at)
        .bind(if is_terminal_run_status(&run.status) { Some(run.updated_at) } else { None })
        .bind(if is_terminal_run_status(&run.status) { Some(run.updated_at - run.created_at) } else { None })
        .bind(orchestration_model)
        .bind(execution_model)
        .bind(usage_json)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run_usage(&self, run_id: &str, usage: &serde_json::Value) -> Result<()> {
        let usage_json = serde_json::to_string(usage)?;
        sqlx::query("UPDATE runs SET usage = ? WHERE run_id = ?")
            .bind(usage_json)
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_run_status(&self, id: &str, status: &str, now: i64) -> Result<()> {
        sqlx::query(
            "UPDATE runs SET status = ?, finished_at = CASE WHEN ? THEN ? ELSE finished_at END, duration_ms = CASE WHEN ? THEN (? - started_at) ELSE duration_ms END WHERE run_id = ?",
        )
        .bind(status)
        .bind(is_terminal_run_status(status))
        .bind(now)
        .bind(is_terminal_run_status(status))
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_run_step(&self, step: &crate::conversation::model::RunStepRecord) -> Result<()> {
        let payload_json = serde_json::to_string(&serde_json::json!({
            "input": step.input,
            "output": step.output,
        }))?;
        let title = step.step_type.clone();

        sqlx::query(
            "INSERT INTO run_steps (step_id, run_id, step_type, title, tool_name, status, started_at, finished_at, payload) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&step.id)
        .bind(&step.run_id)
        .bind(&step.step_type)
        .bind(title)
        .bind(Option::<String>::None)
        .bind(&step.status)
        .bind(step.created_at)
        .bind(if is_terminal_step_status(&step.status) { Some(step.updated_at) } else { None })
        .bind(payload_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_run_step(
        &self,
        id: &str,
        status: &str,
        output: Option<&serde_json::Value>,
        now: i64,
    ) -> Result<()> {
        let existing_payload: Option<String> = sqlx::query("SELECT payload FROM run_steps WHERE step_id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.get("payload"));
        let mut payload = existing_payload
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(output) = output {
            payload["output"] = output.clone();
        }
        let payload_json = serde_json::to_string(&payload)?;

        sqlx::query(
            "UPDATE run_steps SET status = ?, payload = ?, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE step_id = ?",
        )
        .bind(status)
        .bind(payload_json)
        .bind(is_terminal_step_status(status))
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_runs(&self, session_id: &str) -> Result<Vec<crate::conversation::model::RunRecord>> {
        let rows = sqlx::query(
            "SELECT run_id, session_id, status, started_at, COALESCE(finished_at, started_at) AS updated_at, orchestration_model, execution_model, usage, (SELECT COUNT(*) FROM run_steps WHERE run_steps.run_id = runs.run_id AND run_steps.step_type = 'tool_use') AS tool_call_count FROM runs WHERE session_id = ? ORDER BY started_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut runs = Vec::new();
        for row in rows {
            let orchestration_model = parse_model_ref(row.get("orchestration_model"))?;
            let execution_model = parse_model_ref(row.get("execution_model"))?;
            let usage = parse_usage(row.get("usage"))?;
            runs.push(crate::conversation::model::RunRecord {
                id: row.get("run_id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("started_at"),
                updated_at: row.get("updated_at"),
                orchestration_model,
                execution_model,
                tool_call_count: Some(row.get::<i64, _>("tool_call_count") as u32),
                usage,
            });
        }
        Ok(runs)
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<crate::conversation::model::RunRecord>> {
        let row = sqlx::query(
            "SELECT run_id, session_id, status, started_at, COALESCE(finished_at, started_at) AS updated_at, orchestration_model, execution_model, usage, (SELECT COUNT(*) FROM run_steps WHERE run_steps.run_id = runs.run_id AND run_steps.step_type = 'tool_use') AS tool_call_count FROM runs WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let orchestration_model = parse_model_ref(row.get("orchestration_model"))?;
            let execution_model = parse_model_ref(row.get("execution_model"))?;
            let usage = parse_usage(row.get("usage"))?;
            Ok(Some(crate::conversation::model::RunRecord {
                id: row.get("run_id"),
                session_id: row.get("session_id"),
                status: row.get("status"),
                created_at: row.get("started_at"),
                updated_at: row.get("updated_at"),
                orchestration_model,
                execution_model,
                tool_call_count: Some(row.get::<i64, _>("tool_call_count") as u32),
                usage,
            }))
        } else {
            Ok(None)
        }
    }
}
