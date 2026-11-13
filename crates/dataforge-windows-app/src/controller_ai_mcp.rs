//! Live AI Assistant drawer (BYOK + local Ollama), local MCP server toggle,
//! and Settings / VS Code theme file import for [`AppController`].

use std::path::Path;

use dataforge_services::{AiAssistantService, AiEndpointConfig};
use secrecy::SecretString;
use tablepro_ai::{ChatMessage, QueryLanguage, ReplyEvent};
use tablepro_core::AiProvider;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::AppController;
use crate::error::{AppError, Result};
use crate::views::ai_drawer::AiMessage;

impl AppController {
    /// Stores a BYOK API key for `provider` in Windows Credential Manager.
    pub fn store_ai_api_key_blocking(&self, provider: AiProvider, key: &str) -> Result<()> {
        let services = self.require_services()?;
        let ai = services
            .ai
            .as_ref()
            .ok_or_else(|| AppError::InvalidState("AI service unavailable".to_owned()))?;
        let secret = SecretString::from(key.to_owned());
        services
            .runtime
            .block_on(ai.store_api_key(provider, &secret))?;
        Ok(())
    }

    /// Sends `prompt` to the configured AI endpoint (BYOK or local Ollama) with current
    /// schema context (never row data), streaming [`ReplyEvent`]s into `ai_drawer.messages`.
    pub fn send_ai_prompt_blocking(
        &mut self,
        prompt: &str,
        config: &AiEndpointConfig,
    ) -> Result<String> {
        let services = self.require_services()?.clone();
        let ai = services
            .ai
            .as_ref()
            .ok_or_else(|| AppError::InvalidState("AI service unavailable".to_owned()))?
            .clone();

        self.ai_drawer.messages.push(AiMessage {
            is_user: true,
            text: prompt.to_owned(),
        });
        self.ai_drawer.draft_prompt.clear();

        let tables: Vec<String> = self.cached_objects.iter().map(|o| o.name.clone()).collect();
        let schema_ctx = AiAssistantService::build_schema_context(
            self.toolbar_status.engine_version.clone(),
            QueryLanguage::Sql,
            Some(self.toolbar_status.database.clone()),
            self.cached_schemas.first().map(|s| s.name.clone()),
            tables,
            Vec::new(),
        );

        let messages = vec![ChatMessage::user(prompt)];
        let cancel = CancellationToken::new();
        let (tx, mut rx) = mpsc::unbounded_channel::<ReplyEvent>();
        let editor_sql = self.editor_sql_text();

        services.runtime.block_on(async {
            let endpoint = ai.resolve_endpoint(config).await?;
            ai.stream_chat(
                &endpoint,
                &schema_ctx,
                Some(&editor_sql),
                messages,
                cancel,
                tx,
            )
            .await;
            while let Some(ev) = rx.recv().await {
                self.apply_ai_reply_event(ev);
            }
            Ok::<_, dataforge_services::ServiceError>(())
        })?;

        let reply_text = self
            .ai_drawer
            .messages
            .last()
            .filter(|m| !m.is_user)
            .map(|m| m.text.clone())
            .unwrap_or_default();
        Ok(reply_text)
    }

    /// Applies a single streamed [`ReplyEvent`] into `ai_drawer.messages`.
    pub fn apply_ai_reply_event(&mut self, event: ReplyEvent) {
        if let ReplyEvent::Text(delta) = event {
            if let Some(last) = self.ai_drawer.messages.last_mut()
                && !last.is_user
            {
                last.text.push_str(&delta);
            } else {
                self.ai_drawer.messages.push(AiMessage {
                    is_user: false,
                    text: delta,
                });
            }
        }
    }

    /// Starts or stops the local loopback MCP server and updates `settings.mcp.enabled`.
    pub fn toggle_mcp_server(&mut self, enable: bool) -> Result<Option<String>> {
        let services = self.require_services()?.clone();
        let addr = services.toggle_mcp_server(enable)?;
        self.settings.mcp.enabled = enable;
        Ok(addr)
    }

    /// Returns whether the local MCP server is currently running.
    #[must_use]
    pub fn is_mcp_server_running(&self) -> bool {
        self.services
            .as_ref()
            .is_some_and(|s| s.is_mcp_running())
    }

    /// Imports a VS Code or DataForge theme JSON file from `path`.
    pub fn import_vscode_theme_file(&mut self, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path)?;
        self.import_vscode_theme(&bytes)
            .map_err(AppError::InvalidState)
    }

    /// Persists `settings` atomically to `path`.
    pub fn save_settings_to(&self, path: &Path) -> Result<()> {
        self.settings.save_atomic(path)?;
        Ok(())
    }
}
