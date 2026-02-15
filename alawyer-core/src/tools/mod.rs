use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::error::{CoreError, CoreResult};
use crate::retrieval::RetrievalEngine;
use crate::safety::SafetyEngine;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct IntakeQuestion {
    pub id: u32,
    pub question: String,
    pub required: bool,
}

pub fn intake_questions_for_scenario(scenario: &str) -> Vec<IntakeQuestion> {
    match scenario {
        "labor" => vec![
            IntakeQuestion {
                id: 1,
                question: "先确认一下，您主要工作地在什么地区（省/市）？不同地区处理口径会有差异。"
                    .to_owned(),
                required: true,
            },
            IntakeQuestion {
                id: 2,
                question: "您大概什么时候入职的？有没有签劳动合同（电子版也算）？".to_owned(),
                required: true,
            },
            IntakeQuestion {
                id: 3,
                question: "您主要做什么工作？月工资大约多少（税前税后都可以）？".to_owned(),
                required: true,
            },
            IntakeQuestion {
                id: 4,
                question: "被拖欠工资大概持续多久、总额大约多少？不确定可以先给估算。".to_owned(),
                required: false,
            },
            IntakeQuestion {
                id: 5,
                question: "您最希望达成的结果是什么？比如补发工资、经济补偿、出具离职证明等。"
                    .to_owned(),
                required: true,
            },
            IntakeQuestion {
                id: 6,
                question: "目前手里有哪些材料？例如合同、考勤、工资流水、聊天记录、录音等。"
                    .to_owned(),
                required: false,
            },
        ],
        _ => vec![],
    }
}

#[derive(Clone)]
pub struct ToolContext {
    pub retrieval: Arc<RetrievalEngine>,
    pub safety: Arc<SafetyEngine>,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, args: Value, ctx: &ToolContext) -> CoreResult<Value>;
}

#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn with_builtins() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };

        registry.register(KbSearchTool);
        registry.register(KbReadTool);
        registry.register(AskUserTool);
        registry.register(CiteTool);
        registry.register(SummarizeFactsTool);
        registry.register(CheckSafetyTool);
        registry.register(SuggestEscalationTool);
        registry
    }

    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name().to_owned(), Arc::new(tool));
    }

    pub fn run(&self, tool_name: &str, args: Value, ctx: &ToolContext) -> CoreResult<Value> {
        let tool = self
            .tools
            .get(tool_name)
            .ok_or_else(|| CoreError::NotFound(format!("tool {tool_name}")))?;
        tool.run(args, ctx)
    }

    pub fn list_tools(&self) -> Vec<String> {
        let mut names = self.tools.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }
}

struct KbSearchTool;
impl Tool for KbSearchTool {
    fn name(&self) -> &'static str {
        "kb_search"
    }

    fn run(&self, args: Value, ctx: &ToolContext) -> CoreResult<Value> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::Tool("kb_search missing query".to_owned()))?;
        let scenario = args
            .get("scenario")
            .and_then(Value::as_str)
            .unwrap_or("labor");
        let top_k = args.get("top_k").and_then(Value::as_u64).unwrap_or(5) as usize;

        let results = ctx.retrieval.search(query, scenario, top_k)?;
        serde_json::to_value(results)
            .map_err(|e| CoreError::Unknown(format!("serialize kb_search result failed: {e}")))
    }
}

struct KbReadTool;
impl Tool for KbReadTool {
    fn name(&self) -> &'static str {
        "kb_read"
    }

    fn run(&self, args: Value, ctx: &ToolContext) -> CoreResult<Value> {
        let file_path = args
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::Tool("kb_read missing file_path".to_owned()))?;

        let content = ctx.retrieval.read_file(file_path)?;
        Ok(json!({ "file_path": file_path, "content": content }))
    }
}

struct AskUserTool;
impl Tool for AskUserTool {
    fn name(&self) -> &'static str {
        "ask_user"
    }

    fn run(&self, args: Value, _ctx: &ToolContext) -> CoreResult<Value> {
        let scenario = args
            .get("scenario")
            .and_then(Value::as_str)
            .unwrap_or("labor");
        let index = args.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let questions = intake_questions_for_scenario(scenario);

        if let Some(question) = questions.get(index) {
            Ok(json!({
                "done": false,
                "id": question.id,
                "question": question.question,
                "required": question.required,
                "current": index + 1,
                "total": questions.len()
            }))
        } else {
            Ok(json!({ "done": true, "total": questions.len() }))
        }
    }
}

