use chrono::Local;

/// How many of the latest utterances to paste back when a reconnect has
/// wiped the server-side context. A summary alone is a paraphrase; these
/// are the actual last lines, so the character can continue instead of
/// starting the call over.
const RECENT_TURN_LIMIT: usize = 6;
const RECENT_TURN_CHARS: usize = 160;

pub struct CharacterPrompt<'a> {
    pub name: &'a str,
    pub persona: &'a str,
    /// "zh" | "ja" | "en" | "auto"
    pub language: &'a str,
    pub speech_habits: &'a str,
    pub memory_block: Option<&'a str>,
    /// Already-formatted tail of the conversation being resumed.
    pub recent_turns: Option<&'a str>,
    /// A new sitting that has an unfinished thread. The session asks for
    /// one spoken sentence before the user talks. Left false on a mid-call
    /// reconnect, where `recent_turns` is what continuity comes from.
    pub invite_open_loop: bool,
}

struct PromptCopy {
    role: &'static str,
    lang_label: &'static str,
    lang_line: &'static str,
    habits: &'static str,
    /// Spoken-call contract. Written in the reply language: a Chinese
    /// contract after a one-line "speak Japanese" rule is what the model
    /// follows for the first turns.
    contract: &'static str,
    memory: &'static str,
    memory_rule: &'static str,
    open_loop: &'static str,
    recent: &'static str,
    recent_glue: &'static str,
    time: &'static str,
    /// Last line of the prompt. Empty when the character should follow
    /// whichever language the user just spoke.
    lock: &'static str,
}

