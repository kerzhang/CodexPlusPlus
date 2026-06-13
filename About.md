# Codex++ 项目导读

本文档整理自开发上手过程中的问答，面向熟悉 **React / Python**、但对 **Rust / Tauri / 桌面端** 相对陌生的读者。用于快速建立对 Codex++ 的整体认知。

---

## 1. Codex++ 是什么

Codex++ 是面向 **Codex App** 的外部增强工具。它**不修改** Codex 原始安装文件，而是：

1. 用外部启动器带调试参数启动 Codex
2. 通过 **CDP（Chrome DevTools Protocol）** 向 Codex 页面注入增强脚本
3. 用图形化管理工具做配置、诊断、更新、中转注入等

安装后有两个入口（与官方 Release 一致）：

| 名称 | 项目路径 | 作用 |
|------|----------|------|
| **Codex++** | `apps/codex-plus-launcher/` | 静默启动器，无管理界面，负责启动 Codex 并注入增强功能 |
| **Codex++ 管理工具** | `apps/codex-plus-manager/` | Tauri 图形控制面板 |

---

## 2. 项目结构

```
CodexPlusPlus/
├── apps/
│   ├── codex-plus-launcher/       # 静默启动器（纯 Rust）
│   └── codex-plus-manager/        # 管理工具
│       ├── src/                   # React 前端
│       └── src-tauri/             # Tauri Rust 后端
├── crates/
│   ├── codex-plus-core/           # 核心业务逻辑（启动、注入、配置、桥接等）
│   └── codex-plus-data/           # 数据层（SQLite、导出、Provider 同步）
├── assets/inject/
│   └── renderer-inject.js         # 注入到 Codex 渲染端的增强脚本
├── scripts/installer/
│   ├── windows/CodexPlusPlus.nsi  # Windows 安装包
│   └── macos/package-dmg.sh       # macOS DMG 打包
├── Cargo.toml                     # Rust workspace（类似 monorepo 根配置）
└── README.md                      # 官方使用说明
```

### 架构关系（简图）

```
用户
 ├── Codex++ 启动器 ──→ 启动 Codex + CDP 注入 renderer-inject.js
 └── 管理工具 (React + Tauri)
         │
         ├── invoke() 调用 Rust 命令
         └── 读写 ~/.codex/config.toml 等配置

注入脚本 (在 Codex 页面内)
 └── HTTP 请求本地 Bridge (127.0.0.1:57321) ──→ codex-plus-core
```

---

## 3. 技术栈速查（对照 React / Python）

本项目**没有 Python 后端**，主体是 **Rust + React/TypeScript**。

| 部件 | 技术 | 你若熟悉… |
|------|------|-----------|
| 管理工具 UI | React 19 + TypeScript + Vite + Tailwind | 标准 React SPA |
| 管理工具壳 | Rust + Tauri 2 | 类似 Electron 主进程，但更轻 |
| 启动器 | Rust + Tokio | 类似无 UI 的 asyncio 守护进程 |
| 核心业务 | `codex-plus-core`（Rust crate） | 类似 Python 的 `services/` 包 |
| 数据层 | Rust + SQLite（rusqlite） | 类似 `sqlite3` 模块 |
| 包管理（Rust） | Cargo | 类似 `uv` / `poetry` |
| 包管理（前端） | npm | 常规 Node 生态 |

### 常见 Rust 库类比

| Rust | Python 类比 | 用途 |
|------|-------------|------|
| `serde` / `serde_json` | pydantic + json | 序列化 |
| `reqwest` | httpx / requests | HTTP 客户端 |
| `tokio` | asyncio | 异步运行时 |
| `anyhow` | Exception 封装 | 错误处理 |
| `toml` | 读写 TOML 配置 | Codex 配置 |

### Tauri 前后端通信

前端不用 `fetch('/api/...')`，而是：

```typescript
import { invoke } from '@tauri-apps/api/core';
await invoke('load_settings');
```

Rust 侧注册对应命令（见 `apps/codex-plus-manager/src-tauri/src/lib.rs`）。

---

## 4. 「注入」是什么意思（不是黑客手段）

