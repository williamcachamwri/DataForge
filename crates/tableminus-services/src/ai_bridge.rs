//! AI assistant service wiring `tablepro_ai` (BYOK via `WindowsCredentialStore` + local Ollama)
//! with database schema context.

use std::sync::Arc;

use secrecy::SecretString;
use tableminus_windows_platform::WindowsCredentialStore;
use tablepro_ai::{
    AiClient, ApiKeyVault, ChatMessage, ChatRequest, Endpoint, QueryLanguage, ReplyEnd, ReplyEvent,
    SchemaContext, TableShape, chat_system_prompt, collect_reply, completion_message,
    completion_system_prompt, list_models, stream_reply,
};
use tablepro_core::AiProvider;
use tablepro_core::column::ColumnInfo;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::ServiceError;

/// Default local Ollama OpenAI-compatible endpoint URL.
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434/v1";

/// Default Anthropic Messages API base URL.
pub const DEFAULT_ANTHROPIC_URL: &str = "https://api.anthropic.com/v1";

/// Default OpenAI Chat Completions API base URL.
pub const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1";

/// Configuration for building an AI [`Endpoint`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiEndpointConfig {
    pub provider: AiProvider,
    pub base_url: String,
    pub model: String,
}

impl AiEndpointConfig {
    /// Creates a configuration targeting a local Ollama instance.
    #[must_use]
    pub fn ollama(model: impl Into<String>) -> Self {
        Self {
            provider: AiProvider::OpenAiCompatible,
            base_url: DEFAULT_OLLAMA_URL.to_owned(),
            model: model.into(),
        }
    }

    /// Creates a configuration targeting Anthropic's Messages API.
    #[must_use]
    pub fn anthropic(model: impl Into<String>) -> Self {
        Self {
            provider: AiProvider::Anthropic,
            base_url: DEFAULT_ANTHROPIC_URL.to_owned(),
            model: model.into(),
        }
    }

    /// Creates a configuration targeting OpenAI's Chat Completions API.
    #[must_use]
    pub fn openai(model: impl Into<String>) -> Self {
        Self {
            provider: AiProvider::OpenAi,
            base_url: DEFAULT_OPENAI_URL.to_owned(),
            model: model.into(),
        }
    }
}

/// Service managing AI keys in [`WindowsCredentialStore`], endpoint resolution,
/// schema context construction, and streaming/one-shot AI requests.
pub struct AiAssistantService {
    client: AiClient,
    credentials: Arc<WindowsCredentialStore>,
}

impl AiAssistantService {
    /// Creates a new [`AiAssistantService`] using the given [`WindowsCredentialStore`].
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if the underlying HTTP/TLS client cannot be initialized.
    pub fn new(credentials: Arc<WindowsCredentialStore>) -> Result<Self, ServiceError> {
        let client = AiClient::new(env!("CARGO_PKG_VERSION"))
            .map_err(|err| ServiceError::Ai(err.to_string()))?;
        Ok(Self {
            client,
            credentials,
        })
    }

    /// Stores a BYOK API key for `provider` in Windows Credential Manager.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if storing the key fails.
    pub async fn store_api_key(
        &self,
        provider: AiProvider,
        key: &SecretString,
    ) -> Result<(), ServiceError> {
        self.credentials
            .store_key(provider, key)
            .await
            .map_err(|err| ServiceError::Ai(err.to_string()))
    }

    /// Loads a stored BYOK API key for `provider` from Windows Credential Manager.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if reading the key fails.
    pub async fn load_api_key(
        &self,
        provider: AiProvider,
    ) -> Result<Option<SecretString>, ServiceError> {
        self.credentials
            .load_key(provider)
            .await
            .map_err(|err| ServiceError::Ai(err.to_string()))
    }

    /// Deletes a stored BYOK API key for `provider` from Windows Credential Manager.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if deleting the key fails.
    pub async fn delete_api_key(&self, provider: AiProvider) -> Result<(), ServiceError> {
        self.credentials
            .delete_key(provider)
            .await
            .map_err(|err| ServiceError::Ai(err.to_string()))
    }

    /// Resolves an [`Endpoint`] from `config`, loading the BYOK key from Windows Credential
    /// Manager when the provider requires one.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if key retrieval or endpoint validation fails.
    pub async fn resolve_endpoint(
        &self,
        config: &AiEndpointConfig,
    ) -> Result<Endpoint, ServiceError> {
        let key = if config.provider.requires_key() {
            self.load_api_key(config.provider).await?
        } else {
            None
        };
        Endpoint::new(config.provider, &config.base_url, &config.model, key)
            .map_err(|err| ServiceError::Ai(err.to_string()))
    }

