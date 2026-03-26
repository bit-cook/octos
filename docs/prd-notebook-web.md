# MoFa Notebook — PRD 与开发计划

> 目标：基于 octos-web 扩展为类 NotebookLM 的智能阅读与课件生成平台
> 核心场景：图书馆把馆藏书籍批量导入，变成可对话、可生成课件的互动式 Notebook
> 前端仓库：https://github.com/BH3GEI/octos-web
> 日期：2026-03-25

---

## 一、现状分析

### 1.1 octos-web 已有能力

| 能力 | 状态 | 技术实现 |
|---|---|---|
| SSE 流式聊天 | ✅ 成熟 | `@assistant-ui/react` + 自定义 `octos-adapter.ts` |
| Markdown 渲染 | ✅ 成熟 | react-markdown + GFM + KaTeX + Mermaid + 代码高亮 |
| 文件上传 | ✅ 成熟 | 拖拽/粘贴/多文件，含图片/音频/视频预览 |
| 语音/视频录制 + 拍照 | ✅ 成熟 | MediaRecorder API |
| 9 种工具调用 UI | ✅ 成熟 | shell/read_file/write_file/edit_file/web_search/web_fetch/grep/glob/generic |
| 认证系统 | ✅ 成熟 | Email OTP + Admin Token |
| 会话管理 | ✅ 成熟 | 多会话切换/删除/历史恢复 |
| 深色/浅色主题 | ✅ 成熟 | CSS 变量 + localStorage |
| 音视频播放器 | ✅ 成熟 | 自定义 MediaPlayer 组件 |
| 斜杠命令 | ✅ 成熟 | /new /clear /delete /help + 服务端命令 |
| E2E 测试 | ✅ 7 个 Playwright 测试 |
| 文件下载链接识别 | ✅ 自动检测 pdf/pptx/docx/xlsx |

### 1.2 NotebookLM 功能 vs MoFa 差距

| NotebookLM 功能 | MoFa 后端/Skills | octos-web 前端 | 差距 |
|---|---|---|---|
| **Sources 管理** | `mofa-pdf`(★5)、`mofa-paddleocr`(★3)、`mofa-defuddle`(★3) | 有文件上传，无来源管理概念 | 🟡 需增加 Notebook + Source 数据模型和 UI |
| **RAG 对话+引用** | Octos `HybridSearch`；`mofa-memory`(★4) | 有完整聊天 UI，无引用机制 | 🟡 需增加引用标记渲染和跳转 |
| **Notes 笔记** | 无 | 无 | 🔴 需新增 |
| **Slide Deck** | `mofa-slides`(★5) + `mofa-pptx`(★5) | 有文件下载链接识别 | 🟢 后端成熟，需 Studio UI |
| **Audio Overview** | `mofa-fm`(★4) + `mofa-fm-api`(★4) | 有音频播放器 | 🟢 后端+播放器都有，需脚本生成 |
| **Infographic** | `mofa-infographic`(★4) | 无 | 🟢 后端成熟，需 Studio UI |
| **Quiz/Flashcard** | LLM 可生成 | 无 | 🟡 需 Prompt + 前端交互 |
| **Mind Map** | LLM 可生成结构化数据 | 有 Mermaid 图表渲染 | 🟡 Mermaid 可用于基础版 |
| **Deep Research** | `mofa-research-2.0`(★5) | 有 deep-research E2E 测试 | 🟢 基本就绪，需 UI 包装 |
| **Report** | `mofa-docx`(★5) + `mofa-xlsx`(★4) | 有文件下载 | 🟢 后端成熟，需 Studio UI |
| **Comic** | `mofa-comic`(★4) | 无 | 🟢 后端可用，需 Studio UI |
| **协作分享** | Octos 多用户系统 | 有基础认证 | 🟡 需扩展 |

### 1.3 关键结论

octos-web 已经是一个**功能完善的 AI 聊天前端**。要变成 MoFa Notebook，核心工作是：