struct CiteTool;
impl Tool for CiteTool {
    fn name(&self) -> &'static str {
        "cite"
    }

    fn run(&self, args: Value, _ctx: &ToolContext) -> CoreResult<Value> {
        let mut lines = Vec::new();
        if let Some(sources) = args.get("sources").and_then(Value::as_array) {
            for source in sources {
                let file_path = source
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or("未知文件");
                let line_start = source
                    .get("line_start")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let line_end = source
                    .get("line_end")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                lines.push(format!("- {}:{}-{}", file_path, line_start, line_end));
            }
        }

        Ok(json!({ "citations": lines.join("\n") }))
    }
}

struct SummarizeFactsTool;
impl Tool for SummarizeFactsTool {
    fn name(&self) -> &'static str {
        "summarize_facts"
    }

    fn run(&self, args: Value, _ctx: &ToolContext) -> CoreResult<Value> {
        let facts = args
            .get("facts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let mut lines = Vec::new();
        for (key, value) in facts {
            if let Some(text) = value.as_str() {
                lines.push(format!("- {}：{}", key, text));
            }
        }

        Ok(json!({ "summary": lines.join("\n") }))
    }
}

struct CheckSafetyTool;
impl Tool for CheckSafetyTool {
    fn name(&self) -> &'static str {
        "check_safety"
    }

    fn run(&self, args: Value, ctx: &ToolContext) -> CoreResult<Value> {
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::Tool("check_safety missing content".to_owned()))?;

        let result = ctx.safety.check(content);
        serde_json::to_value(result)
            .map_err(|e| CoreError::Unknown(format!("serialize safety result failed: {e}")))
    }
}

struct SuggestEscalationTool;
impl Tool for SuggestEscalationTool {
    fn name(&self) -> &'static str {
        "suggest_escalation"
    }

    fn run(&self, args: Value, _ctx: &ToolContext) -> CoreResult<Value> {
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let high_risk_keywords = ["刑事", "移民", "证券", "重大财产", "坐牢", "犯罪"];
        let need_escalation = high_risk_keywords
            .iter()
            .any(|keyword| content.contains(keyword));

        let message = if need_escalation {
            "这个场景风险较高，建议尽快和执业律师一对一确认关键细节。"
        } else {
            "以上建议仅供参考；如果争议金额较大或事实复杂，建议再请执业律师把关。"
        };

        Ok(json!({
            "need_escalation": need_escalation,
            "message": message
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use serde_json::{json, Value};
    use tempfile::TempDir;

    use super::{ToolContext, ToolRegistry};
    use crate::retrieval::RetrievalEngine;
    use crate::safety::SafetyEngine;

    fn make_context() -> (TempDir, ToolContext) {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().to_path_buf();

        let labor_dir = root.join("labor");
        fs::create_dir_all(&labor_dir).expect("create dir");
        fs::write(
            labor_dir.join("law.md"),
            "# 劳动仲裁\n拖欠工资可申请仲裁，准备劳动合同与工资流水。",
        )
        .expect("write file");

        let ctx = ToolContext {
            retrieval: Arc::new(RetrievalEngine::new(root)),
            safety: Arc::new(SafetyEngine::default()),
        };
        (dir, ctx)
    }

    #[test]
    fn registry_runs_kb_search() {
        let (_dir, ctx) = make_context();
        let registry = ToolRegistry::with_builtins();

        let value = registry
            .run(
                "kb_search",
                json!({"query": "拖欠工资", "scenario": "labor", "top_k": 3}),
                &ctx,
            )
            .expect("kb search");

        assert!(value.as_array().is_some());
    }

    #[test]
    fn check_safety_tool_rewrites_content() {
        let (_dir, ctx) = make_context();
        let registry = ToolRegistry::with_builtins();

        let value = registry
            .run("check_safety", json!({"content": "这个案件包赢"}), &ctx)
            .expect("check safety");

        let modified = value
            .get("modified_content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(modified.contains("结果不确定"));
    }
}
