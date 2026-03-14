---
name: getting-started
description: Quick start commands for running agent-cli-lint
---

# Getting Started

## 快速上手

```bash
# 检查一个 CLI 的完整合规性
agent-cli-lint check <cli> --json

# 只检查 P0 生死线（10 条）
agent-cli-lint check <cli> --priority p0 --json

# 只检查某个维度
agent-cli-lint check <cli> --dimension 03 --json

# 检查单条规则
agent-cli-lint check <cli> --rule O1 --json

# 保存快照（用于跨版本稳定性检查）
agent-cli-lint snapshot <cli>

# 对比当前 vs 快照
agent-cli-lint diff <cli>

# 生成 AI 辅助检查 prompt
agent-cli-lint ai-prompts <cli> --json
```

## 管道示例

```bash
# 只看失败的规则
agent-cli-lint check <cli> --json | jq '.dimensions[].rules[] | select(.status == "fail")'

# 提取认证等级
agent-cli-lint check <cli> --json | jq '.certification'

# 比较两个实现
diff <(agent-cli-lint check impl-a --json | jq '.summary') \
     <(agent-cli-lint check impl-b --json | jq '.summary')
```