1. **增加 Notebook 层级** — 在现有 session 之上加 notebook → sources → notes 概念
2. **Sources 管理 UI** — 来源上传/浏览/勾选/预览
3. **引用追溯** — 聊天回复中的引用可点击跳转到来源原文
4. **Studio 输出面板** — 集成 mofa-skills 的多格式课件生成
5. **图书馆书目** — 馆藏导入/分类浏览

不需要重写现有聊天/渲染/上传/认证能力，在其基础上**增量扩展**。

### 1.4 Octos 后端需修复的 Bug

| Bug | 严重度 | 说明 |
|---|---|---|
| `recall_memory` 空库报错 | 🟡 Medium | 空记忆库时返回错误而非空结果 |
| `/api/upload` FormData 处理 | 🟡 Medium | FormData 场景下上传失败 |

---

## 二、产品定位

**MoFa Notebook** — 开源、可私有部署的图书馆智能阅读平台，把每一本书变成可对话、可生成课件的互动式 Notebook。

| 角色 | 场景 |
|---|---|
| **图书馆管理员** | 批量导入馆藏（PDF/扫描件）→ 自动建 Notebook → 管理分类 |
| **教师** | 选书 → AI 生成课件/测验/音频讲解 → 分享给学生 |
| **学生** | 浏览书目 → 与书对话 → 笔记/闪卡/测验 → 跨书研究 |

核心差异化：开源私有部署 · 馆藏级规模 · 多模型 · 输出格式远超 NotebookLM · 多渠道推送

---

## 三、开发计划（按 Milestone）

### Milestone 0: 基础准备
> 后端 API 扩展 + bug 修复

| # | Issue | Tags |
|---|---|---|
| 0.1 | 修复 `recall_memory` 空库报错 | `backend` `bug` |
| 0.2 | 修复 `/api/upload` FormData 处理 | `backend` `bug` |
| 0.3 | 设计 Notebook 数据模型（notebook → sources → notes → outputs） | `backend` `data-model` |
| 0.4 | 实现 Notebook CRUD API | `backend` `api` |
| 0.5 | 实现 Source CRUD API（上传/URL/文本 → 解析 → 分块 → 索引） | `backend` `api` `rag` |
| 0.6 | 文档解析管道：集成 `mofa-pdf` + `mofa-pdf-convert` + `mofa-paddleocr` | `backend` `skill-integration` |
| 0.7 | 实现 Note CRUD API | `backend` `api` |
| 0.8 | Notebook Chat API（`/api/notebooks/:id/chat`，基于来源的 RAG + SSE） | `backend` `api` `rag` |
| 0.9 | 引用标记机制 — LLM 回复中嵌入 `[src:chunk_id]` | `backend` `rag` `llm` |

---

### Milestone 1: Notebook 核心 UI
> 在 octos-web 现有聊天 UI 基础上增加 Notebook 功能

| # | Issue | Tags |
|---|---|---|
| 1.1 | Notebook 列表页（创建/打开/删除/搜索/封面） | `frontend` `ui` |
| 1.2 | Notebook 路由（`/notebooks` 列表 → `/notebooks/:id` 详情） | `frontend` `routing` |
| 1.3 | Sources 管理 UI（来源列表 + 上传入口 + 文件类型图标 + 来源预览抽屉） | `frontend` `ui` |
| 1.4 | Source 上传交互（复用现有文件上传，增加 URL 导入和文本粘贴模式） | `frontend` `ui` |
| 1.5 | Source 勾选过滤（勾选/取消特定来源参与对话） | `frontend` `ui` |
| 1.6 | 引用渲染 — 在 Markdown 渲染器中解析 `[src:N]` 为可点击引用标记 | `frontend` `ui` |
| 1.7 | 引用跳转 — 点击引用 → 打开来源预览 + 高亮原文段落 | `frontend` `ui` `ux` |
| 1.8 | 建议问题 — 打开 Notebook 时显示基于来源的推荐问题 | `frontend` `ui` |

---

### Milestone 2: 笔记系统
> 保存回复为笔记 + 笔记管理

