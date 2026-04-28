---
name: competitive-deploy
description: 解析部署方案文档，逐个竞品执行环境部署与测试。当用户给一个部署/测试方案，方案里面包含一个或多个竞品需要部署（通常是 docker 环境、git 仓库、hf 模型下载、python 脚本等），该 skill 负责提取每个竞品的部署步骤，使用 bash 命令逐一执行，并验证结果。适用于用户说"部署方案"、"部署测试"、"竞品部署"、"部署计划"、"部署流程"、"帮我把这些竞品部署起来"等场景。
---

# Competitive Deploy Skill

## 核心职责

解析用户提供的部署方案文档，提取其中包含的所有竞品及其部署要求，然后**逐个竞品**执行完整的部署和测试流程。

## 触发条件

当用户输入包含以下要素时激活：
- 提到"部署"、"测试"、"竞品"相关的方案/计划/文档
- 方案中包含多个需要部署的竞品
- 涉及 Docker、Git、HuggingFace 模型、Python 脚本等部署组件

## 工作流程

### 阶段一：方案解析

1. **读取方案文档**
   - 如果用户提供了文件路径，直接读取
   - 如果用户在消息中粘贴了方案内容，解析文本
   - 如果方案内容较少，使用 web_fetch 或 web_search 获取完整文档

2. **提取竞品列表**
   - 识别方案中每个竞品的名称
   - 提取每个竞品的关键信息：
     - 部署类型（Docker / 源码 / 混合）
     - 依赖项（Docker 镜像、Git 仓库、HF 模型）
     - 启动命令
     - 测试方式
     - 端口号（如适用）

3. **生成部署计划**
   - 输出竞品清单
   - 列出每个竞品的部署步骤概要
   - 确认执行顺序（如有依赖关系）

### 阶段二：环境准备

1. **创建隔离工作目录**
   ```bash
   mkdir -p deploy-workspace/{competitor-name}
   ```

2. **检查系统环境**
   - Docker 是否可用：`docker --version`
   - Git 是否可用：`git --version`
   - Python 版本：`python3 --version`
   - 磁盘空间：`df -h .`
   - 网络连通性（对 HF 下载重要）

3. **共享依赖安装**
   - 如果方案中有共享的 Python 依赖（requirements.txt 等），先安装到虚拟环境

### 阶段二（补充）：竞品前置命令检查

**在开始逐个部署前，对每个竞品执行命令存在性检查。**

#### 1. 命令分类定义

| 类型 | 说明 | 缺失时行为 |
|------|------|-----------|
| **关键命令** | 竞品部署必须使用的命令（如 docker、git、huggingface-cli） | **跳过该竞品** |
| **辅助命令** | 增强部署体验但非必需的命令（如 jq、npm、curl） | 记录警告，**不跳过** |
| **测试命令** | 仅用于验证阶段的命令（如 pytest、npm test） | 跳过测试，**不跳过部署** |

#### 2. 命令检查逻辑