项目里多次出现「注入」，在浏览器/Chromium 技术圈指 **Script Injection（脚本注入）**，与 SQL 注入等攻击无关。

### Codex++ 实际做的事

- 启动 Codex 时加上 `--remote-debugging-port=9229`
- 通过 CDP 连接 Codex 页面，执行 `assets/inject/renderer-inject.js`
- **不改** `app.asar`，**不写** DLL，**不篡改** Codex 安装目录

README 明确写道：外部 CDP 注入，不改 Codex 原始文件。

### 类比

| 项目说法 | 熟悉类比 |
|----------|----------|
| CDP 注入 | 浏览器 F12 控制台执行 JS |
| `renderer-inject.js` | Tampermonkey / 油猴脚本 |
| 中转注入 | 改写本地 `config.toml` |
| 用户脚本注入 | 自定义油猴脚本 |

### 注入脚本典型能力

- 会话列表增加「删除」按钮
- API Key 模式下解锁插件入口
- Markdown 导出、项目移动、Timeline 等
- 「打开管理工具」菜单
- 解锁自定义模型列表（见下文）

---

## 5. 核心功能：使用第三方大模型（如 DeepSeek）

这是 Codex++ 的**核心特色**之一。并非把 DeepSeek「塞进」Codex 二进制，而是通过 **配置 + 协议适配 + UI 解锁** 三层配合。

### 5.1 整体流程

```
管理工具填写：模型名、Base URL、API Key、协议类型
        ↓
应用「中转注入」→ 写入 ~/.codex/config.toml 和 auth.json
        ↓
从 Codex++ 启动 Codex
        ↓
（若需要）本地协议代理 :57321 做 API 格式转换
        ↓
注入脚本把 deepseek-v4-pro 等模型补进 Codex 模型选择器
        ↓
Codex 按 config 向第三方 API 发请求
```

### 5.2 配置层：改写 Codex 标准配置

Codex App 本身支持通过 `~/.codex/config.toml` 配置自定义 API 供应商。Codex++ 在管理工具「中转注入」页面收集信息后，由 Rust（`crates/codex-plus-core/src/relay_config.rs`）写入：

**config.toml（示意）**

```toml
model = "deepseek-v4-pro"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://api.deepseek.com/v1"
experimental_bearer_token = "sk-..."
```

**auth.json（纯 API 模式）**

```json
{
  "OPENAI_API_KEY": "sk-..."
}
```

关键配置字段（`RelayProfile`）：

| 字段 | 含义 |
|------|------|
| `model` | 模型名，如 `deepseek-v4-pro` |
| `baseUrl` | API 地址 |
| `apiKey` | 密钥 |
| `protocol` | `responses` 或 `chatCompletions` |
| `relayMode` | 认证模式（见下表） |
| `modelList` | 额外模型列表（换行或逗号分隔） |

### 5.3 三种认证模式（relayMode）

| 模式 | 说明 |
|------|------|
| `official` | 官方 ChatGPT 登录，清空中转 |
| `mixedApi` | 保留 ChatGPT 登录，同时在 config 混入 API Key |
| `pureApi` | 纯 API Key，写在 `auth.json` |

### 5.4 协议层：Responses vs Chat Completions

Codex 内部主要使用 **OpenAI Responses API**；许多厂商（含 DeepSeek）提供 **Chat Completions API**。

| 协议 | 行为 |
|------|------|
| **Responses** | `base_url` 直接指向兼容 Responses 的上游 |
| **Chat Completions** | `config.toml` 的 `base_url` 指向本地代理 `http://127.0.0.1:57321/v1`，由 `protocol_proxy.rs` 做双向格式转换 |

启动器在需要时于 `57321` 端口启动 HTTP 服务（`launcher.rs` 中的 helper 路由）。

### 5.5 UI 层：模型白名单解锁

即使 `config.toml` 正确，Codex 界面可能有模型白名单。`renderer-inject.js` 会：

1. 请求本地 `/codex-model-catalog` 获取模型列表
2. 拦截 Codex 内部模型列表 API 响应
3. 将 `model`、`modelList` 中的自定义模型补进列表

