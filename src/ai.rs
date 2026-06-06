use serde_json::{json, Value};
use std::{env, fs};

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
}

impl AiConfig {
    pub fn from_env() -> Self {
        let dotenv = fs::read_to_string(".env").unwrap_or_default();
        Self::from_pairs_and_dotenv(env::vars(), &dotenv)
    }

    pub fn from_pairs_and_dotenv<I, K, V>(vars: I, dotenv: &str) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env_pairs: Vec<(String, String)> = vars
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        let dotenv_pairs = parse_dotenv(dotenv);

        let get = |name: &str| {
            env_pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .or_else(|| {
                    dotenv_pairs
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.clone())
                })
                .filter(|value| !value.trim().is_empty())
        };

        let api_key = get("DEEPSEEK_API_KEY").or_else(|| get("OPENAI_API_KEY"));
        let base_url = get("DEEPSEEK_BASE_URL")
            .or_else(|| get("OPENAI_BASE_URL"))
            .unwrap_or_else(|| "https://api.deepseek.com".to_string());
        let model = get("DEEPSEEK_MODEL")
            .or_else(|| get("OPENAI_MODEL"))
            .unwrap_or_else(|| "deepseek-chat".to_string());

        Self {
            api_key,
            base_url,
            model,
        }
    }

    pub fn chat_endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

fn parse_dotenv(input: &str) -> Vec<(String, String)> {
    input
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            let (key, value) = trimmed.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }

            Some((key.to_string(), unquote_dotenv_value(value.trim())))
        })
        .collect()
}

fn unquote_dotenv_value(value: &str) -> String {
    let without_comment = if value.starts_with('"') || value.starts_with('\'') {
        value
    } else {
        value.split('#').next().unwrap_or(value).trim_end()
    };

    if without_comment.len() >= 2 {
        let bytes = without_comment.as_bytes();
        let first = bytes[0];
        let last = bytes[without_comment.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return without_comment[1..without_comment.len() - 1].to_string();
        }
    }

    without_comment.to_string()
}

#[derive(Debug, Clone)]
pub struct AiAnalysis {
    pub severity: String,
    pub summary: String,
    pub suggestions: Vec<String>,
}

impl AiAnalysis {
    pub fn to_panel_text(&self) -> String {
        let command = self
            .suggestions
            .iter()
            .find_map(|suggestion| extract_command(suggestion))
            .unwrap_or_else(|| "无".to_string());

        format!(
            "警告: {}\n问题: {}\n命令: {}",
            self.severity, self.summary, command,
        )
    }
}

