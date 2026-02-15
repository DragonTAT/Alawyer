use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use uuid::Uuid;

mod agent;
mod error;
mod model;
mod retrieval;
mod safety;
mod storage;
mod tools;

use agent::{
    advance_intake_index, build_report, collect_facts, format_facts_summary, intake_state,
    mark_intake_done, save_answer, start_intake, AgentPhase,
};
use error::{CoreError, CoreResult};
use model::{ModelConnector, OpenRouterConfig, RetryConfig};
use retrieval::{KnowledgeInfo, RetrievalEngine, SearchResult};
use safety::{SafetyCheckResult, SafetyEngine, Severity};
use storage::{LogEntry, Message, Session, SqliteStorage};
use tools::{ToolContext, ToolRegistry};

static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime")
});

#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreConfig {
    pub kb_path: String,
    pub db_path: String,
    pub max_iterations: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ModelConfig {
    pub api_key: String,
    pub model_name: String,
    pub base_url: Option<String>,
    pub retry_max_retries: u32,
    pub retry_initial_delay_ms: u64,
    pub retry_max_delay_ms: u64,
    pub retry_backoff_factor: f64,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model_name: "openrouter/free".to_owned(),
            base_url: None,
            retry_max_retries: 3,
            retry_initial_delay_ms: 200,
            retry_max_delay_ms: 10_000,
            retry_backoff_factor: 2.0,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreEvent {
    pub kind: String,
    pub payload: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Subscription {
    pub id: u64,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum ToolResponse {
    Allow { always: bool },
    AllowAllThisSession,
    Deny,
}

#[uniffi::export(callback_interface)]
pub trait EventListener: Send + Sync {
    fn on_event(&self, event: CoreEvent);
}

#[derive(Default)]
struct TaskControl {
    cancelled: AtomicBool,
}

impl TaskControl {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

struct PendingToolCall {
    sender: mpsc::Sender<ToolResponse>,
    session_id: String,
    tool_name: String,
}

#[derive(uniffi::Object)]
pub struct Core {
    kb_path: String,
    max_iterations: u32,
    storage: Arc<SqliteStorage>,
    retrieval: Arc<RetrievalEngine>,
    safety: Arc<SafetyEngine>,
    tools: Arc<ToolRegistry>,
    model_connector: Arc<RwLock<Option<ModelConnector>>>,
    listeners: Arc<Mutex<HashMap<u64, Arc<dyn EventListener>>>>,
    next_listener_id: AtomicU64,
    task_controls: Arc<Mutex<HashMap<String, Arc<TaskControl>>>>,
    pending_tool_calls: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    session_allow_all: Arc<Mutex<HashSet<String>>>,
    /// Per-session lock: ensures only one AgentWorker runs per session at a time
    session_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[uniffi::export]
impl Core {
    #[uniffi::constructor]
    pub fn new(config: CoreConfig) -> CoreResult<Arc<Self>> {
        if config.max_iterations == 0 {
            return Err(CoreError::Config("max_iterations must be > 0".to_owned()));
        }

        if !config.kb_path.is_empty() {
            let kb_path = Path::new(&config.kb_path);
            if !kb_path.exists() {
                std::fs::create_dir_all(kb_path)
                    .map_err(|e| CoreError::Config(format!("failed to create kb_path: {e}")))?;
            }
        }

        if let Some(parent) = Path::new(&config.db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CoreError::Config(format!("failed to create db directory: {e}")))?;
        }

        let storage = Arc::new(SqliteStorage::new(&config.db_path)?);
        let retrieval = Arc::new(RetrievalEngine::new(&config.kb_path));
        let safety = Arc::new(SafetyEngine::default());
        let tools = Arc::new(ToolRegistry::with_builtins());

        Ok(Arc::new(Self {
            kb_path: config.kb_path,
            max_iterations: config.max_iterations,
            storage,
            retrieval,
            safety,
            tools,
            model_connector: Arc::new(RwLock::new(None)),
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_listener_id: AtomicU64::new(1),
            task_controls: Arc::new(Mutex::new(HashMap::new())),
            pending_tool_calls: Arc::new(Mutex::new(HashMap::new())),
            session_allow_all: Arc::new(Mutex::new(HashSet::new())),
            session_locks: Arc::new(Mutex::new(HashMap::new())),
        }))
    }

    pub fn hello(&self) -> String {
        "hello from alawyer-core (rust)".to_owned()
    }

    pub fn core_info(&self) -> String {
        format!(
            "kb_path={}, max_iterations={}",
            self.kb_path, self.max_iterations
        )
    }

    pub fn subscribe_events(&self, listener: Box<dyn EventListener>) -> CoreResult<Subscription> {
        let id = self.next_listener_id.fetch_add(1, Ordering::Relaxed);
        let listener: Arc<dyn EventListener> = Arc::from(listener);
        {
            let mut listeners = self
                .listeners
                .lock()
                .map_err(|_| CoreError::InvalidState("event listener lock poisoned".to_owned()))?;
            listeners.insert(id, listener);
        } // <-- drop lock BEFORE emitting

        emit_event_static(
            &self.listeners,
            "subscribed",
            format!("subscription_id={id}"),
        );
        Ok(Subscription { id })
    }

    pub fn unsubscribe_events(&self, subscription_id: u64) -> CoreResult<()> {
        let mut listeners = self
            .listeners
            .lock()
            .map_err(|_| CoreError::InvalidState("event listener lock poisoned".to_owned()))?;

        if listeners.remove(&subscription_id).is_none() {
            return Err(CoreError::NotFound(format!(
                "subscription {subscription_id}"
            )));
        }

        Ok(())
    }

    pub fn emit_test_event(&self, message: String) {
        emit_event_static(&self.listeners, "test", message);
    }

    pub fn create_session(&self, scenario: String, title: Option<String>) -> CoreResult<String> {
        let session = self.storage.create_session(&scenario, title.as_deref())?;
        emit_event_static(
            &self.listeners,
            "session_created",
            format!("session_id={},scenario={}", session.id, session.scenario),
        );
        Ok(session.id)
    }

    pub fn list_sessions(&self) -> CoreResult<Vec<Session>> {
        self.storage.list_sessions()
    }

    pub fn update_session_title(&self, session_id: String, title: String) -> CoreResult<()> {
        self.storage.update_session_title(&session_id, &title)
    }

    pub fn delete_session(&self, session_id: String) -> CoreResult<()> {
        self.storage.delete_session(&session_id)
    }

    pub fn create_message(
        &self,
        session_id: String,
        role: String,
        content: String,
        phase: Option<String>,
        tool_calls_json: Option<String>,
    ) -> CoreResult<Message> {
        let tool_calls: Option<Value> =
            match tool_calls_json {
                Some(raw) => Some(serde_json::from_str(&raw).map_err(|e| {
                    CoreError::InvalidState(format!("invalid tool_calls json: {e}"))
                })?),
                None => None,
            };

        let message = self.storage.create_message(
            &session_id,
            &role,
            &content,
            phase.as_deref(),
            tool_calls.as_ref(),
        )?;

        emit_event_static(
            &self.listeners,
            "message_created",
            format!(
                "session_id={},message_id={}",
                message.session_id, message.id
            ),
        );
        Ok(message)
    }

    pub fn get_messages(&self, session_id: String) -> CoreResult<Vec<Message>> {
        self.storage.get_messages(&session_id)
    }

    pub fn set_setting(&self, key: String, value: String) -> CoreResult<()> {
        self.storage.set_setting(&key, &value)
    }

    pub fn get_setting(&self, key: String) -> CoreResult<Option<String>> {
        self.storage.get_setting(&key)
    }

    pub fn set_tool_permission(&self, tool_name: String, permission: String) -> CoreResult<()> {
        self.storage.set_tool_permission(&tool_name, &permission)
    }

    pub fn get_tool_permission(&self, tool_name: String) -> CoreResult<String> {
        self.storage.get_tool_permission(&tool_name)
    }

    pub fn append_log(
        &self,
        level: String,
        message: String,
        session_id: Option<String>,
    ) -> CoreResult<i64> {
        self.storage
            .append_log(&level, &message, session_id.as_deref())
    }

    pub fn list_logs(&self, limit: u32) -> CoreResult<Vec<LogEntry>> {
        self.storage.list_logs(limit)
    }

    pub fn update_model_config(&self, config: ModelConfig) -> CoreResult<()> {
        let connector = ModelConnector::new(OpenRouterConfig {
            api_key: config.api_key,
            model_name: config.model_name,
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_owned()),
            retry: RetryConfig {
                max_retries: config.retry_max_retries,
                initial_delay_ms: config.retry_initial_delay_ms,
                max_delay_ms: config.retry_max_delay_ms,
                backoff_factor: config.retry_backoff_factor,
            },
        })?;

        let mut slot = self
            .model_connector
            .write()
            .map_err(|_| CoreError::InvalidState("model connector lock poisoned".to_owned()))?;
        *slot = Some(connector);

        emit_event_static(
            &self.listeners,
            "model_updated",
            "model config updated".to_owned(),
        );
        Ok(())
    }

    pub fn test_model_connection(&self) -> CoreResult<()> {
        let connector = {
            let slot = self
                .model_connector
                .read()
                .map_err(|_| CoreError::InvalidState("model connector lock poisoned".to_owned()))?;
            slot.clone()
        }
        .ok_or_else(|| CoreError::InvalidState("model not configured".to_owned()))?;

        RUNTIME.block_on(connector.test_connection())?;
        emit_event_static(
            &self.listeners,
            "model_connection_ok",
            "openrouter reachable".to_owned(),
        );
        Ok(())
    }

    pub fn ping_model(&self, prompt: String) -> CoreResult<String> {
        let connector = {
            let slot = self
                .model_connector
                .read()
                .map_err(|_| CoreError::InvalidState("model connector lock poisoned".to_owned()))?;
            slot.clone()
        }
        .ok_or_else(|| CoreError::InvalidState("model not configured".to_owned()))?;

        let messages = vec![model::ChatMessage {
            role: "user".to_owned(),
            content: prompt,
        }];

        let result = RUNTIME.block_on(connector.chat_completion(&messages))?;
        emit_event_static(
            &self.listeners,
            "model_ping",
            "chat completion finished".to_owned(),
        );
        Ok(result)
    }

    pub fn send_message(&self, session_id: String, content: String) -> CoreResult<String> {
        let session = self
            .storage
            .get_session(&session_id)?
            .ok_or_else(|| CoreError::NotFound(format!("session {session_id}")))?;

        self.storage
            .create_message(&session_id, "user", &content, Some("plan"), None)?;

        let task_id = Uuid::new_v4().to_string();
        let control = Arc::new(TaskControl::new());

        {
            let mut controls = self
                .task_controls
                .lock()
                .map_err(|_| CoreError::InvalidState("task_controls lock poisoned".to_owned()))?;
            controls.insert(task_id.clone(), control.clone());
        }

        // Obtain per-session lock Arc (create if absent)
        let session_lock = {
            let mut locks = self
                .session_locks
                .lock()
                .map_err(|_| CoreError::InvalidState("session_locks lock poisoned".to_owned()))?;
            locks
                .entry(session_id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let worker = AgentWorker {
            task_id: task_id.clone(),
            session_id,
            scenario: session.scenario,
            user_content: content,
            max_iterations: self.max_iterations,
            storage: self.storage.clone(),
            retrieval: self.retrieval.clone(),
            safety: self.safety.clone(),
            tools: self.tools.clone(),
            listeners: self.listeners.clone(),
            pending_tool_calls: self.pending_tool_calls.clone(),
            session_allow_all: self.session_allow_all.clone(),
            control: control.clone(),
            task_controls: self.task_controls.clone(),
        };

        thread::spawn(move || {
            // Acquire per-session lock so only one AgentWorker runs per session
            let _session_guard = session_lock.lock();

            let run_result = worker.run();
            if let Err(err) = run_result {
                if matches!(err, CoreError::Cancelled) {
                    emit_event_static(&worker.listeners, "cancelled", worker.task_id.clone());
                } else {
                    emit_event_static(
                        &worker.listeners,
                        "error",
                        json!({
                            "task_id": worker.task_id,
                            "message": err.to_string(),
                            "retryable": false
                        })
                        .to_string(),
                    );
                }
            }

            if let Ok(mut controls) = worker.task_controls.lock() {
                controls.remove(&worker.task_id);
            }
        });

        Ok(task_id)
    }

    pub fn cancel_agent_task(&self, task_id: String) -> CoreResult<()> {
        let controls = self
            .task_controls
            .lock()
            .map_err(|_| CoreError::InvalidState("task_controls lock poisoned".to_owned()))?;
        let control = controls
            .get(&task_id)
            .ok_or_else(|| CoreError::NotFound(format!("task {task_id}")))?;
        control.cancel();

        emit_event_static(
            &self.listeners,
            "cancelling",
            json!({"task_id": task_id}).to_string(),
        );
        Ok(())
    }

    pub fn respond_tool_call(&self, request_id: String, response: ToolResponse) -> CoreResult<()> {
        let pending = {
            let mut pending_map = self.pending_tool_calls.lock().map_err(|_| {
                CoreError::InvalidState("pending_tool_calls lock poisoned".to_owned())
            })?;
            pending_map
                .remove(&request_id)
                .ok_or_else(|| CoreError::NotFound(format!("request {request_id}")))?
        };

        if matches!(response, ToolResponse::AllowAllThisSession) {
            if let Ok(mut allow_all) = self.session_allow_all.lock() {
                allow_all.insert(pending.session_id.clone());
            }
        }

        if let ToolResponse::Allow { always: true } = response {
            let _ = self
                .storage
                .set_tool_permission(&pending.tool_name, "allow");
        }

        pending
            .sender
            .send(response)
            .map_err(|_| CoreError::InvalidState("tool request channel closed".to_owned()))?;

        emit_event_static(
            &self.listeners,
            "tool_call_response",
            json!({
                "request_id": request_id,
                "tool_name": pending.tool_name,
                "session_id": pending.session_id
            })
            .to_string(),
        );

        Ok(())
    }

    pub fn list_tools(&self) -> Vec<String> {
        self.tools.list_tools()
    }

    pub fn search_knowledge(
        &self,
        query: String,
        scenario: String,
        top_k: u32,
    ) -> CoreResult<Vec<SearchResult>> {
        self.retrieval.search(&query, &scenario, top_k as usize)
    }

    pub fn read_knowledge_file(&self, file_path: String) -> CoreResult<String> {
        self.retrieval.read_file(&file_path)
    }

    pub fn get_knowledge_info(&self) -> CoreResult<KnowledgeInfo> {
        self.retrieval.knowledge_info()
    }

    pub fn generate_report(&self, session_id: String) -> CoreResult<String> {
        let messages = self.storage.get_messages(&session_id)?;
        let report = messages
            .iter()
            .rev()
            .find(|msg| msg.role == "assistant" && msg.phase.as_deref() == Some("review"))
            .or_else(|| {
                messages.iter().rev().find(|msg| {
                    msg.role == "assistant"
                        && msg.content.contains("【事实摘要】")
                        && msg.content.contains("【免责声明】")
                })
            })
            .map(|msg| msg.content.clone())
            .ok_or_else(|| CoreError::NotFound(format!("report for session {session_id}")))?;
        Ok(report)
    }

    pub fn export_report_markdown(&self, session_id: String, path: String) -> CoreResult<()> {
        let report = self.generate_report(session_id.clone())?;
        std::fs::write(&path, report)
            .map_err(|e| CoreError::Storage(format!("write markdown failed: {e}")))?;

        let _ = self.storage.append_log(
            "info",
            &format!("report exported: {path}"),
            Some(session_id.as_str()),
        );
        Ok(())
    }

    pub fn regenerate_report(&self, session_id: String) -> CoreResult<String> {
        emit_event_static(
            &self.listeners,
            "report_regenerating",
            json!({ "session_id": session_id }).to_string(),
        );

        self.send_message(
            session_id,
            "请基于已收集的事实重新生成一版完整法律咨询报告。".to_owned(),
        )
    }
}

struct AgentWorker {
    task_id: String,
    session_id: String,
    scenario: String,
    user_content: String,
    max_iterations: u32,
    storage: Arc<SqliteStorage>,
    retrieval: Arc<RetrievalEngine>,
    safety: Arc<SafetyEngine>,
    tools: Arc<ToolRegistry>,
    listeners: Arc<Mutex<HashMap<u64, Arc<dyn EventListener>>>>,
    pending_tool_calls: Arc<Mutex<HashMap<String, PendingToolCall>>>,
    session_allow_all: Arc<Mutex<HashSet<String>>>,
    control: Arc<TaskControl>,
    task_controls: Arc<Mutex<HashMap<String, Arc<TaskControl>>>>,
}

impl AgentWorker {
    fn run(&self) -> CoreResult<()> {
        self.run_with_iteration(1)
    }

    fn run_with_iteration(&self, iteration: u32) -> CoreResult<()> {
        if iteration > self.max_iterations {
            return Err(CoreError::Unknown(format!(
                "max_iterations exceeded: {}",
                self.max_iterations
            )));
        }

        self.guard_not_cancelled()?;

        emit_event_static(
            &self.listeners,
            "agent_phase",
            json!({"task_id": self.task_id, "phase": AgentPhase::Plan.as_str()}).to_string(),
        );

        let intake = intake_state(&self.storage, &self.session_id, &self.scenario)?;
        if !intake.done {
            return self.handle_intake(iteration, intake);
        }

        emit_event_static(
            &self.listeners,
            "agent_phase",
            json!({"task_id": self.task_id, "phase": AgentPhase::Draft.as_str()}).to_string(),
        );

        let tool_ctx = ToolContext {
            retrieval: self.retrieval.clone(),
            safety: self.safety.clone(),
        };

        let facts = collect_facts(&self.storage, &self.session_id, &self.scenario)?;
        let facts_map: serde_json::Map<String, Value> = facts
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        let summary_value = self.execute_tool_with_permission(
            "summarize_facts",
            json!({"facts": facts_map}),
            &tool_ctx,
        )?;
        let facts_summary = summary_value
            .get("summary")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format_facts_summary(&facts));

        let query_text = if self.user_content.trim().is_empty() {
            "劳动仲裁".to_owned()
        } else {
            format!("劳动仲裁 {}", self.user_content)
        };

        let search_value = self.execute_tool_with_permission(
            "kb_search",
            json!({"query": query_text, "scenario": self.scenario, "top_k": 3}),
            &tool_ctx,
        )?;

        let search_results: Vec<SearchResult> = serde_json::from_value(search_value)
            .map_err(|e| CoreError::Unknown(format!("parse search result failed: {e}")))?;

        let legal_analysis = if search_results.is_empty() {
            "当前未检索到足够的法规条文。建议补充案情细节（时间、金额、证据）后再生成一次分析。".to_owned()
        } else {
            let references = search_results
                .iter()
                .take(3)
                .enumerate()
                .map(|(idx, item)| {
                    format!(
                        "{}. 《{}》提到：{}",
                        idx + 1,
                        item.title.trim(),
                        item.snippet.replace('\n', " ")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            format!(
                "结合知识库中的条文信息，现阶段可以先这样理解：\n{}\n\n以上为通用分析，最终判断仍要结合当地裁审口径和证据完整度。",
                references
            )
        };

        let citation_sources = search_results
            .iter()
            .take(3)
            .map(|item| {
                json!({
                    "file_path": item.file_path,
                    "line_start": item.line_start,
                    "line_end": item.line_end
                })
            })
            .collect::<Vec<_>>();
        let citation_value = self.execute_tool_with_permission(
            "cite",
            json!({"sources": citation_sources}),
            &tool_ctx,
        )?;
        let citations = citation_value
            .get("citations")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let process_path = "1. 先把证据按时间线整理：合同/考勤/工资流水/沟通记录尽量对应到具体日期。\n2. 准备并提交仲裁申请：写清诉求、金额和事实经过，向有管辖权的仲裁委递交。\n3. 参加调解或开庭：围绕劳动关系、欠薪事实、金额计算这三点陈述，并按要求补充材料。";
        let risk_value = self.execute_tool_with_permission(
            "suggest_escalation",
            json!({"content": self.user_content}),
            &tool_ctx,
        )?;
        let risk_message = risk_value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("本回答基于你当前提供的信息，存在不确定性；若金额较大或争议复杂，建议尽快咨询执业律师。");

        let draft_report = build_report(
            &facts_summary,
            &format!("{}\n\n【引用】\n{}", legal_analysis, citations),
            process_path,
            risk_message,
        );

        emit_event_static(
            &self.listeners,
            "agent_phase",
            json!({"task_id": self.task_id, "phase": AgentPhase::Review.as_str()}).to_string(),
        );

        let safety_value = self.execute_tool_with_permission(
            "check_safety",
            json!({"content": draft_report}),
            &tool_ctx,
        )?;
        let fallback_modified_content = safety_value
            .get("modified_content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let safety_result = serde_json::from_value::<SafetyCheckResult>(safety_value).unwrap_or(
            SafetyCheckResult {
                modified_content: fallback_modified_content,
                issues: Vec::new(),
                has_critical: false,
            },
        );

        if !safety_result.issues.is_empty() {
            let critical_count = safety_result
                .issues
                .iter()
                .filter(|issue| issue.severity == Severity::Critical)
                .count();
            let event_name = if safety_result.has_critical {
                "review_intercepted"
            } else {
                "review_adjusted"
            };

            emit_event_static(
                &self.listeners,
                event_name,
                json!({
                    "task_id": self.task_id,
                    "session_id": self.session_id,
                    "issue_count": safety_result.issues.len(),
                    "critical_count": critical_count
                })
                .to_string(),
            );
        }

        let mut final_report = safety_result.modified_content;
        if safety_result.has_critical {
            let critical_count = safety_result
                .issues
                .iter()
                .filter(|issue| issue.severity == Severity::Critical)
                .count();
            final_report = format!(
                "【安全审查】\n检测到 {} 处高风险表述，已自动拦截并改写。\n\n{}",
                critical_count, final_report
            );
        }

        self.guard_not_cancelled()?;
        self.storage.create_message(
            &self.session_id,
            "assistant",
            &final_report,
            Some("review"),
            None,
        )?;

        emit_event_static(
            &self.listeners,
            "completed",
            json!({
                "task_id": self.task_id,
                "session_id": self.session_id,
                "report": final_report
            })
            .to_string(),
        );

        Ok(())
    }

    fn handle_intake(&self, iteration: u32, state: agent::IntakeState) -> CoreResult<()> {
        let tool_ctx = ToolContext {
            retrieval: self.retrieval.clone(),
            safety: self.safety.clone(),
        };

        if state.current_index == 0 {
            let first = self.execute_tool_with_permission(
                "ask_user",
                json!({"scenario": self.scenario, "index": 0}),
                &tool_ctx,
            )?;
            start_intake(&self.storage, &self.session_id)?;

            let question = first
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or("请描述您的情况");
            let total = first.get("total").and_then(Value::as_u64).unwrap_or(1);
            let text = format!(
                "我先帮你把案情梳理清楚，接下来会问你 {} 个小问题。\n你按知道的回答就可以，不确定也可以说“暂不清楚”。\n\n进度：1/{}\n\n第 1 题：{}",
                total, total, question
            );

            self.storage.create_message(
                &self.session_id,
                "assistant",
                &text,
                Some("draft"),
                None,
            )?;

            emit_event_static(
                &self.listeners,
                "intake_progress",
                json!({
                    "task_id": self.task_id,
                    "current": 1,
                    "total": total,
                    "question": question
                })
                .to_string(),
            );
            emit_event_static(
                &self.listeners,
                "completed",
                json!({
                    "task_id": self.task_id,
                    "session_id": self.session_id,
                    "message": text
                })
                .to_string(),
            );
            return Ok(());
        }

        let answered_index = state.current_index.saturating_sub(1);
        save_answer(
            &self.storage,
            &self.session_id,
            answered_index,
            &self.user_content,
        )?;

        if state.current_index < state.questions.len() {
            let next_value = self.execute_tool_with_permission(
                "ask_user",
                json!({"scenario": self.scenario, "index": state.current_index}),
                &tool_ctx,
            )?;
            let question = next_value
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or("请继续补充信息");
            let current = next_value
                .get("current")
                .and_then(Value::as_u64)
                .unwrap_or((state.current_index + 1) as u64);
            let total = next_value
                .get("total")
                .and_then(Value::as_u64)
                .unwrap_or(state.questions.len() as u64);

            advance_intake_index(&self.storage, &self.session_id, state.current_index + 1)?;

            let ack = self.intake_acknowledgement(answered_index, &self.user_content);
            let text = format!(
                "{}\n\n进度：{}/{}\n\n下一题：{}",
                ack, current, total, question
            );
            self.storage.create_message(
                &self.session_id,
                "assistant",
                &text,
                Some("draft"),
                None,
            )?;

            emit_event_static(
                &self.listeners,
                "intake_progress",
                json!({
                    "task_id": self.task_id,
                    "current": current,
                    "total": total,
                    "question": question
                })
                .to_string(),
            );
            emit_event_static(
                &self.listeners,
                "completed",
                json!({
                    "task_id": self.task_id,
                    "session_id": self.session_id,
                    "message": text
                })
                .to_string(),
            );
            return Ok(());
        }

        mark_intake_done(&self.storage, &self.session_id)?;
        emit_event_static(
            &self.listeners,
            "intake_done",
            json!({"task_id": self.task_id, "session_id": self.session_id}).to_string(),
        );
        self.run_with_iteration(iteration + 1)
    }

    fn execute_tool_with_permission(
        &self,
        tool_name: &str,
        args: Value,
        ctx: &ToolContext,
    ) -> CoreResult<Value> {
        self.guard_not_cancelled()?;

        let mut permission = self.storage.get_tool_permission(tool_name)?;
        let allow_all = self
            .session_allow_all
            .lock()
            .map_err(|_| CoreError::InvalidState("session_allow_all lock poisoned".to_owned()))?
            .contains(&self.session_id);
        if allow_all && permission == "ask" {
            permission = "allow".to_owned();
        }

        if permission == "deny" {
            return Err(CoreError::Tool(format!("tool {tool_name} is denied")));
        }

        if permission == "ask" {
            let request_id = Uuid::new_v4().to_string();
            let (tx, rx) = mpsc::channel::<ToolResponse>();

            {
                let mut pending_map = self.pending_tool_calls.lock().map_err(|_| {
                    CoreError::InvalidState("pending_tool_calls lock poisoned".to_owned())
                })?;
                pending_map.insert(
                    request_id.clone(),
                    PendingToolCall {
                        sender: tx,
                        session_id: self.session_id.clone(),
                        tool_name: tool_name.to_owned(),
                    },
                );
            }

            emit_event_static(
                &self.listeners,
                "tool_call_request",
                json!({
                    "task_id": self.task_id,
                    "request_id": request_id,
                    "tool_name": tool_name,
                    "arguments": args
                })
                .to_string(),
            );

            let decision = loop {
                if let Err(err) = self.guard_not_cancelled() {
                    let _ = self.remove_pending_tool_call(&request_id);
                    return Err(err);
                }
                match rx.recv_timeout(Duration::from_millis(300)) {
                    Ok(resp) => break resp,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        let _ = self.remove_pending_tool_call(&request_id);
                        return Err(CoreError::InvalidState(
                            "approval channel disconnected".to_owned(),
                        ));
                    }
                }
            };

            match decision {
                ToolResponse::Allow { always } => {
                    if always {
                        self.storage.set_tool_permission(tool_name, "allow")?;
                    }
                }
                ToolResponse::AllowAllThisSession => {
                    if let Ok(mut allow_all_set) = self.session_allow_all.lock() {
                        allow_all_set.insert(self.session_id.clone());
                    }
                }
                ToolResponse::Deny => {
                    return Err(CoreError::Tool(format!("tool {tool_name} denied by user")));
                }
            }
        }

        let result = self.tools.run(tool_name, args.clone(), ctx)?;
        emit_event_static(
            &self.listeners,
            "tool_call_result",
            json!({
                "task_id": self.task_id,
                "tool_name": tool_name,
                "result": result
            })
            .to_string(),
        );

        Ok(result)
    }

    fn remove_pending_tool_call(&self, request_id: &str) -> CoreResult<()> {
        let mut pending_map = self
            .pending_tool_calls
            .lock()
            .map_err(|_| CoreError::InvalidState("pending_tool_calls lock poisoned".to_owned()))?;
        pending_map.remove(request_id);
        Ok(())
    }

    fn guard_not_cancelled(&self) -> CoreResult<()> {
        if self.control.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        Ok(())
    }

    fn intake_acknowledgement(&self, answered_index: usize, answer: &str) -> &'static str {
        if answer.contains("（用户跳过此题）") || answer.contains("跳过") {
            return "好的，这题先记为待补充，不影响我们继续往下走。";
        }

        const ACKS: [&str; 4] = [
            "收到，这条信息很有帮助。",
            "明白了，我已经记下这一点。",
            "好的，信息很关键，继续下一题。",
            "了解，感谢补充，我们再确认下一项。",
        ];
        ACKS[answered_index % ACKS.len()]
    }
}

fn emit_event_static(
    listeners: &Arc<Mutex<HashMap<u64, Arc<dyn EventListener>>>>,
    kind: &str,
    payload: String,
) {
    let event = CoreEvent {
        kind: kind.to_owned(),
        payload,
        timestamp: Utc::now().timestamp(),
    };

    let listeners_snapshot = match listeners.lock() {
        Ok(lock) => lock.values().cloned().collect::<Vec<_>>(),
        Err(_) => return,
    };

    for listener in listeners_snapshot {
        listener.on_event(event.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use tempfile::TempDir;

    use super::{Core, CoreConfig, CoreEvent, EventListener};

    #[derive(Clone, Default)]
    struct EventCollector {
        events: Arc<Mutex<Vec<CoreEvent>>>,
    }

    impl EventCollector {
        fn push(&self, event: CoreEvent) {
            if let Ok(mut events) = self.events.lock() {
                events.push(event);
            }
        }

        fn snapshot(&self) -> Vec<CoreEvent> {
            self.events
                .lock()
                .map(|events| events.clone())
                .unwrap_or_default()
        }

        fn wait_for<F>(&self, timeout: Duration, predicate: F) -> bool
        where
            F: Fn(&[CoreEvent]) -> bool,
        {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                let snapshot = self.snapshot();
                if predicate(&snapshot) {
                    return true;
                }
                thread::sleep(Duration::from_millis(30));
            }
            false
        }
    }

    struct TestListener {
        collector: EventCollector,
    }

    impl EventListener for TestListener {
        fn on_event(&self, event: CoreEvent) {
            self.collector.push(event);
        }
    }

    fn setup_core(max_iterations: u32) -> (TempDir, Arc<Core>, EventCollector, String) {
        setup_core_with_doc(
            max_iterations,
            "# 劳动仲裁\n拖欠工资可申请劳动仲裁，准备劳动合同、工资流水和沟通记录。",
        )
    }

    fn setup_core_with_doc(
        max_iterations: u32,
        labor_doc_content: &str,
    ) -> (TempDir, Arc<Core>, EventCollector, String) {
        let temp_dir = TempDir::new().expect("temp dir");
        let kb_root = temp_dir.path().join("kb");
        let labor = kb_root.join("labor");
        fs::create_dir_all(&labor).expect("create labor dir");
        fs::write(labor.join("law.md"), labor_doc_content).expect("write kb file");

        let db_path = temp_dir.path().join("core.db");
        let core = Core::new(CoreConfig {
            kb_path: kb_root.to_string_lossy().to_string(),
            db_path: db_path.to_string_lossy().to_string(),
            max_iterations,
        })
        .expect("init core");

        let collector = EventCollector::default();
        core.subscribe_events(Box::new(TestListener {
            collector: collector.clone(),
        }))
        .expect("subscribe");

        let session_id = core
            .create_session("labor".to_owned(), Some("测试".to_owned()))
            .expect("create session");

        (temp_dir, core, collector, session_id)
    }

    /// Set all built-in tools to "allow" so Agent never blocks on permission
    fn allow_all_tools(core: &Core) {
        for tool_name in [
            "ask_user",
            "kb_search",
            "kb_read",
            "cite",
            "summarize_facts",
            "check_safety",
            "suggest_escalation",
        ] {
            core.set_tool_permission(tool_name.to_owned(), "allow".to_owned())
                .expect("allow tool");
        }
    }

    #[test]
    fn agent_phase_transitions_plan_draft_review() {
        let (_temp_dir, core, collector, session_id) = setup_core(12);
        allow_all_tools(&core);

        // First message starts intake (question 1/6)
        core.send_message(session_id.clone(), "我想咨询劳动仲裁".to_owned())
            .expect("start intake");

        // Wait for first intake question to complete before sending answers
        // (per-session lock ensures serialization)
        for idx in 0..6 {
            // Small pause to let the per-session lock serialize
            thread::sleep(Duration::from_millis(200));
            core.send_message(session_id.clone(), format!("补充信息{}", idx + 1))
                .expect("send answer");
        }

        let has_report = collector.wait_for(Duration::from_secs(30), |events| {
            events
                .iter()
                .any(|event| event.kind == "completed" && event.payload.contains("\"report\""))
        });
        assert!(has_report, "final report completion event not observed");

        let phases = collector
            .snapshot()
            .into_iter()
            .filter(|event| event.kind == "agent_phase")
            .filter_map(|event| {
                serde_json::from_str::<serde_json::Value>(&event.payload)
                    .ok()?
                    .get("phase")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>();

        assert!(phases.iter().any(|phase| phase == "planning"));
        assert!(phases.iter().any(|phase| phase == "drafting"));
        assert!(phases.iter().any(|phase| phase == "reviewing"));
    }

    #[test]
    fn max_iterations_triggers_error_event() {
        let (_temp_dir, core, collector, session_id) = setup_core(1);
        allow_all_tools(&core);

        // Mark intake as nearly done: set index to last question
        core.set_setting(format!("intake:{session_id}:idx"), "6".to_owned())
            .expect("set intake idx");

        core.send_message(session_id, "最后一题答案".to_owned())
            .expect("send");

        let hit_limit = collector.wait_for(Duration::from_secs(10), |events| {
            events
                .iter()
                .any(|event| event.kind == "error" && event.payload.contains("max_iterations"))
        });
        assert!(hit_limit, "max_iterations error event not observed");
    }

    #[test]
    fn cancel_agent_task_emits_cancelled_event() {
        let (_temp_dir, core, collector, session_id) = setup_core(6);
        // Leave ask_user at default "ask" so the agent blocks on tool_call_request
        // and we can cancel it

        let task_id = core
            .send_message(session_id, "我想咨询劳动仲裁".to_owned())
            .expect("send");

        let has_request = collector.wait_for(Duration::from_secs(10), |events| {
            events.iter().any(|event| event.kind == "tool_call_request")
        });
        assert!(has_request, "tool call request not emitted");

        core.cancel_agent_task(task_id).expect("cancel");
        let cancelled = collector.wait_for(Duration::from_secs(10), |events| {
            events.iter().any(|event| event.kind == "cancelled")
        });
        assert!(cancelled, "cancelled event not observed");
    }

    #[test]
    fn denied_tool_emits_error_event() {
        let (_temp_dir, core, collector, session_id) = setup_core(6);
        // Allow all tools first, then deny kb_search specifically
        allow_all_tools(&core);
        core.set_tool_permission("summarize_facts".to_owned(), "allow".to_owned())
            .expect("allow summarize_facts");
        core.set_tool_permission("kb_search".to_owned(), "deny".to_owned())
            .expect("deny kb_search");

        // Skip intake entirely
        core.set_setting(format!("intake:{session_id}:done"), "1".to_owned())
            .expect("mark intake done");

        core.send_message(session_id, "直接生成报告".to_owned())
            .expect("send");

        let denied_error = collector.wait_for(Duration::from_secs(10), |events| {
            events
                .iter()
                .any(|event| event.kind == "error" && event.payload.contains("denied"))
        });
        assert!(denied_error, "denied tool error event not observed");
    }

    #[test]
    fn report_contains_required_sections_and_citations() {
        let (_temp_dir, core, collector, session_id) = setup_core(8);
        allow_all_tools(&core);
        core.set_setting(format!("intake:{session_id}:done"), "1".to_owned())
            .expect("mark intake done");

        core.send_message(session_id, "请生成劳动仲裁报告".to_owned())
            .expect("send");

        let mut report_text = String::new();
        let has_report = collector.wait_for(Duration::from_secs(20), |events| {
            for event in events.iter().rev() {
                if event.kind == "completed" && event.payload.contains("\"report\"") {
                    return true;
                }
            }
            false
        });
        assert!(has_report, "report completion event not observed");

        for event in collector.snapshot().iter().rev() {
            if event.kind == "completed" {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                    if let Some(text) = data.get("report").and_then(serde_json::Value::as_str) {
                        report_text = text.to_owned();
                        break;
                    }
                }
            }
        }

        assert!(report_text.contains("【事实摘要】"));
        assert!(report_text.contains("【法律分析】"));
        assert!(report_text.contains("【办事路径】"));
        assert!(report_text.contains("【风险提示】"));
        assert!(report_text.contains("【免责声明】"));
        assert!(report_text.contains("【引用】"));
    }

    #[test]
    fn review_intercepts_critical_safety_phrases() {
        let (_temp_dir, core, collector, session_id) =
            setup_core_with_doc(8, "# 劳动仲裁\n这个方案包赢，保证胜诉。");
        allow_all_tools(&core);
        core.set_setting(format!("intake:{session_id}:done"), "1".to_owned())
            .expect("mark intake done");

        core.send_message(session_id, "请给出分析".to_owned())
            .expect("send");

        let intercepted = collector.wait_for(Duration::from_secs(20), |events| {
            events
                .iter()
                .any(|event| event.kind == "review_intercepted")
        });
        assert!(intercepted, "review_intercepted event not observed");

        let mut report_text = String::new();
        for event in collector.snapshot().iter().rev() {
            if event.kind == "completed" {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                    if let Some(text) = data.get("report").and_then(serde_json::Value::as_str) {
                        report_text = text.to_owned();
                        break;
                    }
                }
            }
        }

        assert!(report_text.contains("【安全审查】"));
        assert!(!report_text.contains("包赢"));
    }
}

uniffi::setup_scaffolding!();