相关 Rust 逻辑见 `crates/codex-plus-core/src/model_catalog.rs`。

### 5.6 用户操作步骤（简）

1. 打开 **Codex++ 管理工具** →「中转注入」
2. 新建供应商：Base URL、API Key、模型名（如 `deepseek-v4-pro`）、协议类型
3. 点击「应用中转注入」
4. 从 **Codex++**（非原版 Codex）启动
5. 在 Codex 中选择对应模型使用

### 5.7 与 CC Switch 联动

若安装了 CC Switch，可从其 SQLite 数据库导入已有供应商配置（`ccs_import.rs`），无需重复手填。

---

## 6. codex-plus-core 核心模块

两个 App 共用的「业务大脑」：

| 模块 | 文件 | 职责 |
|------|------|------|
| `launcher` | `launcher.rs` | 启动 Codex、注入流程 |
| `cdp` | `cdp.rs` | CDP 连接 |
| `bridge` / `routes` | `bridge.rs`, `routes.rs` | 本地 HTTP 桥接 |
| `settings` | `settings.rs` | 用户设置 |
| `relay_config` | `relay_config.rs` | 中转 / 供应商配置 |
| `protocol_proxy` | `protocol_proxy.rs` | Chat ↔ Responses 协议转换 |
| `model_catalog` | `model_catalog.rs` | 模型列表 |
| `config_coordinator` | `config_coordinator.rs` | 与 CC Switch 配置协调 |
| `update` | `update.rs` | GitHub Release 更新 |

---

## 7. 数据流三条主路径

**路径 1：管理工具 UI → Rust**

```
React invoke('save_settings') → Tauri commands → settings → 写本地 JSON/TOML
```

**路径 2：启动器 → Codex 页面**

```
launcher 启动 Codex → cdp 注入 renderer-inject.js → 页面增强
```

**路径 3：Codex 页面 → Rust 后端**

```
注入脚本 fetch('http://127.0.0.1:57321/bridge/...') → routes → core/data → JSON
```

---

## 8. 从源码构建

### 8.1 前置要求

- Rust 1.85+（`rustc --version`）
- Node.js + npm（管理工具前端）

### 8.2 完整构建步骤（macOS 示例）

```bash
# 1. 构建前端（必须先做，否则 Tauri 编译失败）
cd apps/codex-plus-manager
npm install
npm run vite:build

# 2. 编译 Rust 二进制
cd ../..
cargo build --release -p codex-plus-launcher -p codex-plus-manager

# 3. 打 macOS 安装包（可选）
bash scripts/installer/macos/package-dmg.sh 1.2.5 arm64
```

### 8.3 产物位置

| 产物 | 路径 |
|------|------|
| 启动器二进制 | `target/release/codex-plus-plus` |
| 管理工具二进制 | `target/release/codex-plus-plus-manager` |
| Codex++.app | `dist/macos/stage/Codex++.app` |
| 管理工具 .app | `dist/macos/stage/Codex++ 管理工具.app` |
| DMG | `dist/macos/CodexPlusPlus-*-macos-*.dmg` |

### 8.4 常见构建坑

1. **直接 `cargo build` 失败**：提示 `frontendDist "../dist"` 不存在 → 先 `npm run vite:build`
2. **CONTRIBUTING.md 不完整**：只写了 `cargo build`，未提前端步骤；以 README「开发」章节为准
3. **macOS 本地构建未签名**：首次打开若提示「已损坏」：

   ```bash
   sudo xattr -rd com.apple.quarantine /Applications/Codex++.app
   sudo xattr -rd com.apple.quarantine "/Applications/Codex++ 管理工具.app"
   ```

### 8.5 日常开发：如何启动两个程序

日常开发**不要**直接跑 `target/release/` 下的成品二进制（无热更新，且易与已安装 App 冲突）。推荐方式如下。

#### 管理工具（改 React UI / 调 Tauri 命令）

```bash
cd apps/codex-plus-manager
npm install          # 首次需要
npm run dev          # Tauri + Vite 热更新
```