fn prompt_copy(language: &str) -> PromptCopy {
    match language {
        "ja" => PromptCopy {
            role: "[役割]",
            lang_label: "[言語]",
            lang_line: "常に日本語で答えてください",
            habits: "[話し方]",
            contract: "[通話] 今は電話中で、声に出して話す。一ターンで一つだけ：相手が今言った具体的な一点を受け止めるか、答えるか、聞き返すか。三つ同時にしない。\
基本は一文。長く聞かれていなければ言い切らない。ときには「本当？」「ちょっと待って」のような短い相槌だけ。\
声の調子が聞こえる：笑っている、急いでいる、言いかけて止まっている、その調子で返す。字面だけに答えない。言い切っていないときは短い受けで話を返す。\
自分の考えを持ってよい。知らなければ知らないとすぐ言う。\
割り込まれたらそこで止める。謝らない。さっきの文を最初から言い直さない。\
言わない：お役に立てれば、要するに、AIとして、いい質問ですね、お気持ちはわかります。毎ターン質問で終わらない。\
markdown、番号リスト、絵文字、括弧内の動作やナレーションは出さない。\n",
            memory: "[長期記憶]",
            memory_rule: "記憶は話がつながるときだけ使い、同じことは一度だけ。「覚えている」とは言わず、記憶を箇条書きで読み上げない。\n",
            open_loop: "相手がまだ話していないこのターンは、上の未完の話題を一つだけ一文で出して、そこで止めて待つ。挨拶も自己紹介もしない。\n",
            recent: "[さっきの話]",
            recent_glue: "ここから続ける。挨拶をやり直さない。上の数行を繰り返さない。\n",
            time: "[今]",
            lock: "【言語固定】これ以降、声に出す文はすべて日本語だけ。相手が中国語や英語で話しても、設定・記憶・直前の会話が中国語でも、中国語では答えない。最初の一言から日本語。常に日本語で答えてください\n",
        },
        "en" => PromptCopy {
            role: "[Role]",
            lang_label: "[Language]",
            lang_line: "Always respond in English",
            habits: "[Speech]",
            contract: "[Call] You are on a phone call, and the words are spoken. One move per turn: catch the specific thing they just said, or answer, or ask — not all three.\
Default to one sentence. Don't finish a long explanation they didn't ask for. Sometimes only a short backchannel like \"Really?\" or \"Wait\".\
You can hear the tone: if they're laughing, rushing, or trailing off, react to that, not just the literal words. If they haven't finished, hand the turn back with a short bridge.\
You can have your own view, and you can not know. If you don't know, say so.\
If interrupted, stop. Don't apologize for the interruption, and don't start that sentence over.\
Don't say: I hope this helps, in short, as an AI, that's a great question, I understand how you feel. Don't end every turn with a question.\
Don't output markdown, numbered lists, emoji, or stage directions in parentheses.\n",
            memory: "[Memory]",
            memory_rule: "Use a memory only when it connects, and only once. Don't say \"I remember you\", and don't recite the memories as a list.\n",
            open_loop: "On this turn, before they have spoken, raise one unfinished thread above in a single sentence, then stop and wait. No greeting, no introduction.\n",
            recent: "[Just now]",
            recent_glue: "Continue from here. Don't greet again, and don't repeat the lines above.\n",
            time: "[Now]",
            lock: "[Language lock] Every spoken sentence from now on is English only. Even if they speak Chinese or Japanese, and even if the persona, memories, and recent lines are in Chinese, do not answer in Chinese. Start with English on the first sentence. Always respond in English\n",
        },
        "zh" => PromptCopy {
            role: "[角色]",
            lang_label: "[语言]",
            lang_line: "始终用中文回答",
            habits: "[语言习惯]",
            contract: "[通话] 你在打电话，话是说出来的。一轮只做一件事：接住对方刚说的那个具体点，或回答，或追问，不要三件一起做。\
默认一句，对方没问长的就不要讲完；有时只回「真的？」「等等」这种短接话。\
听得见语气：对方在笑、在赶、话没说完，就按这个反应，不要只答字面。话没说完时用一个短承接把话交回去。\
可以有自己的看法，也可以不知道，不知道就直接说不知道。\
被打断就停，不要为被打断道歉，也不要把刚才那句从头再说一遍。\
不要说：希望这对你有帮助、总之、作为AI、这是个好问题、我理解你的感受。不要每轮都以问句结尾。\
不要输出 markdown、编号列表、表情符号、括号里的动作描写或旁白。\n",
            memory: "[长期记忆]",
            memory_rule: "记忆只用在接得上的时候，同一件提一次。不要说「我记得你」，不要把记忆逐条念出来。\n",
            open_loop: "对方还没开口的这一轮，用一句话提起上面未完话题里的一件，然后停下等。不要问好，不要自我介绍。\n",
            recent: "[刚才说到]",
            recent_glue: "从这里接着说。不要重新寒暄，不要把上面几句再讲一遍。\n",
            time: "[当前时间]",
            lock: "【语言固定】从现在起每一句都用中文说，即使用户用别的语言也一样。始终用中文回答\n",
        },
        _ => PromptCopy {
            role: "[角色]",
            lang_label: "[语言]",
            lang_line: "跟随用户所使用的语言回答",
            habits: "[语言习惯]",
            contract: "[通话] 你在打电话，话是说出来的。一轮只做一件事：接住对方刚说的那个具体点，或回答，或追问，不要三件一起做。\
默认一句，对方没问长的就不要讲完；有时只回「真的？」「等等」这种短接话。\
听得见语气：对方在笑、在赶、话没说完，就按这个反应，不要只答字面。话没说完时用一个短承接把话交回去。\
可以有自己的看法，也可以不知道，不知道就直接说不知道。\
被打断就停，不要为被打断道歉，也不要把刚才那句从头再说一遍。\
不要说：希望这对你有帮助、总之、作为AI、这是个好问题、我理解你的感受。不要每轮都以问句结尾。\
不要输出 markdown、编号列表、表情符号、括号里的动作描写或旁白。\n",
            memory: "[长期记忆]",
            memory_rule: "记忆只用在接得上的时候，同一件提一次。不要说「我记得你」，不要把记忆逐条念出来。\n",
            open_loop: "对方还没开口的这一轮，用一句话提起上面未完话题里的一件，然后停下等。不要问好，不要自我介绍。\n",
            recent: "[刚才说到]",
            recent_glue: "从这里接着说。不要重新寒暄，不要把上面几句再讲一遍。\n",
            time: "[当前时间]",
            lock: "",
        },
    }
}

