use super::traits::{ChatMessage, ChatRequest, ChatResponse};
use super::Provider;
use async_trait::async_trait;
use std::collections::HashMap;

/// Whiskey fork: assemble the final system prompt by stacking three
/// optional segments — the active mode's persona prefix, the active
/// mode's persona-memory block (markdown files), and the caller's
/// upstream system prompt — in that order, separated by `---`.
///
/// All three inputs are independently optional. Returns `None` when
/// every input is `None` (preserves upstream behaviour for
/// `DefaultMode` callers that don't pass a system prompt either).
///
/// Keeping this as a free function makes the unit tests simple — no
/// need to spin up a `RouterProvider` to test the assembly logic.
fn assemble_system_prompt(
    persona_prefix: Option<&str>,
    persona_memory: Option<&str>,
    caller_system_prompt: Option<&str>,
) -> Option<String> {
    let segments: Vec<&str> = [persona_prefix, persona_memory, caller_system_prompt]
        .into_iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("\n\n---\n\n"))
}

/// Whiskey fork: inject the active mode's persona + memory into a
/// `ChatMessage` slice. If the slice already starts with a `system`
/// message, its content is replaced by the assembled prompt (the old
/// content is folded into the assembly as `caller_system_prompt`).
/// If no leading system message exists, one is prepended. When the
/// active mode has neither a prefix nor a memory block (the
/// DefaultMode case), the slice is returned as a `Vec` clone with no
/// other change so the call site doesn't need a separate code path.
///
/// Closes WHISKEY_AUDIT.md C1: the `chat`, `chat_with_history`, and
/// `chat_with_tools` Provider methods all need to inject the persona
/// + memory the same way `chat_with_system` already does, otherwise
/// the agent's tool loop (which calls `chat`) silently drops the
/// Whiskey persona on every interactive turn.
fn inject_active_mode_into_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mode = crate::openhuman::modes::active_mode();
    let prefix = mode.system_prompt_prefix();
    let memory = crate::openhuman::modes::memory_cache::resolve(&*mode);

    // No-op fast path: DefaultMode and any other mode that supplies
    // neither a prefix nor memory just clones the slice.
    if prefix.is_none() && memory.is_none() {
        return messages.to_vec();
    }

    // Locate (and consume) any leading `system` message so its content
    // becomes the `caller_system_prompt` segment of the assembly.
    let (caller_system, rest_start) = match messages.first() {
        Some(first) if first.role == "system" => (Some(first.content.clone()), 1),
        _ => (None, 0),
    };

    let assembled =
        match assemble_system_prompt(prefix, memory.as_deref(), caller_system.as_deref()) {
            Some(s) => s,
            None => return messages.to_vec(),
        };

    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len() + 1);
    out.push(ChatMessage {
        id: None,
        role: "system".into(),
        content: assembled,
        extra_metadata: None,
    });
    out.extend(messages[rest_start..].iter().cloned());
    out
}

/// A single route: maps a task hint to a provider + model combo.
#[derive(Debug, Clone)]
pub struct Route {
    pub provider_name: String,
    pub model: String,
}

/// Multi-model router — routes requests to different provider+model combos
/// based on a task hint encoded in the model parameter.
///
/// The model parameter can be:
/// - A regular model name (e.g. "anthropic/claude-sonnet-4") → uses default provider
/// - A hint-prefixed string (e.g. "hint:reasoning") → resolves via route table
///
/// This wraps multiple pre-created providers and selects the right one per request.
pub struct RouterProvider {
    routes: HashMap<String, (usize, String)>, // hint → (provider_index, model)
    providers: Vec<(String, Box<dyn Provider>)>,
    default_index: usize,
    default_model: String,
}