| # | Issue | Tags |
|---|---|---|
| 2.1 | 笔记面板 UI（笔记卡片列表，可折叠/展开） | `frontend` `ui` |
| 2.2 | "保存到笔记" — 聊天回复一键保存（保留引用链接） | `frontend` `ui` `ux` |
| 2.3 | 笔记编辑（Markdown 编辑 + 预览，复用现有 RichMarkdown 组件） | `frontend` `ui` |
| 2.4 | AI 笔记整合 — 选中多条笔记 → 生成摘要/大纲/学习指南 | `frontend` `llm` |
| 2.5 | 笔记导出（Markdown / Word via `mofa-docx` / PDF） | `frontend` `export` |

---

### Milestone 3: Studio 输出 — 课件生成
> 集成 mofa-skills 生成多格式课件

| # | Issue | Tags |
|---|---|---|
| 3.1 | Studio 面板 UI（输出格式网格 + 生成状态 + 历史 + 下载） | `frontend` `ui` |
| 3.2 | Slide Deck 生成 UI：风格选择（17 种 mofa-slides 风格）+ 预览 + 下载 | `frontend` `ui` `skill-integration` |
| 3.3 | Slide Deck 逐页反馈编辑 + 重新生成 | `frontend` `ui` |
| 3.4 | Quiz 测验 UI：交互式答题 + 即时评分 + 答案解析 | `frontend` `ui` |
| 3.5 | Flashcard 闪卡 UI：翻转卡片 + Spaced Repetition | `frontend` `ui` |
| 3.6 | Mind Map 思维导图 UI（Mermaid 渲染或 react-flow 交互式） | `frontend` `ui` `visualization` |
| 3.7 | Infographic 信息图生成 + 预览（集成 `mofa-infographic`） | `frontend` `ui` `skill-integration` |
| 3.8 | Report 报告生成 + 下载（集成 `mofa-docx` / `mofa-xlsx`） | `frontend` `ui` `skill-integration` |
| 3.9 | Comic 漫画讲解生成 + 预览（集成 `mofa-comic`） | `frontend` `ui` `skill-integration` |

---

### Milestone 4: Audio Overview
> 播客式音频讲解

| # | Issue | Tags |
|---|---|---|
| 4.1 | 播客脚本生成（来源 → LLM 两人对话脚本，Deep Dive/Brief/Critique 格式） | `backend` `llm` |
| 4.2 | TTS 合成：集成 `mofa-fm`（本地 TTS + 声音克隆） | `backend` `skill-integration` |
| 4.3 | 音频播放 UI（复用现有 MediaPlayer，增加章节跳转/倍速） | `frontend` `ui` |
| 4.4 | 播客发布到 mofa.fm（集成 `mofa-fm-api`） | `backend` `skill-integration` |

---

### Milestone 5: Deep Research
> 深度研究集成

| # | Issue | Tags |
|---|---|---|
| 5.1 | Fast Research UI（快速搜索 → 结果列表 → 一键导入为 Source） | `frontend` `ui` |
| 5.2 | Deep Research UI：集成 `mofa-research-2.0`（进度流 → 报告展示） | `frontend` `ui` `skill-integration` |
| 5.3 | Research 报告一键导入为 Source | `frontend` `api` |

---

### Milestone 6: 协作与分享

| # | Issue | Tags |
|---|---|---|
| 6.1 | Notebook 分享（邀请链接 / 权限控制） | `frontend` `backend` `auth` |
| 6.2 | Viewer / Editor 角色权限 | `backend` `auth` |
| 6.3 | 班级/课程空间管理 | `frontend` `backend` |
| 6.4 | 课件模板库 | `frontend` `backend` |

---

### Milestone 7: 图书馆书目管理