- 会打开 **Codex++ 管理工具** 调试窗口
- 改 `src/App.tsx` 等前端文件会热更新
- 改 `src-tauri/src/commands.rs` 等 Rust 文件会触发 Tauri 重新编译
- Vite 开发服务器：`http://127.0.0.1:1420`（见 `tauri.conf.json`）

#### 启动器（测 Codex 启动 + CDP 注入）

在**另一个终端**，于仓库根目录：

```bash
# 调试构建（编译快，适合频繁改 Rust）
cargo run -p codex-plus-launcher

# 或 release 构建（更接近成品行为）
cargo run --release -p codex-plus-launcher
```

等价于运行 `target/debug/codex-plus-plus` 或 `target/release/codex-plus-plus`。

#### 典型开发流程

```
1. npm run dev          → 打开管理工具，改配置 / 中转注入 / 看日志
2. cargo run -p ...     → 启动 Codex++，验证注入与启动逻辑
3. 或在管理工具 UI 里点「启动 Codex++」（走同一套 launcher 逻辑）
```

| 你想做的事 | 推荐命令 |
|------------|----------|
| 改管理界面、设置页 | `npm run dev` |
| 改启动、注入、中转核心逻辑 | `cargo run -p codex-plus-launcher` + 必要时 `npm run dev` |
| 验证打包后的行为 | 先 `npm run vite:build`，再 `cargo build --release`，最后跑 release 二进制 |

> **注意**：`npm run dev` 与 release 二进制**不是**同一种启动方式；前者是开发模式，后者是成品模式。

### 8.6 故障排查：管理工具「启动了但什么都没出现」

两个程序都有**单实例保护**。若检测到已有实例占用守卫端口，新进程会**静默退出**（exit 0，无窗口、无终端报错）。

#### 守卫端口（`crates/codex-plus-core/src/ports.rs`）

| 端口 | 用途 |
|------|------|
| **57319** | 管理工具单实例（`codex-plus-plus-manager`） |
| **57320** | 启动器单实例（`codex-plus-plus`） |
| **57321** | 启动器 Helper / 协议代理（运行时） |

管理工具在 `lib.rs` 中若拿不到 57319 守卫，会直接 `return`：

```rust
let Some(_guard) = acquire_single_instance_guard() else {
    return;  // 静默退出
};
```

#### 常见症状

- 执行 `./target/release/codex-plus-plus-manager` 或 `npm run dev` 后**没有任何窗口**
- 进程瞬间结束，终端无错误输出
- 日志里出现 `manager.already_running`

#### 诊断步骤

```bash
# 1. 看日志（最直接）
tail -20 ~/.codex-session-delete/codex-plus.log
# 若见 {"event":"manager.already_running","detail":{"guard_port":57319}} → 确认是单实例冲突

# 2. 看谁占着 57319
netstat -vanp tcp | grep 57319
pgrep -fl 'codex-plus-plus-manager|CodexPlusPlusMan'

# 3. 测试端口是否仍被占用
python3 -c "import socket; s=socket.create_connection(('127.0.0.1',57319),1); print('57319 可连接'); s.close()"
```

#### 僵死进程（`?E` / `CodexPlusPlusMan`）

有时管理工具窗口已关，但进程处于退出中（`ps` 显示 `?E`），**仍占用 57319**。此时：

- **新启动的管理工具**：起不来（静默退出）
- **已僵死的旧实例**：不可靠，界面可能已消失
- **启动器** `codex-plus-plus`：通常**不受影响**（用 57320），仍可启动 Codex；只是**没有可用的管理面板**

#### 处理办法（由轻到重）

```bash
# 1. 活动监视器里退出「Codex++ 管理工具」

# 2. 按进程名结束
pkill -9 -f CodexPlusPlusMan
pkill -9 -f codex-plus-plus-manager

# 3. 确认 57319 已释放
netstat -vanp tcp | grep 57319
# 不应再看到 LISTEN 行

# 4. 若端口仍被占用 → 注销重新登录，或重启 Mac
```

释放端口后重新启动：

```bash
cd apps/codex-plus-manager && npm run dev
```

日志中应出现 `manager.start`，而不是 `manager.already_running`。

