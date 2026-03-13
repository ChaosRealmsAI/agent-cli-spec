---
name: workflow
description: Recommended workflow for running agent-cli-lint
---

1. 首次使用前运行 agent-cli-lint --help 了解所有命令
2. agent-cli-lint check <cli> --json 检查目标 CLI
3. agent-cli-lint check <cli> --priority p0 只检查 P0 生死线
4. agent-cli-lint check <cli> --dimension 01 只检查某个维度
5. agent-cli-lint snapshot <cli> 保存快照（用于稳定性检查）
6. agent-cli-lint diff <cli> 对比当前 vs 快照
7. agent-cli-lint ai-prompts <cli> 生成 AI 辅助检查 prompt
8. 遇到问题用 agent-cli-lint issue create 提交反馈
