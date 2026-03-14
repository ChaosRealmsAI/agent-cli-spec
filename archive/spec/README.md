# Agent-Friendly CLI Spec v0.1

> 让命令行工具先满足 agent 的执行契约，再逐步长成完整的 agent-native 生态。

## 核心观点

这套 spec 现在明确拆成两条正交轴：

1. **层级（layer）**：决定“这条规则属于底线、增强，还是生态扩展”
2. **优先级（priority）**：决定“这条规则不满足时有多严重”

优先级仍然保留 `P0 / P1 / P2`，但认证和日常落地优先看分层。

## 三层模型

| Layer | 目标 | 典型内容 |
|------|------|---------|
| `core` | 最低可自动化执行契约 | 默认 JSON、结构化错误、稳定 schema、明确 exit code、stdout/stderr 分离、安全输入校验 |
| `recommended` | 更好用的机器接口 | `--help`/`--brief`、显式 `--human`/`--agent`、类型化 schema、`--dry-run`、更好的 pipe 语义 |
| `ecosystem` | agent-native 扩展生态 | `agent/` 目录、`skills`、`issue`、内联上下文、项目级元信息 |

当前 lint registry 一共 **98** 条规则：
- `core`: 20
- `recommended`: 59
- `ecosystem`: 19

## 认证

- **Agent-Friendly**：`core` 全部通过
- **Agent-Ready**：`core` + `recommended` 全部通过
- **Agent-Native**：`core` + `recommended` + `ecosystem` 全部通过

这比“只看 P0/P1/P2”更适合迁移，因为很多团队会先把 CLI 变成可靠的自动化接口，再决定是否接入 `agent/skills/issue` 这类生态约定。

## 优先级

优先级没有废弃，只是不再承担“分阶段落地”的全部职责：

| Priority | 含义 |
|---------|------|
| `P0` | 不过通常意味着 agent 无法稳定使用 |
| `P1` | 不过通常意味着 agent 能用但体验差、恢复弱 |
| `P2` | 不过通常意味着缺少增强能力或生态配套 |

## Lint 用法

```bash
# 全量检查
agent-cli-lint check mycli --json

# 先看底线
agent-cli-lint check mycli --layer core

# 再看增强层
agent-cli-lint check mycli --layer recommended

# 只看生态扩展
agent-cli-lint check mycli --layer ecosystem

# 仍然支持优先级和维度过滤
agent-cli-lint check mycli --priority p0
agent-cli-lint check mycli --dimension 03
agent-cli-lint check mycli --rule O1
```

## 设计取向

- `core` 是规范真正的硬约束，目标是“CLI 像稳定 API 一样可调用”
- `recommended` 是高质量接口习惯，目标是“agent 少猜、少试错、少浪费 token”
- `ecosystem` 是协作闭环，目标是“工具会自描述、会反馈、会扩展”

## 生态定位

```text
cli-toolkit  -> scaffold
spec         -> contract
agent-cli-lint -> verification
issue        -> feedback loop
```

## 参考来源

- [You Need to Rewrite Your CLI for AI Agents — Justin Poehnelt](https://justin.poehnelt.com/posts/rewrite-your-cli-for-ai-agents/)
- [Google Workspace CLI](https://github.com/googleworkspace/cli)
- [CLI-Anything](https://github.com/HKUDS/CLI-Anything)
- [AGENTS.md Specification](https://agents.md/)
- [OWASP AI Agent Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/AI_Agent_Security_Cheat_Sheet.html)
- [OWASP Top 10 for Agentic Applications 2026](https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/)