#### 启动器单实例冲突（57320）

若 `cargo run -p codex-plus-launcher` 也异常，用同样方式检查 **57320**：

```bash
tail -20 ~/.codex-session-delete/codex-plus.log
netstat -vanp tcp | grep 57320
pkill -9 -f codex-plus-plus
```

#### Helper 端口冲突（57321）— `Address already in use`

在管理工具里点「启动 Codex++」，或同时开了 `cargo run -p codex-plus-launcher` 与 `npm run dev` 时，终端可能出现：

```text
Error: failed to bind helper runtime on 127.0.0.1:57321
Caused by:
    Address already in use (os error 48)
```

**原因**：**57321** 是启动器的 Helper / Bridge / 协议代理端口，**全局只能有一个实例监听**。常见情况是：

- 终端 A 已运行 `cargo run -p codex-plus-launcher`（占用 57321）
- 终端 B 的 `npm run dev` 管理工具里又点了「启动 Codex++」，第二个启动器无法再绑定 57321

**诊断**：

```bash
netstat -vanp tcp | grep 57321
pgrep -fl codex-plus-plus
```

若看到 `codex-plus-plus` 在 `LISTEN` 57321，说明已有启动器在跑。

**处理**：

```bash
# 结束多余的启动器（保留管理工具 dev 进程）
pkill -f 'target/debug/codex-plus-plus$'
pkill -f 'target/release/codex-plus-plus$'
# 或更直接（会同时结束管理工具若路径匹配，注意只用 launcher 那条）：
kill $(pgrep -f 'target/.*/codex-plus-plus$')

# 确认 57321 已释放
netstat -vanp tcp | grep 57321
```

**日常开发建议（二选一，不要双开）**：

| 方式 | 适用场景 |
|------|----------|
| 仅 `npm run dev`，在 UI 里启动 Codex | 改管理界面 + 顺带测启动 |
| 仅 `cargo run -p codex-plus-launcher` | 专注改启动器 / 注入逻辑，不需要管理 UI |

若两种都要：先结束已有 `codex-plus-plus`，再在管理工具里启动。

---


## 9. Git 与构建产物

以下目录已在 `.gitignore` 中，**不会被 Git 跟踪**：

```
dist/
target/
node_modules/
```

本地 `npm install`、`cargo build`、打 DMG 不会产生大量「源码修改」；若 IDE 显示海量未跟踪文件，通常是 `node_modules/` 未忽略（已修复）。

构建相关变更不应提交；仅源码与配置改动才需要 commit。

---

## 10. 建议阅读顺序

若你熟悉 React，建议按此顺序读代码：

1. `apps/codex-plus-manager/src/App.tsx` — UI 与 `invoke` 命令
2. `apps/codex-plus-manager/src-tauri/src/lib.rs` — 注册的 Tauri 命令列表
3. `apps/codex-plus-manager/src-tauri/src/commands.rs` — 各命令实现
4. `crates/codex-plus-core/src/launcher.rs` — 启动 + 注入主流程
5. `crates/codex-plus-core/src/relay_config.rs` — 第三方模型配置
6. `assets/inject/renderer-inject.js` — Codex 页面内实际增强

---

## 11. 与 Python 全栈项目的差异（一句话）

| 你熟悉的 | 本项目 |
|----------|--------|
| FastAPI / Flask | Rust（Tauri commands + core crate） |
| 浏览器访问 `localhost:8000` | Tauri `invoke` 或本地 Bridge HTTP |
| pip / uv | cargo |
| 无桌面壳 / Electron | Tauri 2 |
| 服务端渲染 | 纯客户端 React + Vite |

一切都是**本地桌面应用**：Rust 直接读写文件、启进程、连 CDP，没有传统意义上的远程 Web 后端。

---

## 12. 相关文档

- [README.md](README.md) — 安装、使用、中转注入操作说明
- [CONTRIBUTING.md](CONTRIBUTING.md) — 贡献指南（构建步骤建议以 README 为准）
- [README_EN.md](README_EN.md) — 英文版说明

---

*本文档为学习笔记性质，随项目演进可能需要更新。以源码与 README 为准。*