fn extract_command(text: &str) -> Option<String> {
    if let Some(start) = text.find('`') {
        if let Some(end) = text[start + 1..].find('`') {
            let command = text[start + 1..start + 1 + end].trim();
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }

    let trimmed = text.trim().trim_end_matches('。');
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    {
        return None;
    }

    Some(trimmed.to_string())
}

fn analysis_from_model_text(content: &str) -> AiAnalysis {
    let mut severity = "参考".to_string();
    let mut summary = content.trim().to_string();
    let mut suggestions = Vec::new();

    for line in content
        .lines()
        .flat_map(|line| line.split(['；', ';']))
        .map(str::trim)
    {
        if let Some(value) = line.strip_prefix("警告:") {
            severity = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("问题:") {
            summary = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("命令:") {
            let command = value.trim();
            if !command.is_empty() && command != "无" {
                suggestions.push(command.to_string());
            }
        }
    }

    AiAnalysis {
        severity,
        summary,
        suggestions,
    }
}

pub fn build_prompt(node_name: &str, user: &str, context: &str, previous_advice: &str) -> String {
    let previous_section = if previous_advice.trim().is_empty() {
        "无".to_string()
    } else {
        previous_advice.trim().to_string()
    };

    format!(
        "你是 Linux 运维助手。用户正在通过 BenSSH 的 AI SSH 模式连接节点 `{}`，登录用户 `{}`。\n\
请根据下面最近的终端输出判断是否存在当前仍需处理的错误、安全风险或问题，务必短，不要长篇解释。\n\
只输出 3 行：警告: 低/中/高/严重；问题: 一句话；命令: 一个可复制命令，没有就写“无”。\n\
如果最近终端输出没有问题，只输出：警告: 无；问题: 无；命令: 无。\n\
先对比“上一轮 AI 建议”和“最近终端输出”。如果同类问题反复出现，不要重复给同一个修复命令；优先给验证当前状态的命令，帮助判断修复是否已生效、提示是否是缓存、是否需要下一步排查。\n\
如果终端上下文显示用户已经执行过你建议的修复命令，或系统重启后仍出现同类提示，不要重复给同一个修复命令。\n\
登录 banner、历史提示、上一次登录失败记录只能作为参考；除非最近输出证明问题仍存在，否则不要告警。\n\
遇到内核安全更新、当前运行内核版本、重启是否生效这类提示时，优先给验证命令，例如 `uname -r && rpm -q kernel-core --last | head -5`，只有确认仍需重启时才建议重启。\n\
不要编号，不要 Markdown，不要编造，不要要求用户泄露密钥或凭证。\n\n\
上一轮 AI 建议：\n{}\n\n\
最近终端输出：\n{}",
        node_name, user, previous_section, context
    )
}

pub async fn analyze_with_model(
    config: &AiConfig,
    node_name: &str,
    user: &str,
    context: &str,
    previous_advice: &str,
) -> Result<AiAnalysis, Box<dyn std::error::Error + Send + Sync>> {
    let api_key = config
        .api_key
        .as_ref()
        .ok_or("未配置 API Key，无法调用大模型")?;
    let prompt = build_prompt(node_name, user, context, previous_advice);
    let client = reqwest::Client::new();
    let response: Value = client
        .post(config.chat_endpoint())
        .bearer_auth(api_key)
        .json(&json!({
            "model": config.model,
            "messages": [
                {
                    "role": "system",
                    "content": "你是谨慎的 Linux 运维诊断助手，只提供解释和建议，不自动执行命令。"
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.2
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("模型返回格式异常")?
        .trim()
        .to_string();

    Ok(analysis_from_model_text(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_config_prefers_deepseek_variables() {
        let vars = [
            ("DEEPSEEK_API_KEY", "deepseek-key"),
            ("DEEPSEEK_BASE_URL", "https://api.deepseek.com"),
            ("DEEPSEEK_MODEL", "deepseek-chat"),
            ("OPENAI_API_KEY", "openai-key"),
            ("OPENAI_BASE_URL", "https://api.openai.com/v1"),
            ("OPENAI_MODEL", "gpt-4.1-mini"),
        ];
        let config = AiConfig::from_pairs_and_dotenv(vars, "");
        assert_eq!(config.api_key.as_deref(), Some("deepseek-key"));
        assert_eq!(config.base_url, "https://api.deepseek.com");
        assert_eq!(config.model, "deepseek-chat");
    }

    #[test]
    fn ai_config_uses_dotenv_when_env_is_missing() {
        let dotenv = r#"
            # DeepSeek settings
            DEEPSEEK_API_KEY="dotenv-key"
            DEEPSEEK_BASE_URL=https://api.deepseek.com
            DEEPSEEK_MODEL='deepseek-chat'
        "#;

        let config = AiConfig::from_pairs_and_dotenv(Vec::<(&str, &str)>::new(), dotenv);

        assert_eq!(config.api_key.as_deref(), Some("dotenv-key"));
        assert_eq!(config.base_url, "https://api.deepseek.com");
        assert_eq!(config.model, "deepseek-chat");
    }

    #[test]
    fn ai_config_env_overrides_dotenv() {
        let env_vars = [("DEEPSEEK_API_KEY", "env-key")];
        let dotenv = "DEEPSEEK_API_KEY=dotenv-key";

        let config = AiConfig::from_pairs_and_dotenv(env_vars, dotenv);

        assert_eq!(config.api_key.as_deref(), Some("env-key"));
    }

    #[test]
    fn chat_endpoint_normalizes_base_url() {
        let config = AiConfig {
            api_key: Some("k".into()),
            base_url: "https://api.deepseek.com/".into(),
            model: "deepseek-chat".into(),
        };
        assert_eq!(
            config.chat_endpoint(),
            "https://api.deepseek.com/chat/completions"
        );
    }

    #[test]
    fn prompt_does_not_include_secret_fields() {
        let prompt = build_prompt("prod", "root", "Permission denied", "");
        assert!(prompt.contains("prod"));
        assert!(prompt.contains("root"));
        assert!(prompt.contains("Permission denied"));
        assert!(!prompt.to_lowercase().contains("password"));
        assert!(!prompt.contains("keyPath"));
    }

    #[test]
    fn prompt_requires_short_four_line_answer() {
        let prompt = build_prompt("prod", "root", "recent terminal output", "");
        assert!(prompt.contains("只输出 3 行"));
        assert!(prompt.contains("警告:"));
        assert!(prompt.contains("命令:"));
        assert!(prompt.contains("警告: 无"));
        assert!(prompt.contains("不要重复给同一个修复命令"));
        assert!(prompt.contains("uname -r"));
        assert!(prompt.contains("不要 Markdown"));
    }

    #[test]
    fn prompt_includes_previous_advice_to_avoid_repeated_fix_command() {
        let prompt = build_prompt(
            "prod",
            "root",
            "Security: kernel-core is an installed security update\nSecurity: kernel-core is the currently running version",
            "警告: 高\n问题: 安全内核已安装但未生效\n命令: systemctl reboot",
        );

        assert!(prompt.contains("上一轮 AI 建议"));
        assert!(prompt.contains("systemctl reboot"));
        assert!(prompt.contains("优先给验证当前状态的命令"));
        assert!(prompt.contains("不要重复给同一个修复命令"));
    }

    #[test]
    fn panel_text_only_contains_warning_problem_and_command() {
        let text = AiAnalysis {
            severity: "高".to_string(),
            summary: "服务端口拒绝连接。".to_string(),
            suggestions: vec!["运行 `ss -lntp` 查看监听端口。".to_string()],
        }
        .to_panel_text();

        assert_eq!(text, "警告: 高\n问题: 服务端口拒绝连接。\n命令: ss -lntp");
    }

    #[test]
    fn model_text_is_parsed_into_short_panel() {
        let text = analysis_from_model_text(
            "警告: 高\n问题: SSH 暴力登录尝试过多。\n命令: fail2ban-client status sshd",
        )
        .to_panel_text();

        assert_eq!(
            text,
            "警告: 高\n问题: SSH 暴力登录尝试过多。\n命令: fail2ban-client status sshd"
        );
    }

    #[test]
    fn model_text_parses_semicolon_separated_three_fields() {
        let text = analysis_from_model_text("警告: 无；问题: 无；命令: 无").to_panel_text();

        assert_eq!(text, "警告: 无\n问题: 无\n命令: 无");
    }
}