impl RouterProvider {
    /// Create a new router with a default provider and optional routes.
    ///
    /// `providers` is a list of (name, provider) pairs. The first one is the default.
    /// `routes` maps hint names to Route structs containing provider_name and model.
    pub fn new(
        providers: Vec<(String, Box<dyn Provider>)>,
        routes: Vec<(String, Route)>,
        default_model: String,
    ) -> Self {
        // Build provider name → index lookup
        let name_to_index: HashMap<&str, usize> = providers
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.as_str(), i))
            .collect();

        // Resolve routes to provider indices
        let resolved_routes: HashMap<String, (usize, String)> = routes
            .into_iter()
            .filter_map(|(hint, route)| {
                let index = name_to_index.get(route.provider_name.as_str()).copied();
                match index {
                    Some(i) => Some((hint, (i, route.model))),
                    None => {
                        tracing::warn!(
                            hint = hint,
                            provider = route.provider_name,
                            "Route references unknown provider, skipping"
                        );
                        None
                    }
                }
            })
            .collect();

        Self {
            routes: resolved_routes,
            providers,
            default_index: 0,
            default_model,
        }
    }

    /// Resolve a model parameter to a (provider, actual_model) pair.
    ///
    /// If the model starts with "hint:", look up the hint in the route table.
    /// Otherwise, use the default provider with the given model name.
    /// Resolve a model parameter to a (provider_index, actual_model) pair.
    fn resolve(&self, model: &str) -> (usize, String) {
        if let Some(hint) = model.strip_prefix("hint:") {
            if let Some((idx, resolved_model)) = self.routes.get(hint) {
                return (*idx, resolved_model.clone());
            }
            tracing::warn!(
                hint = hint,
                "Unknown route hint, falling back to default provider"
            );
        }

        // Not a hint or hint not found — use default provider with the model as-is
        (self.default_index, model.to_string())
    }
}