```bash
# 定义命令分类
declare -A CRITICAL_COMMANDS
declare -A OPTIONAL_COMMANDS
declare -A TEST_COMMANDS

# 关键命令（缺失则跳过）
CRITICAL_COMMANDS["docker"]="Docker 部署必需"
CRITICAL_COMMANDS["git"]="Git 仓库拉取必需"
CRITICAL_COMMANDS["huggingface-cli"]="HF 模型下载必需"
CRITICAL_COMMANDS["python"]="Python 环境必需"
CRITICAL_COMMANDS["pip"]="Python 依赖安装必需"

# 辅助命令（缺失仅警告）
OPTIONAL_COMMANDS["jq"]="JSON 解析辅助"
OPTIONAL_COMMANDS["curl"]="HTTP 请求辅助"
OPTIONAL_COMMANDS["npm"]="Node.js 依赖管理"

# 测试命令（缺失则跳过测试）
TEST_COMMANDS["pytest"]="Python 单元测试"
TEST_COMMANDS["npm"]="Node.js 测试运行"

# 批量检查所有命令
echo "=== 命令检查开始 ==="
MISSING_CRITICAL=()
MISSING_OPTIONAL=()

for cmd in "${!CRITICAL_COMMANDS[@]}"; do
    if command -v $cmd > /dev/null 2>&1; then
        echo "✓ $cmd 可用 (${CRITICAL_COMMANDS[$cmd]})"
    else
        echo "✗ $cmd 缺失 (${CRITICAL_COMMANDS[$cmd]})"
        MISSING_CRITICAL+=("$cmd")
    fi
done

for cmd in "${!OPTIONAL_COMMANDS[@]}"; do
    if command -v $cmd > /dev/null 2>&1; then
        echo "✓ $cmd 可用 (${OPTIONAL_COMMANDS[$cmd]})"
    else
        echo "⚠ $cmd 缺失 (${OPTIONAL_COMMANDS[$cmd]})"
        MISSING_OPTIONAL+=("$cmd")
    fi
done

echo "=== 检查完成 ==="
echo "关键命令缺失: ${MISSING_CRITICAL[*]:-无}"
echo "辅助命令缺失: ${MISSING_OPTIONAL[*]:-无}"
```

#### 3. 竞品级别命令检查

对每个竞品单独检查其特定命令：

```bash
# 检查某个竞品所需的命令
check_competitor_commands() {
    local competitor=$1
    local required_commands=("${@:2}")  # 从第二个参数开始
    
    local missing=()
    for cmd in "${required_commands[@]}"; do
        if ! command -v $cmd > /dev/null 2>&1; then
            missing+=("$cmd")
        fi
    done
    
    if [ ${#missing[@]} -gt 0 ]; then
        echo "SKIP: $competitor - 缺少关键命令: ${missing[*]}"
        return 1
    else
        echo "OK: $competitor - 所有命令可用"
        return 0
    fi
}

# 使用示例
check_competitor_commands "competitor-a" docker git huggingface-cli python
check_competitor_commands "competitor-b" docker npm curl
```

#### 4. 版本检查（可选增强）

如果方案中指定了最低版本要求：

```bash
# 检查 docker 版本是否 >= 20.10
check_docker_version() {
    local version=$(docker --version | grep -oP '\d+\.\d+' | head -1)
    local major=$(echo $version | cut -d. -f1)
    local minor=$(echo $version | cut -d. -f2)
    
    if [ "$major" -gt 20 ] || ([ "$major" -eq 20 ] && [ "$minor" -ge 10 ]); then
        echo "OK: Docker 版本 $version 满足要求 (>= 20.10)"
        return 0
    else
        echo "SKIP: Docker 版本 $version 不满足要求 (>= 20.10)"
        return 1
    fi
}
```

#### 5. 检查规则汇总

| 场景 | 处理方式 |
|------|---------|
| 竞品需要 `docker` 但系统无 `docker` | **跳过该竞品**，记录原因 |
| 竞品需要 `huggingface-cli` 但系统只有 `hf` 别名 | **不跳过**，尝试使用替代命令 |
| 竞品需要 `npm` 但系统无 `npm` | 记录警告，如果部署不需要 Node.js 则继续 |
| 所有竞品都因同一命令缺失而跳过 | 输出汇总提示，建议用户先安装缺失命令 |

#### 6. 记录检查结果

在部署计划阶段输出：

```
=== 竞品命令检查计划 ===
| 竞品 | 所需命令 | 检查结果 | 是否跳过 |
|------|---------|---------|---------|
| A | docker, git, huggingface-cli | ✓ 全部可用 | 否 |
| B | docker, npm | ✗ npm 缺失 | 否（仅警告）|
| C | podman, git | ✗ podman 缺失 | 是 |

总计: 3 个竞品，2 个可部署，1 个跳过
```

### 阶段三：逐个竞品部署

对每个竞品按顺序执行：

#### A. 获取代码

```bash
# Git 仓库拉取
git clone <repo-url> <competitor-name>/
cd <competitor-name>/

# 或下载特定分支/tag
git checkout <branch-or-tag>
```

