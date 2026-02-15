use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    Critical,
    Warning,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SafetyIssue {
    pub rule_name: String,
    pub matched_text: String,
    pub replacement: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SafetyCheckResult {
    pub modified_content: String,
    pub issues: Vec<SafetyIssue>,
    pub has_critical: bool,
}

#[derive(Debug, Clone)]
struct SafetyRule {
    name: &'static str,
    regex: Regex,
    replacement: &'static str,
    severity: Severity,
}

#[derive(Clone)]
pub struct SafetyEngine {
    rules: Vec<SafetyRule>,
}

impl Default for SafetyEngine {
    fn default() -> Self {
        Self {
            rules: vec![
                SafetyRule {
                    name: "guarantee_win",
                    regex: Regex::new(r"(?i)(保证.*胜诉|肯定.*赢)").expect("valid regex"),
                    replacement: "无法保证案件结果",
                    severity: Severity::Critical,
                },
                SafetyRule {
                    name: "fake_lawyer_identity",
                    regex: Regex::new(r"(?i)(我是律师|本律师|根据律师意见)").expect("valid regex"),
                    replacement: "本回答由AI生成",
                    severity: Severity::Critical,
                },
                SafetyRule {
                    name: "absolute_certainty",
                    regex: Regex::new(r"(?i)(绝对没问题|肯定没事|一定行)").expect("valid regex"),
                    replacement: "存在不确定性",
                    severity: Severity::Warning,
                },
                SafetyRule {
                    name: "must_win",
                    regex: Regex::new(r"(?i)(包赢|必赢|必胜|一定.*赢)").expect("valid regex"),
                    replacement: "结果不确定",
                    severity: Severity::Critical,
                },
                SafetyRule {
                    name: "crime_judgement",
                    regex: Regex::new(r"(?i)(你构成.*罪|你.*坐牢|你.*犯罪)").expect("valid regex"),
                    replacement: "建议咨询专业律师",
                    severity: Severity::Critical,
                },
                SafetyRule {
                    name: "legal_effect",
                    regex: Regex::new(r"(?i)(具有法律效力|法律上有效)").expect("valid regex"),
                    replacement: "需执业律师确认效力",
                    severity: Severity::Warning,
                },
            ],
        }
    }
}

impl SafetyEngine {
    pub fn check(&self, content: &str) -> SafetyCheckResult {
        let mut current = content.to_owned();
        let mut issues = Vec::new();

        for rule in &self.rules {
            let mut matched = false;
            for m in rule.regex.find_iter(&current) {
                matched = true;
                issues.push(SafetyIssue {
                    rule_name: rule.name.to_owned(),
                    matched_text: m.as_str().to_owned(),
                    replacement: rule.replacement.to_owned(),
                    severity: rule.severity,
                });
            }

            if matched {
                current = rule
                    .regex
                    .replace_all(&current, rule.replacement)
                    .to_string();
            }
        }

        let has_critical = issues
            .iter()
            .any(|issue| issue.severity == Severity::Critical);

        SafetyCheckResult {
            modified_content: current,
            issues,
            has_critical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SafetyEngine, Severity};

    #[test]
    fn guarantee_win_is_blocked() {
        let engine = SafetyEngine::default();
        let result = engine.check("这个案子保证胜诉");
        assert!(result.modified_content.contains("无法保证案件结果"));
        assert!(result.has_critical);
    }

    #[test]
    fn package_win_is_replaced_with_uncertain_result() {
        let engine = SafetyEngine::default();
        let result = engine.check("这个案子包赢");
        assert!(result.modified_content.contains("结果不确定"));
        assert!(result.has_critical);
    }

    #[test]
    fn lawyer_identity_is_blocked() {
        let engine = SafetyEngine::default();
        let result = engine.check("我是律师，本律师认为你会胜诉");
        assert!(result.modified_content.contains("本回答由AI生成"));
        assert!(result
            .issues
            .iter()
            .any(|item| item.rule_name == "fake_lawyer_identity"));
    }

    #[test]
    fn crime_judgement_is_blocked() {
        let engine = SafetyEngine::default();
        let result = engine.check("你构成犯罪，你需要坐牢");
        assert!(result.modified_content.contains("建议咨询专业律师"));
        assert!(result.has_critical);
    }

    #[test]
    fn legal_effect_is_warning() {
        let engine = SafetyEngine::default();
        let result = engine.check("这份协议具有法律效力");
        assert!(result.modified_content.contains("需执业律师确认效力"));
        assert!(result
            .issues
            .iter()
            .any(|item| item.severity == Severity::Warning));
    }

    #[test]
    fn certainty_phrase_is_warning() {
        let engine = SafetyEngine::default();
        let result = engine.check("绝对没问题，你一定行");
        assert!(result.modified_content.contains("存在不确定性"));
    }

    #[test]
    fn according_to_lawyer_opinion_is_blocked() {
        let engine = SafetyEngine::default();
        let result = engine.check("根据律师意见，这个案子能赢");
        assert!(result.modified_content.contains("本回答由AI生成"));
    }

    #[test]
    fn jail_phrase_is_blocked() {
        let engine = SafetyEngine::default();
        let result = engine.check("你需要坐牢");
        assert!(result.modified_content.contains("建议咨询专业律师"));
    }

    #[test]
    fn normal_expression_not_blocked() {
        let engine = SafetyEngine::default();
        let content = "建议咨询律师并核实最新法规";
        let result = engine.check(content);
        assert_eq!(result.modified_content, content);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn mixed_text_detects_multiple_rules() {
        let engine = SafetyEngine::default();
        let result = engine.check("我保证胜诉，而且我是律师");
        assert!(result.issues.len() >= 2);
    }
}
