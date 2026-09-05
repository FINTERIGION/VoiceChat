use serde::Deserialize;

use crate::llm::flash::FlashClient;
use crate::store::memory::Memory;
use crate::store::message::Message;

/// Roughly 800 tokens' worth of characters for the injected `[长期记忆]`
/// block. CJK text runs close to 1-1.5 chars/token, so this is a generous
/// but conservative proxy without needing a real tokenizer.
const INJECT_BUDGET_CHARS: usize = 700;
const MAX_FACTS_PER_CHARACTER: usize = 30;
const TOP_K_FOR_INJECTION: usize = 12;
/// Enough of a conversation to tell what it is about. A title only ever
/// describes what the conversation opened with, so feeding the whole
/// transcript would cost tokens without changing the answer.
const TITLE_CONTEXT_MESSAGES: usize = 12;
/// Hard cap on the stored title, in characters. The model is asked for
/// something far shorter; this only catches a model that ignores the
/// instruction, so one bad response can't put a paragraph in the sidebar.
const MAX_TITLE_CHARS: usize = 40;

pub struct SummaryResult {
    pub summary: String,
    pub facts: Vec<String>,
}

/// Summarizes one conversation's transcript into a rolling summary plus
/// discrete facts, folding in whatever summary already existed so the
/// result stays continuous across many short conversations rather than
/// starting fresh each time.
pub async fn summarize_conversation(
    client: &FlashClient,
    character_name: &str,
    previous_summary: Option<&str>,
    messages: &[Message],
) -> Result<SummaryResult, String> {
    if messages.is_empty() {
        return Err(crate::tr!("Nothing in this conversation to summarize", "没有可摘要的对话内容").to_string());
    }

    let transcript = messages
        .iter()
        .map(|m| {
            let speaker = if m.role == "user" { "用户" } else { character_name };
            format!("{speaker}: {}", m.text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let system = "你是语音陪聊应用的记忆整理助手。基于本轮对话记录（可能附带历史摘要），\
        输出严格的 JSON，不要有任何其他文字：\
        {\"summary\": \"合并历史摘要与本轮内容后的滚动摘要，不超过200字\", \
        \"facts\": [\"结构化事实，如用户偏好/称呼/约定，每条不超过40字\"]}\
        facts 最多 5 条，没有新事实时给空数组。";

    let user = match previous_summary {
        Some(prev) if !prev.trim().is_empty() => {
            format!("历史摘要：{prev}\n\n本轮对话：\n{transcript}")
        }
        _ => format!("本轮对话：\n{transcript}"),
    };

    let raw = client.complete(system, &user).await?;

    #[derive(Deserialize)]
    struct Parsed {
        summary: String,
        #[serde(default)]
        facts: Vec<String>,
    }
    let parsed: Parsed = serde_json::from_str(raw.trim())
        .map_err(|e| {
            crate::tr!(
                format!("Could not parse the summary: {e}; raw: {}", crate::dashscope::snippet(&raw)),
                format!("解析摘要失败: {e}; 原始: {}", crate::dashscope::snippet(&raw)),
            )
        })?;

    Ok(SummaryResult {
        summary: parsed.summary,
        facts: parsed.facts,
    })
}

/// Names a conversation for the Chat tab's history list.
///
/// Deliberately a separate, cheap call rather than another field on
/// `summarize_conversation`: naming happens early, while the conversation is
/// still going and there is only a turn or two to go on, whereas
/// summarization happens once at the end over the whole transcript.
pub async fn generate_title(
    client: &FlashClient,
    character_name: &str,
    messages: &[Message],
) -> Result<String, String> {
    let transcript = messages
        .iter()
        .filter(|m| !m.text.trim().is_empty())
        .take(TITLE_CONTEXT_MESSAGES)
        .map(|m| {
            let speaker = if m.role == "user" { "用户" } else { character_name };
            format!("{speaker}: {}", m.text)
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.is_empty() {
        return Err(crate::tr!("Nothing in this conversation to name", "没有可命名的对话内容").to_string());
    }

    let system = "你是对话标题助手。根据对话记录，起一个概括主题的短标题。\
        要求：中文不超过 12 个字，英文不超过 6 个词；\
        使用与对话内容相同的语言；不要引号、书名号，不要以标点结尾；\
        只输出标题本身，不要任何解释。";

    let raw = client.complete(system, &transcript).await?;
    let title = clean_title(&raw);
    if title.is_empty() {
        return Err(crate::tr!(
            format!("The model returned no usable title; raw: {}", crate::dashscope::snippet(&raw)),
            format!("模型没有返回可用的标题; 原始: {}", crate::dashscope::snippet(&raw)),
        ));
    }
    Ok(title)
}

/// Reduces a model response to something that fits one sidebar row: the
/// first line only (a model that adds an explanation puts it below), with
/// the quotes it likes to wrap titles in stripped off.
fn clean_title(raw: &str) -> String {
    const WRAPPERS: &[char] = &['"', '\'', '“', '”', '‘', '’', '《', '》', '「', '」'];
    raw.trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| WRAPPERS.contains(&c))
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Writes a summarization result into storage: replaces the rolling summary
/// row, inserts new facts, and caps total fact count so memories don't grow
/// unbounded over a long-lived character.
pub fn store_summary(
    conn: &rusqlite::Connection,
    character_id: &str,
    result: &SummaryResult,
) -> Result<(), String> {
    crate::store::memory::replace_summary(conn, character_id, &result.summary)
        .map_err(|e| e.to_string())?;
    for fact in &result.facts {
        if fact.trim().is_empty() {
            continue;
        }
        crate::store::memory::create(conn, character_id, "fact", fact, 0.5)
            .map_err(|e| e.to_string())?;
    }
    crate::store::memory::cap_facts(conn, character_id, MAX_FACTS_PER_CHARACTER)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn concat_memories(memories: &[Memory]) -> String {
    let mut block = String::new();
    for m in memories {
        if !block.is_empty() {
            block.push('\n');
        }
        block.push_str(m.content.trim());
        if block.chars().count() >= INJECT_BUDGET_CHARS {
            break;
        }
    }
    block
}

/// Sync DB read, kept separate from `build_injection_block` below so
/// callers never hold the `rusqlite::Connection` mutex guard across an
/// `.await` (which would make the enclosing actor future non-`Send` and
/// fail to compile under `tauri::async_runtime::spawn`).
pub fn top_k_memories(
    conn: &rusqlite::Connection,
    character_id: &str,
) -> Result<Vec<Memory>, String> {
    crate::store::memory::top_k(conn, character_id, TOP_K_FOR_INJECTION).map_err(|e| e.to_string())
}

/// Builds the `[长期记忆]` block for a character's next `instructions` from
/// already-fetched top-K memories, compressing via `flash` if they still
/// don't fit the injection budget after concatenation.
pub async fn build_injection_block(client: &FlashClient, memories: &[Memory]) -> Option<String> {
    if memories.is_empty() {
        return None;
    }

    let block = concat_memories(memories);
    if block.chars().count() <= INJECT_BUDGET_CHARS {
        return Some(block);
    }

    let system = "请把下面的长期记忆内容压缩到300字以内，保留最重要的信息，直接输出压缩后的正文，不要解释、不要加引号。";
    match client.complete(system, &block).await {
        Ok(compressed) => Some(compressed),
        Err(e) => {
            tracing::warn!("memory compression failed, truncating instead: {e}");
            Some(block.chars().take(INJECT_BUDGET_CHARS).collect())
        }
    }
}