#### B. 下载模型/资源

```bash
# HuggingFace 模型下载
huggingface-cli download <model-id> --local-dir <competitor-name>/models/

# 或 Python API 下载
python -c "from huggingface_hub import snapshot_download; snapshot_download('<model-id>', local_dir='<path>')"
```

#### C. 部署 Docker（如适用）

```bash
# 构建镜像
docker build -t <competitor-name>:latest <competitor-name>/

# 或拉取已有镜像
docker pull <image-url>

# 启动容器（根据端口映射）
docker run -d -p <host-port>:<container-port> --name <competitor-name> <image>
```

#### D. 安装依赖 & 启动

```bash
# Python 依赖
pip install -r requirements.txt

# 启动服务
python main.py &
# 或
./start.sh
```

### 阶段四：验证与测试

对每个已部署的竞品执行验证：

1. **服务可访问性检查**
   ```bash
   # HTTP 健康检查
   curl -s http://localhost:<port>/health | jq '.'
   
   # 或等待服务就绪
   while ! curl -s http://localhost:<port>/health > /dev/null; do sleep 2; done
   ```

2. **功能测试**
   - 执行方案中定义的基础测试命令
   - 运行 smoke test / unit test
   - 验证模型加载是否正常

3. **资源监控**
   ```bash
   # 检查容器状态
   docker ps --filter "name=<competitor-name>"
   
   # 检查进程
   ps aux | grep <process-name>
   ```

4. **记录结果**
   - 部署成功/失败
   - 耗时
   - 端口占用
   - 错误信息（如有）

### 阶段五：生成部署报告

输出结构化部署报告，包含成功、失败和跳过的竞品：

```markdown
## 部署报告

**总计**: 3 个竞品 | 2 个成功 | 0 个失败 | 1 个跳过

| 竞品 | 状态 | 端口 | 耗时 | 备注 |
|------|------|------|------|------|
| A | ✅ 成功 | 8001 | 3m | - |
| B | ✅ 成功 | 8002 | 5m | HF 模型下载较慢 |
| C | ⏭️ 跳过 | - | - | 缺少关键命令: podman |

### 跳过的竞品详情

#### ⏭️ C - 命令缺失跳过
- **缺失命令**: `podman` (关键命令)
- **建议**: 安装 podman 或改用 docker 部署
- **跳过命令检查**:
  ```
  ✗ podman 缺失 (Docker 替代部署必需)
  ✓ git 可用 (Git 仓库拉取必需)
  ```

### 命令检查汇总

| 命令 | 状态 | 影响范围 |
|------|------|---------|
| docker | ✓ 可用 | - |
| git | ✓ 可用 | - |
| huggingface-cli | ✓ 可用 | - |
| python | ✓ 可用 | - |
| pip | ✓ 可用 | - |
| podman | ✗ 缺失 | 竞品 C |
| jq | ⚠ 缺失 | 健康检查降级 |

### 详细日志
...
```

**报告生成规则：**
1. 使用不同的状态符号区分成功、失败、跳过
2. 跳过竞品必须注明**跳过的具体原因**和**缺失的命令**
3. 在报告末尾附加**命令检查汇总**，方便用户一次性了解所有命令状态
4. 对每个跳过的竞品给出**修复建议**

## 错误处理

- **网络问题**：重试 3 次，失败后记录并跳过当前步骤
- **端口冲突**：自动尝试下一个可用端口
- **磁盘空间不足**：清理临时文件后重试
- **模型下载失败**：使用 `huggingface-cli download --resume-download` 断点续传
- **部分失败**：继续部署其他竞品，最后汇总报告

## 安全约束

- 不盲目删除文件或容器，操作前确认
- 不执行未经确认的 `docker rm -f` 或 `docker rmi -f`
- 清理时使用带确认的工作目录而非全局路径

## 输出格式

每次部署完成后输出：
1. 竞品清单概览
2. 每个竞品的详细部署状态
3. 所有测试结果的汇总
4. 后续操作建议（如需要手动介入的步骤）