    /// Lists available models from `endpoint`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if the provider request fails.
    pub async fn list_models(&self, endpoint: &Endpoint) -> Result<Vec<String>, ServiceError> {
        list_models(&self.client, endpoint)
            .await
            .map_err(|err| ServiceError::Ai(err.to_string()))
    }

    /// Builds a [`SchemaContext`] from catalog metadata (never including row data).
    #[must_use]
    pub fn build_schema_context(
        engine: impl Into<String>,
        language: QueryLanguage,
        database: Option<String>,
        schema: Option<String>,
        tables: Vec<String>,
        described: Vec<(String, Vec<ColumnInfo>)>,
    ) -> SchemaContext {
        let shapes = described
            .into_iter()
            .map(|(name, cols)| TableShape {
                name,
                columns: cols
                    .into_iter()
                    .map(|c| tablepro_ai::ColumnShape {
                        name: c.name,
                        type_name: format!("{:?}", c.column_type),
                        nullable: c.nullable,
                        primary_key: c.primary_key,
                    })
                    .collect(),
            })
            .collect();

        SchemaContext {
            engine: engine.into(),
            language,
            database,
            schema,
            tables,
            described: shapes,
        }
    }

    /// Streams a chat reply with `schema_context` into `events`.
    pub async fn stream_chat(
        &self,
        endpoint: &Endpoint,
        schema_context: &SchemaContext,
        editor_sql: Option<&str>,
        messages: Vec<ChatMessage>,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ReplyEvent>,
    ) {
        let request = ChatRequest {
            system: chat_system_prompt(schema_context, editor_sql),
            messages,
            max_tokens: ChatRequest::CHAT_MAX_TOKENS,
        };
        stream_reply(&self.client, endpoint, &request, cancel, events).await;
    }

    /// Requests a one-shot inline SQL/command completion using `schema_context`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Ai`] if the completion request fails.
    pub async fn complete_inline(
        &self,
        endpoint: &Endpoint,
        schema_context: &SchemaContext,
        before_cursor: &str,
        after_cursor: &str,
    ) -> Result<(String, ReplyEnd), ServiceError> {
        let request = ChatRequest {
            system: completion_system_prompt(schema_context),
            messages: vec![ChatMessage::user(completion_message(
                before_cursor,
                after_cursor,
            ))],
            max_tokens: ChatRequest::SUGGESTION_MAX_TOKENS,
        };
        collect_reply(&self.client, endpoint, &request)
            .await
            .map_err(|err| ServiceError::Ai(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::{ColumnDefault, ColumnType};

    #[test]
    fn endpoint_configs_set_expected_providers_and_urls() {
        let ollama = AiEndpointConfig::ollama("llama3.2");
        assert_eq!(ollama.provider, AiProvider::OpenAiCompatible);
        assert_eq!(ollama.base_url, DEFAULT_OLLAMA_URL);
        assert_eq!(ollama.model, "llama3.2");

        let anthropic = AiEndpointConfig::anthropic("claude-sonnet-4-20250514");
        assert_eq!(anthropic.provider, AiProvider::Anthropic);
        assert_eq!(anthropic.base_url, DEFAULT_ANTHROPIC_URL);

        let openai = AiEndpointConfig::openai("gpt-4.1");
        assert_eq!(openai.provider, AiProvider::OpenAi);
        assert_eq!(openai.base_url, DEFAULT_OPENAI_URL);
    }

    #[tokio::test]
    async fn resolve_endpoint_allows_local_ollama_without_key() -> Result<(), ServiceError> {
        let creds = Arc::new(WindowsCredentialStore::new("TableMinusTestAi/"));
        let service = AiAssistantService::new(creds)?;
        let endpoint = service
            .resolve_endpoint(&AiEndpointConfig::ollama("qwen2.5-coder"))
            .await?;
        assert_eq!(endpoint.kind(), AiProvider::OpenAiCompatible);
        assert_eq!(endpoint.model(), "qwen2.5-coder");
        Ok(())
    }

    #[test]
    fn build_schema_context_maps_columns_without_row_data() {
        let cols = vec![ColumnInfo {
            name: "id".to_owned(),
            column_type: ColumnType::untyped_text("INTEGER"),
            nullable: false,
            primary_key: true,
            is_auto_increment: true,
            is_generated: false,
            partition_scoped: false,
            default: ColumnDefault::None,
            comment: None,
        }];
        let ctx = AiAssistantService::build_schema_context(
            "SQLite",
            QueryLanguage::Sql,
            Some("main".to_owned()),
            None,
            vec!["users".to_owned()],
            vec![("users".to_owned(), cols)],
        );
        assert_eq!(ctx.engine, "SQLite");
        assert_eq!(ctx.tables, vec!["users"]);
        assert_eq!(ctx.described.len(), 1);
        assert_eq!(ctx.described[0].columns[0].name, "id");
        assert!(ctx.described[0].columns[0].primary_key);
    }
}
