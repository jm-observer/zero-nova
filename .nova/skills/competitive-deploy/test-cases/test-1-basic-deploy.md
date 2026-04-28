# 测试用例 1：基础多竞品部署

## 用户输入（触发 Prompt）

```
帮我部署以下方案中的 3 个竞品：

### LLM 服务部署方案

**竞品 A - ChatGPT Clone**
- 部署方式：Docker
- 镜像：ghcr.io/competitor-a/chatgpt:latest
- 端口：8001
- 启动命令：docker run -d -p 8001:8000 --name chatgpt-clone ghcr.io/competitor-a/chatgpt:latest
- 健康检查：curl http://localhost:8001/health
- HF 模型：meta-llama/Llama-2-7b-chat-hf

**竞品 B - 通义千问服务**
- 部署方式：源码部署
- 仓库：git@github.com:competitor-b/qwen-service.git
- 分支：main
- 端口：8002
- 启动命令：python main.py --port 8002
- 健康检查：curl http://localhost:8002/status
- 依赖：pip install -r requirements.txt

**竞品 C - 文心一言服务**
- 部署方式：Docker
- 镜像：registry.example.com/wenxin:latest
- 端口：8003
- 启动命令：docker run -d -p 8003:8000 --name wenxin registry.example.com/wenxin:latest
- 健康检查：curl http://localhost:8003/health
- HF 模型：BAAI/bge-m3
```

## 预期行为

1. 提取 3 个竞品信息
2. 检查命令可用性（docker, git, python, pip, curl, huggingface-cli）
3. 按顺序部署：
   - A：拉取 HF 模型 → 启动 Docker 容器
   - B：git clone → 安装依赖 → 启动服务
   - C：启动 Docker 容器
4. 验证每个竞品的健康检查
5. 生成部署报告

## 验证点
- [ ] 正确识别 3 个竞品
- [ ] 输出命令检查计划
- [ ] 每个竞品都有对应的部署步骤
- [ ] 生成包含成功状态的部署报告