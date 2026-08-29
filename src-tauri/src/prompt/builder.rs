use chrono::Local;

pub struct CharacterPrompt<'a> {
    pub name: &'a str,
    pub persona: &'a str,
    /// "zh" | "ja" | "en" | "auto"
    pub language: &'a str,
    pub speech_habits: &'a str,
    pub memory_block: Option<&'a str>,
}

pub fn build_instructions(p: &CharacterPrompt) -> String {
    let lang_line = match p.language {
        "zh" => "始终用中文回答",
        "ja" => "常に日本語で答えてください",
        "en" => "Always respond in English",
        _ => "跟随用户所使用的语言回答",
    };

    let mut s = String::new();
    s.push_str(&format!("[角色] {}：{}\n", p.name, p.persona));
    s.push_str(&format!("[语言] {lang_line}\n"));
    if !p.speech_habits.is_empty() {
        s.push_str(&format!("[语言习惯] {}\n", p.speech_habits));
    }
    s.push_str(
        "[对话风格] 口语化、句子短、允许被打断；这是语音对话——不要输出 markdown、编号列表、表情符号、括号里的动作描写或旁白\n",
    );
    if let Some(mem) = p.memory_block {
        if !mem.is_empty() {
            s.push_str(&format!("[长期记忆] {mem}\n"));
        }
    }
    s.push_str(&format!(
        "[当前时间] {}\n",
        Local::now().format("%Y-%m-%d %H:%M %A")
    ));
    s
}
