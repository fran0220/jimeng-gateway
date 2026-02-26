use anyhow::Result;
use bollard::Docker;
use bollard::container::LogsOptions;
use futures::StreamExt;

/// Provides access to Docker container logs.
#[derive(Clone)]
pub struct DockerService {
    client: Docker,
    container_name: String,
}

impl DockerService {
    pub fn new(container_name: &str) -> Result<Self> {
        let client = Docker::connect_with_socket_defaults()?;
        Ok(Self {
            client,
            container_name: container_name.to_string(),
        })
    }

    /// Fetch recent container logs.
    pub async fn get_logs(&self, lines: usize, since: Option<i64>) -> Result<Vec<String>> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: lines.to_string(),
            since: since.unwrap_or(0),
            timestamps: true,
            ..Default::default()
        };

        let mut stream = self.client.logs(&self.container_name, Some(options));
        let mut logs = Vec::new();

        while let Some(log) = stream.next().await {
            match log {
                Ok(output) => logs.push(output.to_string()),
                Err(e) => {
                    tracing::warn!("Error reading log: {e}");
                    break;
                }
            }
        }

        Ok(logs)
    }

    /// Check if the container is running.
    pub async fn is_running(&self) -> bool {
        match self.client.inspect_container(&self.container_name, None).await {
            Ok(info) => info
                .state
                .and_then(|s| s.running)
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Get container stats (CPU, memory).
    pub async fn container_info(&self) -> Result<serde_json::Value> {
        let info = self.client.inspect_container(&self.container_name, None).await?;
        let state = info.state.unwrap_or_default();

        Ok(serde_json::json!({
            "name": self.container_name,
            "running": state.running.unwrap_or(false),
            "status": state.status.map(|s| format!("{s:?}")),
            "started_at": state.started_at,
            "pid": state.pid,
        }))
    }
}
