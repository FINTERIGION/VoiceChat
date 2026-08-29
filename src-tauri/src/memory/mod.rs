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
        return Err("没有可摘要的对话内容".to_string());
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
        .map_err(|e| format!("解析摘要失败: {e}; 原始: {raw}"))?;

    Ok(SummaryResult {
        summary: parsed.summary,
        facts: parsed.facts,
    })
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