| # | Issue | Tags |
|---|---|---|
| 7.1 | 书目元数据模型（ISBN/MARC/分类号/作者/出版社/封面） | `backend` `data-model` |
| 7.2 | 批量书目导入 API（CSV/MARC + 自动匹配元数据） | `backend` `api` |
| 7.3 | 扫描件 OCR 批量处理（集成 `mofa-paddleocr`） | `backend` `skill-integration` |
| 7.4 | 书架浏览 UI（按学科/分类号/年级分类） | `frontend` `ui` |
| 7.5 | ISBN 自动查询书目信息 | `backend` `api` |
| 7.6 | 版权控制（来源不可直接下载，仅 AI 交互） | `backend` `auth` |
| 7.7 | 使用统计面板 | `frontend` `backend` |
| 7.8 | 跨书 Notebook | `frontend` `backend` `rag` |

---

### Milestone 8: 多渠道推送

| # | Issue | Tags |
|---|---|---|
| 8.1 | 课件推送至微信/飞书群 | `backend` `channel` |
| 8.2 | 定时推送（学习提醒 + 闪卡复习） | `backend` `cron` |
| 8.3 | IM 内直接与 Notebook 对话 | `backend` `channel` |

---

## 四、MoFa Skills 集成矩阵

| Notebook 功能 | MoFa Skill | 成熟度 | 集成点 |
|---|---|---|---|
| PDF 提取 | `mofa-pdf` | ★★★★★ | Source 导入 |
| PDF 转换 | `mofa-pdf-convert` | ★★★ | Source 导入 |
| OCR | `mofa-paddleocr` | ★★★ | Source 导入 |
| 网页净化 | `mofa-defuddle` | ★★★ | URL Source |
| AI 图像 PPT | `mofa-slides` | ★★★★★ | Studio |
| 传统 PPT | `mofa-pptx` | ★★★★★ | Studio |
| Word | `mofa-docx` | ★★★★★ | Studio / 导出 |
| Excel | `mofa-xlsx` | ★★★★ | Studio |
| 信息图 | `mofa-infographic` | ★★★★ | Studio |
| 漫画 | `mofa-comic` | ★★★★ | Studio |
| TTS | `mofa-fm` | ★★★★ | Audio |
| 播客 | `mofa-fm-api` | ★★★★ | Audio 发布 |
| 视频 | `mofa-video` | ★★★ | Studio |
| 深度研究 | `mofa-research-2.0` | ★★★★★ | Research |
| 记忆 | `mofa-memory` | ★★★★ | RAG |
| 爬取 | `mofa-firecrawl` | ★★★★ | Research |

---

## 五、技术方案

### 5.1 在 octos-web 基础上扩展

复用现有：`@assistant-ui/react` SSE adapter · `RichMarkdown` 渲染器 · 文件上传 · `MediaPlayer` · 认证系统 · 会话管理 · 主题系统

新增路由：
```
/notebooks                        → Notebook 列表页
/notebooks/:id                    → Notebook 详情（Sources + Chat + Notes + Studio）
/library                          → 图书馆书架浏览
```

### 5.2 Source Grounding 引用

```
上传 → mofa-pdf / mofa-paddleocr / mofa-defuddle 解析 → 分块 → 向量化
提问 → 检索 Top-K → Prompt 注入 → LLM 回复嵌入 [src:chunk_id]
前端 → RichMarkdown 扩展解析引用标记 → 点击跳转来源预览
```

### 5.3 后端 API

```
/api/notebooks                       → CRUD
/api/notebooks/:id/sources           → Source CRUD + 上传
/api/notebooks/:id/chat              → RAG 对话（SSE）
/api/notebooks/:id/notes             → Note CRUD
/api/notebooks/:id/studio/:type      → 课件生成（slides/quiz/flashcards/mindmap/audio/infographic/comic/report/research）
/api/notebooks/:id/share             → 分享
/api/library/books                   → 书目管理
/api/library/catalog                 → 分类浏览
```

---

## 六、里程碑节奏

**MVP（M0→M1）：** 上传文档 → 基于文档对话（带引用）→ Sources 管理
**课件版（+M2→M3）：** 笔记 + PPT/测验/闪卡/信息图/漫画
**图书馆版（+M6→M7）：** 协作分享 + 馆藏批量导入 + 书架浏览
**完整版（+M4→M5→M8）：** 音频播客 + 深度研究 + 多渠道推送