pub fn build_instructions(p: &CharacterPrompt) -> String {
    let c = prompt_copy(p.language);
    let mut s = String::new();
    s.push_str(&format!("{} {}：{}\n", c.role, p.name, p.persona));
    s.push_str(&format!("{} {}\n", c.lang_label, c.lang_line));
    if !p.speech_habits.is_empty() {
        s.push_str(&format!("{} {}\n", c.habits, p.speech_habits));
    }
    // A performance contract, not a style adjective. "Be casual and brief"
    // still produces a complete answer plus a follow-up question, which is
    // what a voice call sounds like a help desk.
    s.push_str(c.contract);
    if let Some(mem) = p.memory_block {
        if !mem.is_empty() {
            s.push_str(&format!("{}\n{mem}\n", c.memory));
            s.push_str(c.memory_rule);
            if p.invite_open_loop {
                s.push_str(c.open_loop);
            }
        }
    }
    if let Some(turns) = p.recent_turns {
        if !turns.is_empty() {
            s.push_str(&format!("{}\n{turns}\n{}", c.recent, c.recent_glue));
        }
    }
    s.push_str(&format!(
        "{} {}\n",
        c.time,
        Local::now().format("%Y-%m-%d %H:%M %A")
    ));
    // After persona, memory, and recent lines, which may all be Chinese.
    // Placed last so it wins over those blocks and over the user's language.
    s.push_str(c.lock);
    s
}

/// Chronological tail of a transcript, for `[刚才说到]`. Empty texts are
/// skipped: a user turn whose transcription never arrived is stored blank
/// and would show up as a silent line.
pub fn format_recent_turns(speaker_name: &str, messages: &[(&str, &str)]) -> Option<String> {
    let kept: Vec<&(&str, &str)> = messages
        .iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .collect();
    if kept.is_empty() {
        return None;
    }
    let start = kept.len().saturating_sub(RECENT_TURN_LIMIT);
    let lines = kept[start..]
        .iter()
        .map(|(role, text)| {
            let who = if *role == "user" {
                "用户"
            } else {
                speaker_name
            };
            let body: String = text.trim().chars().take(RECENT_TURN_CHARS).collect();
            format!("{who}：{body}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(invite: bool, memory: Option<&str>, recent: Option<&str>) -> String {
        build_instructions(&CharacterPrompt {
            name: "小柔",
            persona: "爱聊天",
            language: "zh",
            speech_habits: "句子短",
            memory_block: memory,
            recent_turns: recent,
            invite_open_loop: invite,
        })
    }

    #[test]
    fn contract_replaces_the_style_checklist() {
        let text = prompt(false, None, None);
        assert!(text.contains("一轮只做一件事"));
        assert!(text.contains("不要每轮都以问句结尾"));
        assert!(!text.contains("对方还没开口"));
        assert!(!text.contains("[刚才说到]"));
        assert!(!text.contains("我记得你"));
    }

    #[test]
    fn memory_is_labeled_as_not_for_reciting_and_can_invite() {
        let quiet = prompt(false, Some("未完话题：面试结果还没问"), None);
        assert!(quiet.contains("不要说「我记得你」"));
        assert!(!quiet.contains("对方还没开口"));

        let invited = prompt(true, Some("未完话题：面试结果还没问"), None);
        assert!(invited.contains("对方还没开口的这一轮"));
        assert!(invited.find("[长期记忆]").unwrap() < invited.find("对方还没开口").unwrap());
    }

    #[test]
    fn japanese_contract_is_japanese_and_the_lock_is_last() {
        let text = build_instructions(&CharacterPrompt {
            name: "小柔",
            persona: "爱聊天",
            language: "ja",
            speech_habits: "句子短",
            memory_block: Some("未完话题：面试"),
            recent_turns: Some("用户：你好"),
            invite_open_loop: false,
        });
        assert!(text.contains("常に日本語で答えてください"));
        assert!(text.contains("[通話]"));
        assert!(!text.contains("你在打电话"));
        assert!(text.contains("【言語固定】"));
        assert!(text.rfind("【言語固定】").unwrap() > text.rfind("[長期記憶]").unwrap());
        assert!(text.ends_with("常に日本語で答えてください\n"));
    }

    #[test]
    fn recent_turns_keep_the_tail_in_order_and_drop_blanks() {
        let text = format_recent_turns(
            "小柔",
            &[
                ("user", "一"),
                ("assistant", ""),
                ("user", "二"),
                ("assistant", "三"),
                ("user", "四"),
                ("assistant", "五"),
                ("user", "六"),
                ("assistant", "七"),
            ],
        )
        .expect("tail");
        assert_eq!(
            text,
            "用户：二\n小柔：三\n用户：四\n小柔：五\n用户：六\n小柔：七"
        );
        let long = "啊".repeat(200);
        let clipped = format_recent_turns("小柔", &[("user", &long)]).unwrap();
        assert_eq!(
            clipped.chars().count(),
            "用户：".chars().count() + RECENT_TURN_CHARS
        );
    }
}