#[async_trait]
impl Provider for RouterProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (provider_idx, resolved_model) = self.resolve(model);

        let (provider_name, provider) = &self.providers[provider_idx];
        // Whiskey fork: assemble the system prompt as
        //   {mode persona prefix}
        //   ---
        //   {mode persona memory block — markdown files under
        //    additional_memory_roots, mtime-cached and bounded}
        //   ---
        //   {caller's existing system prompt}
        // DefaultMode returns None for both prefix and memory, so this
        // is a no-op for upstream behaviour. See `modes::memory_cache`
        // for the cache + size caps + the file-list logic.
        let mode = crate::openhuman::modes::active_mode();
        let prefix = mode.system_prompt_prefix();
        let memory = crate::openhuman::modes::memory_cache::resolve(&*mode);
        let merged_system: Option<String> =
            assemble_system_prompt(prefix, memory.as_deref(), system_prompt);
        let merged_ref = merged_system.as_deref();

        tracing::info!(
            provider = provider_name.as_str(),
            model = resolved_model.as_str(),
            mode = mode.id(),
            "Router dispatching request"
        );

        provider
            .chat_with_system(merged_ref, message, &resolved_model, temperature)
            .await
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (provider_idx, resolved_model) = self.resolve(model);
        let (_, provider) = &self.providers[provider_idx];
        // WHISKEY_AUDIT.md C1: inject persona + memory into the
        // message slice so non-`chat_with_system` paths see Whiskey
        // context too. DefaultMode is a no-op clone.
        let injected = inject_active_mode_into_messages(messages);
        provider
            .chat_with_history(&injected, &resolved_model, temperature)
            .await
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let (provider_idx, resolved_model) = self.resolve(model);
        let (_, provider) = &self.providers[provider_idx];
        // WHISKEY_AUDIT.md C1: this is the path the agent's tool loop
        // takes (`tool_loop::run_tool_call_loop` → `provider.chat`).
        // Without injection here the Whiskey persona + memory are
        // never seen by the LLM during normal interactive turns.
        let injected = inject_active_mode_into_messages(request.messages);
        let injected_request = ChatRequest {
            messages: &injected,
            tools: request.tools,
            stream: request.stream,
        };
        provider
            .chat(injected_request, &resolved_model, temperature)
            .await
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let (provider_idx, resolved_model) = self.resolve(model);
        let (_, provider) = &self.providers[provider_idx];
        // WHISKEY_AUDIT.md C1: same reason as chat() above.
        let injected = inject_active_mode_into_messages(messages);
        provider
            .chat_with_tools(&injected, tools, &resolved_model, temperature)
            .await
    }

    fn supports_native_tools(&self) -> bool {
        self.providers
            .get(self.default_index)
            .map(|(_, p)| p.supports_native_tools())
            .unwrap_or(false)
    }

    fn supports_vision(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_vision())
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        for (name, provider) in &self.providers {
            tracing::info!(provider = name, "Warming up routed provider");
            if let Err(e) = provider.warmup().await {
                tracing::warn!(provider = name, "Warmup failed (non-fatal): {e}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ---- assemble_system_prompt: pure logic, no provider needed ---------

    #[test]
    fn assemble_returns_none_when_all_inputs_are_none() {
        assert_eq!(assemble_system_prompt(None, None, None), None);
    }

    #[test]
    fn assemble_returns_none_when_all_inputs_are_blank() {
        assert_eq!(
            assemble_system_prompt(Some("   "), Some("\n\n"), Some("")),
            None
        );
    }

    #[test]
    fn assemble_passes_caller_prompt_through_unchanged_when_alone() {
        let out = assemble_system_prompt(None, None, Some("you are helpful"))
            .expect("non-empty caller prompt yields a Some");
        assert_eq!(out, "you are helpful");
    }

    #[test]
    fn assemble_stacks_persona_memory_caller_in_order() {
        let out = assemble_system_prompt(Some("PERSONA"), Some("MEMORY"), Some("CALLER"))
            .expect("non-empty");
        // Order matters — persona first so the model sees identity
        // before being asked to act, memory second so it can ground
        // its identity in the user's curated context, caller last so
        // any per-call instructions (e.g. JSON-mode framing) win on
        // close-recency.
        let persona_idx = out.find("PERSONA").expect("persona present");
        let memory_idx = out.find("MEMORY").expect("memory present");
        let caller_idx = out.find("CALLER").expect("caller present");
        assert!(persona_idx < memory_idx);
        assert!(memory_idx < caller_idx);
        // And there's a separator between every pair.
        assert!(out.contains("\n\n---\n\n"));
    }

    #[test]
    fn assemble_drops_only_the_blank_segments() {
        let out = assemble_system_prompt(Some("PERSONA"), Some("   "), Some("CALLER"))
            .expect("non-empty");
        assert!(out.contains("PERSONA"));
        assert!(out.contains("CALLER"));
        // Exactly one separator (between persona and caller) — the
        // blank middle segment is dropped, not joined as an empty
        // line.
        assert_eq!(out.matches("---").count(), 1);
    }

    // ---- inject_active_mode_into_messages: WHISKEY_AUDIT.md C1 ----------
    //
    // The audit caught that `chat`, `chat_with_history`, and
    // `chat_with_tools` skip persona injection — the agent's tool loop
    // calls `chat()` so Whiskey was silently invisible on real
    // interactive turns. These tests pin the new injector's behaviour:
    // DefaultMode is a no-op, WhiskeyMode prepends or replaces a system
    // message in the slice. They serialize via the in-file Mutex used
    // elsewhere in the project's mode-touching tests.

    use std::sync::Mutex as InjectMutex;
    static INJECT_TEST_LOCK: InjectMutex<()> = InjectMutex::new(());

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            id: None,
            role: "user".into(),
            content: content.into(),
            extra_metadata: None,
        }
    }
    fn system_msg(content: &str) -> ChatMessage {
        ChatMessage {
            id: None,
            role: "system".into(),
            content: content.into(),
            extra_metadata: None,
        }
    }

    fn reset_to_default_mode() {
        let _ = crate::openhuman::modes::set_active_mode(crate::openhuman::modes::DefaultMode::ID);
    }

    #[test]
    fn inject_default_mode_is_a_clone_no_op() {
        let _g = INJECT_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_to_default_mode();
        let msgs = vec![system_msg("caller-system"), user_msg("hello")];
        let out = inject_active_mode_into_messages(&msgs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "system");
        assert_eq!(out[0].content, "caller-system");
        assert_eq!(out[1].role, "user");
        assert_eq!(out[1].content, "hello");
    }

    #[test]
    fn inject_whiskey_mode_prepends_system_when_absent() {
        let _g = INJECT_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _ = crate::openhuman::modes::set_active_mode(crate::openhuman::modes::WhiskeyMode::ID);
        let msgs = vec![user_msg("first message — no system prompt")];
        let out = inject_active_mode_into_messages(&msgs);
        // A leading system was prepended.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "system");
        // It contains the Whiskey persona signature substring (the
        // word "trading mentor" appears in WHISKEY_SYSTEM_PREFIX).
        assert!(
            out[0].content.contains("trading mentor"),
            "expected Whiskey persona text in injected system, got: {}",
            out[0].content
        );
        // The original user message is preserved at index 1.
        assert_eq!(out[1].content, "first message — no system prompt");
        reset_to_default_mode();
    }

    #[test]
    fn inject_whiskey_mode_merges_existing_system_message() {
        let _g = INJECT_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _ = crate::openhuman::modes::set_active_mode(crate::openhuman::modes::WhiskeyMode::ID);
        let msgs = vec![system_msg("you are a JSON-only responder"), user_msg("ok")];
        let out = inject_active_mode_into_messages(&msgs);
        // Still two messages — we replace the leading system, don't
        // duplicate it.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "system");
        // Both the persona AND the original caller system content
        // appear in the assembled prompt (caller wins on close-
        // recency for per-call framing like JSON mode).
        assert!(out[0].content.contains("trading mentor"));
        assert!(out[0].content.contains("JSON-only responder"));
        // User message is unchanged.
        assert_eq!(out[1].role, "user");
        assert_eq!(out[1].content, "ok");
        reset_to_default_mode();
    }

    // ---- existing router tests ------------------------------------------

    struct MockProvider {
        calls: Arc<AtomicUsize>,
        response: &'static str,
        last_model: parking_lot::Mutex<String>,
    }

    impl MockProvider {
        fn new(response: &'static str) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                response,
                last_model: parking_lot::Mutex::new(String::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_model(&self) -> String {
            self.last_model.lock().clone()
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_model.lock() = model.to_string();
            Ok(self.response.to_string())
        }
    }

    fn make_router(
        providers: Vec<(&'static str, &'static str)>,
        routes: Vec<(&str, &str, &str)>,
    ) -> (RouterProvider, Vec<Arc<MockProvider>>) {
        let mocks: Vec<Arc<MockProvider>> = providers
            .iter()
            .map(|(_, response)| Arc::new(MockProvider::new(response)))
            .collect();

        let provider_list: Vec<(String, Box<dyn Provider>)> = providers
            .iter()
            .zip(mocks.iter())
            .map(|((name, _), mock)| {
                (
                    name.to_string(),
                    Box::new(Arc::clone(mock)) as Box<dyn Provider>,
                )
            })
            .collect();

        let route_list: Vec<(String, Route)> = routes
            .iter()
            .map(|(hint, provider_name, model)| {
                (
                    hint.to_string(),
                    Route {
                        provider_name: provider_name.to_string(),
                        model: model.to_string(),
                    },
                )
            })
            .collect();

        let router = RouterProvider::new(provider_list, route_list, "default-model".to_string());

        (router, mocks)
    }

    // Arc<MockProvider> should also be a Provider
    #[async_trait]
    impl Provider for Arc<MockProvider> {
        async fn chat_with_system(
            &self,
            system_prompt: Option<&str>,
            message: &str,
            model: &str,
            temperature: f64,
        ) -> anyhow::Result<String> {
            self.as_ref()
                .chat_with_system(system_prompt, message, model, temperature)
                .await
        }
    }

    #[tokio::test]
    async fn routes_hint_to_correct_provider() {
        let (router, mocks) = make_router(
            vec![("fast", "fast-response"), ("smart", "smart-response")],
            vec![
                ("fast", "fast", "llama-3-70b"),
                ("reasoning", "smart", "claude-opus"),
            ],
        );

        let result = router
            .simple_chat("hello", "hint:reasoning", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "smart-response");
        assert_eq!(mocks[1].call_count(), 1);
        assert_eq!(mocks[1].last_model(), "claude-opus");
        assert_eq!(mocks[0].call_count(), 0);
    }

    #[tokio::test]
    async fn routes_fast_hint() {
        let (router, mocks) = make_router(
            vec![("fast", "fast-response"), ("smart", "smart-response")],
            vec![("fast", "fast", "llama-3-70b")],
        );

        let result = router.simple_chat("hello", "hint:fast", 0.5).await.unwrap();
        assert_eq!(result, "fast-response");
        assert_eq!(mocks[0].call_count(), 1);
        assert_eq!(mocks[0].last_model(), "llama-3-70b");
    }

    #[tokio::test]
    async fn unknown_hint_falls_back_to_default() {
        let (router, mocks) = make_router(
            vec![("default", "default-response"), ("other", "other-response")],
            vec![],
        );

        let result = router
            .simple_chat("hello", "hint:nonexistent", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "default-response");
        assert_eq!(mocks[0].call_count(), 1);
        // Falls back to default with the hint as model name
        assert_eq!(mocks[0].last_model(), "hint:nonexistent");
    }

    #[tokio::test]
    async fn non_hint_model_uses_default_provider() {
        let (router, mocks) = make_router(
            vec![
                ("primary", "primary-response"),
                ("secondary", "secondary-response"),
            ],
            vec![("code", "secondary", "codellama")],
        );

        let result = router
            .simple_chat("hello", "anthropic/claude-sonnet-4-20250514", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "primary-response");
        assert_eq!(mocks[0].call_count(), 1);
        assert_eq!(mocks[0].last_model(), "anthropic/claude-sonnet-4-20250514");
    }

    #[test]
    fn resolve_preserves_model_for_non_hints() {
        let (router, _) = make_router(vec![("default", "ok")], vec![]);

        let (idx, model) = router.resolve("gpt-4o");
        assert_eq!(idx, 0);
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn resolve_strips_hint_prefix() {
        let (router, _) = make_router(
            vec![("fast", "ok"), ("smart", "ok")],
            vec![("reasoning", "smart", "claude-opus")],
        );

        let (idx, model) = router.resolve("hint:reasoning");
        assert_eq!(idx, 1);
        assert_eq!(model, "claude-opus");
    }

    #[test]
    fn skips_routes_with_unknown_provider() {
        let (router, _) = make_router(
            vec![("default", "ok")],
            vec![("broken", "nonexistent", "model")],
        );

        // Route should not exist
        assert!(!router.routes.contains_key("broken"));
    }

    #[tokio::test]
    async fn warmup_calls_all_providers() {
        let (router, _) = make_router(vec![("a", "ok"), ("b", "ok")], vec![]);

        // Warmup should not error
        assert!(router.warmup().await.is_ok());
    }

    #[tokio::test]
    async fn chat_with_system_passes_system_prompt() {
        let mock = Arc::new(MockProvider::new("response"));
        let router = RouterProvider::new(
            vec![(
                "default".into(),
                Box::new(Arc::clone(&mock)) as Box<dyn Provider>,
            )],
            vec![],
            "model".into(),
        );

        let result = router
            .chat_with_system(Some("system"), "hello", "model", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "response");
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn chat_with_tools_delegates_to_resolved_provider() {
        let mock = Arc::new(MockProvider::new("tool-response"));
        let router = RouterProvider::new(
            vec![(
                "default".into(),
                Box::new(Arc::clone(&mock)) as Box<dyn Provider>,
            )],
            vec![],
            "model".into(),
        );

        let messages = vec![ChatMessage {
            id: None,
            role: "user".to_string(),
            content: "use tools".to_string(),
            extra_metadata: None,
        }];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "shell",
                "description": "Run shell command",
                "parameters": {}
            }
        })];

        // chat_with_tools should delegate through the router to the mock.
        // MockProvider's default chat_with_tools calls chat_with_history -> chat_with_system.
        let result = router
            .chat_with_tools(&messages, &tools, "model", 0.7)
            .await
            .unwrap();
        assert_eq!(result.text.as_deref(), Some("tool-response"));
        assert_eq!(mock.call_count(), 1);
        assert_eq!(mock.last_model(), "model");
    }

    #[tokio::test]
    async fn chat_with_tools_routes_hint_correctly() {
        let (router, mocks) = make_router(
            vec![("fast", "fast-tool"), ("smart", "smart-tool")],
            vec![("reasoning", "smart", "claude-opus")],
        );

        let messages = vec![ChatMessage {
            id: None,
            role: "user".to_string(),
            content: "reason about this".to_string(),
            extra_metadata: None,
        }];
        let tools = vec![serde_json::json!({"type": "function", "function": {"name": "test"}})];

        let result = router
            .chat_with_tools(&messages, &tools, "hint:reasoning", 0.5)
            .await
            .unwrap();
        assert_eq!(result.text.as_deref(), Some("smart-tool"));
        assert_eq!(mocks[1].call_count(), 1);
        assert_eq!(mocks[1].last_model(), "claude-opus");
        assert_eq!(mocks[0].call_count(), 0);
    }
}
