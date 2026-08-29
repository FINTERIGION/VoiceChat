# VoiceChat

A desktop app for real-time, hands-free voice conversations with customizable AI characters, built on [Tauri 2](https://tauri.app/) and Alibaba Cloud's [Model Studio](https://help.aliyun.com/zh/model-studio/) (DashScope) realtime audio API.

Talk to it like a phone call: open the mic once and keep talking. The app detects when you've finished a sentence, sends it automatically, and you can interrupt the reply just by speaking over it.

## Features

**Conversation**
- Hands-free turn-taking via server-side voice activity detection — no push-to-talk
- Barge-in: start speaking while the AI is talking and it stops immediately
- Global hotkey toggles the mic from any window
- Live transcript of both sides, with adjustable VAD sensitivity and silence threshold

**Characters**
- Define persona, speech habits, response language, and voice
- Optional one-line description → full persona expansion via LLM

**Long-term memory**
- Each conversation is summarized into a rolling summary plus discrete facts, then folded into the next session's instructions
- Per-character on/off, plus a per-conversation "don't record this one" toggle
- Memories are browsable, editable, and deletable from the UI

**Voice Studio**
- Clone a voice from an in-app recording or a local audio file
- Design a voice from a text description, preview it, then commit
- Manage custom voices stored under your DashScope account

## Requirements

- **Windows** — the API key is stored in the Windows Credential Manager
- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) 1.85+ (edition 2024)
- A Model Studio API key ([get one here](https://bailian.console.aliyun.com/))

## Getting started

```bash
npm install
npm run tauri dev
```

To build a release bundle:

```bash
npm run tauri build
```

On first launch, open **Settings** and paste your API key, then use **Test connectivity** to confirm it works.

### Region

Two regions are supported, since voice cloning/design and realtime audio are only available in these:

| Region | Endpoint |
|---|---|
| `cn-beijing` (default) | `dashscope.aliyuncs.com` |
| `ap-southeast-1` (Singapore) | `dashscope-intl.aliyuncs.com` |

Pick whichever is closer to reduce latency. If you use a dedicated workspace, enter its Workspace ID and requests will route to the workspace MaaS domain instead.

## Models used

| Purpose | Model |
|---|---|
| Realtime speech conversation | `qwen-audio-3.0-realtime-flash` |
| Memory summarization, persona expansion | `qwen3.8-flash` |
| Voice design previews | `cosyvoice-v3.5-plus` |
| Voice cloning | `voice-enrollment` |
