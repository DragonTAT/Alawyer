use crate::error::CoreResult;
use crate::storage::SqliteStorage;
use crate::tools::{intake_questions_for_scenario, IntakeQuestion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPhase {
    Plan,
    Draft,
    Review,
}

impl AgentPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "planning",
            Self::Draft => "drafting",
            Self::Review => "reviewing",
        }
    }
}

pub const DISCLAIMER: &str = r#"【免责声明】
1. 本报告由AI生成，仅供参考，不构成法律意见或律师建议
2. 案件具体情况可能影响法律适用，建议咨询执业律师
3. 法规可能存在时效性，请以最新颁布版本为准
4. 本报告不保证准确性、完整性或适用性"#;

pub fn intake_state(
    storage: &SqliteStorage,
    session_id: &str,
    scenario: &str,
) -> CoreResult<IntakeState> {
    let questions = intake_questions_for_scenario(scenario);

    let idx_key = format!("intake:{session_id}:idx");
    let done_key = format!("intake:{session_id}:done");

    let index = storage
        .get_setting(&idx_key)?
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(0);
    let done = storage
        .get_setting(&done_key)?
        .map(|value| value == "1")
        .unwrap_or(false);

    Ok(IntakeState {
        questions,
        current_index: index,
        done,
    })
}

pub fn start_intake(storage: &SqliteStorage, session_id: &str) -> CoreResult<()> {
    storage.set_setting(&format!("intake:{session_id}:idx"), "1")
}

pub fn mark_intake_done(storage: &SqliteStorage, session_id: &str) -> CoreResult<()> {
    storage.set_setting(&format!("intake:{session_id}:done"), "1")
}

pub fn save_answer(
    storage: &SqliteStorage,
    session_id: &str,
    question_index: usize,
    answer: &str,
) -> CoreResult<()> {
    storage.set_setting(
        &format!("intake:{session_id}:answer:{question_index}"),
        answer,
    )
}

pub fn advance_intake_index(
    storage: &SqliteStorage,
    session_id: &str,
    next: usize,
) -> CoreResult<()> {
    storage.set_setting(&format!("intake:{session_id}:idx"), &next.to_string())
}

/// Collect answered facts in question-order (stable output).
pub fn collect_facts(
    storage: &SqliteStorage,
    session_id: &str,
    scenario: &str,
) -> CoreResult<Vec<(String, String)>> {
    let questions = intake_questions_for_scenario(scenario);
    let mut facts = Vec::with_capacity(questions.len());

    for (idx, question) in questions.iter().enumerate() {
        let key = format!("intake:{session_id}:answer:{idx}");
        let answer = storage
            .get_setting(&key)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                if question.required {
                    "未提供".to_owned()
                } else {
                    "可补充".to_owned()
                }
            });
        facts.push((question.question.clone(), answer));
    }

    Ok(facts)
}

pub fn format_facts_summary(facts: &[(String, String)]) -> String {
    facts
        .iter()
        .map(|(question, answer)| format!("- {}：{}", question, answer))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_report(
    facts_summary: &str,
    legal_analysis: &str,
    process_path: &str,
    risk_notice: &str,
) -> String {
    format!(
        "【先说结论】\n从您目前提供的信息看，这类争议通常可以先走劳动仲裁路径；建议尽快把证据按时间线整理好，再按步骤推进。\n\n【事实摘要】\n我先把您提供的信息整理如下：\n{}\n\n【法律分析】\n{}\n\n【办事路径】\n建议按“先准备、再提交、再跟进”的顺序推进：\n{}\n\n【风险提示】\n{}\n\n{}",
        facts_summary, legal_analysis, process_path, risk_notice, DISCLAIMER
    )
}

#[derive(Debug, Clone)]
pub struct IntakeState {
    pub questions: Vec<IntakeQuestion>,
    pub current_index: usize,
    pub done: bool,
}
