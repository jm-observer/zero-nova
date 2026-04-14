# Claw 插件系统使用指南

## 1. 插件体系概览

Claw 将 **插件（Plugin）** 视为独立的可执行程序或脚本，它们在运行时被 `claw` 核心二进制动态发现并以子命令的形式挂载到主 CLI 中。

### 1.1 插件种类
| 类别 | 说明 | 是否随二进制一起发行 | 默认启用方式 |
|------|------|----------------------|--------------|
| **Builtin** | 编译进 `claw` 本体的功能块，代码在 `rust/crates/plugins/src/builtin.rs`（或类似文件）里实现。它们在任何环境下必然可用，无法被卸载，只能被禁用（如果实现了 `defaultEnabled` 为 `false`）。 | 是 | `defaultEnabled` 决定，通常 **true**。 |
| **Bundled** | 随源码仓库一起提供，位于项目根目录的 `plugins/bundled/` 目录下。安装时会自动复制到用户的插件根目录 (`<install_root>/installed/`) 并写入注册表 `installed.json`。用户可以通过 `claw plugins enable/disable` 来切换。 | 否（随源码一起提供） | `plugin.json` 中的 `defaultEnabled` 决定，若未指定则默认为 `false`。 |
| **External** | 由用户自行下载或 `git clone` 的插件。它们存放在用户配置的 **install_root**（默认 `~/.claw/plugins/installed/`）下，注册信息保存在 `installed.json`。可以随时通过 `install`、`update`、`uninstall` 管理。 | 否 | 同上，默认 **false**，但安装后会自动标记为 `enabled`（除非 `--disable` 参数）。 |

### 1.2 插件加载顺序
1. **Builtin** 插件实例化（永远第一步）。
2. **Bundled** 插件从 `bundled_root`（`<repo>/plugins/bundled`）读取 `plugin.json` 并同步到用户注册表。
3. **External** 插件从 `install_root`（`~/.claw/plugins/installed/`）加载已经安装的插件。
4. 最后根据 **settings.json** 中的 `enabledPlugins` 过滤出实际可用的插件。

## 2. 配置文件位置
| 文件 | 作用 | 默认路径 |
|------|------|----------|
| `settings.json` | 保存全局配置，包括 **enabledPlugins**（记录每个插件的启用状态）和 **PluginManagerConfig**（如 `install_root`、`bundled_root`、`external_dirs`）。 | `~/.claw/settings.json`（或 `$XDG_CONFIG_HOME/claw/settings.json`） |
| `installed.json` | 注册已安装的 External 插件（插件 ID、版本、安装路径、来源 URL、时间戳）。 | `~/.claw/plugins/installed.json` |
| `plugin.json` (每个插件) | 插件自身的元信息及工具声明。放在插件根目录（即插件仓库根）下。 |

### 2.1 `settings.json` 示例
```json
{
  "enabledPlugins": {
    "hello-plugin": true,
    "example-bundled": false
  },
  "plugin_manager": {
    "install_root": "${HOME}/.claw/plugins",
    "registry_path": "${HOME}/.claw/plugins/installed.json",
    "bundled_root": "$(pwd)/plugins/bundled",
    "external_dirs": ["/opt/claw/extra_plugins"]
  }
}
```
> **Tip**：如果项目根目录下已经有 `settings.json`，`claw` 会在启动时合并默认值与用户自定义值。

## 3. Plugin Manifest (`plugin.json`)
插件的元信息全部写在 `plugin.json`，解析逻辑位于 `PluginRegistry::parse_manifest`（`src/lib.rs` 中）。下面列出必选与可选字段。

| 字段 | 必填 | 类型 | 说明 |
|------|------|------|------|
| `name` | ✅ | string | 插件唯一标识（在 `installed.json` 中作为 `id` 使用）。 |
| `version` | ✅ | string (SemVer) | 插件版本，用于 `update` 判断是否有新版本。 |
| `description` | ✅ | string | 简要描述，显示在 `claw plugins list` 中。 |
| `defaultEnabled` | ❌ | bool | 是否在第一次安装后默认启用。默认 `false`。 |
| `tools` | ✅ | array of objects | 声明插件提供的子命令（工具），每个对象结构见下表。 |
| `hooks` | ❌ | object | 生命周期钩子（如 `on_start`、`on_exit`），暂未实现，仅预留。 |
| `permissions` | ❌ | array[string] | 插件全局所需权限列表，若省略则以每个工具声明的权限为准。 |

### 3.1 `tools` 子结构
| 字段 | 必填 | 类型 | 说明 |
|------|------|------|------|
| `name` | ✅ | string | 被挂载到 `claw` 主 CLI 的子命令名，例如 `hello`。 |
| `description` | ✅ | string | `claw plugins list` 时的简短描述。 |
| `command` | ✅ | string | 实际可执行文件的相对路径或绝对路径（相对于插件根目录）。当 `claw hello …` 被调用时，内部会执行 `command` 并把后面的参数转发。 |
| `args` | ❌ | array[string] | 默认参数列表（可省略）。 |
| `inputSchema` | ❌ | object | JSON‑Schema，用于 CLI 参数校验（未来特性）。 |
| `permissions` | ✅ | array[string] | 取值范围：`read`、`write`、`execute`（对应 `read‑only`、`workspace‑write`、`danger‑full‑access`）。若插件需要写入工作区文件，需要 `write` 权限。 |

## 4. 常用 CLI 命令
下面列出 `claw` 已实现的插件管理子命令（均在 `src/cli/plugins.rs` 中实现）。

| 命令 | 用法 | 说明 |
|------|------|------|
| `claw plugins list` | `claw plugins list` | 列出 **所有已知插件**（包括未安装的 bundled），并标明 `enabled/disabled`。 |
| `claw plugins installed` | `claw plugins installed` | 只列出已在 `installed.json` 中注册的 External 插件。 |
| `claw plugins install <src>` | `claw plugins install ./myplugin`<br>`claw plugins install https://github.com/user/foo-plugin` | 支持本地路径或 Git URL。会将插件克隆到 `<install_root>/tmp/`，校验 `plugin.json`，然后移动到正式安装目录并更新 `installed.json`。默认自动 `enable`（除非使用 `--disable` 标记）。 |
| `claw plugins enable <id>` | `claw plugins enable hello-plugin` | 将插件的 `enabledPlugins[id]` 设为 `true`（如果插件没有 `defaultEnabled` 仍可手动开启）。 |
| `claw plugins disable <id>` | `claw plugins disable hello-plugin` | 将插件的 `enabledPlugins[id]` 设为 `false`。 |
| `claw plugins update <id>` | `claw plugins update hello-plugin` | 重新拉取插件的来源（如果是 Git），更新版本号并写入 `installed.json`。 |
| `claw plugins uninstall <id>` | `claw plugins uninstall hello-plugin` | 删除插件目录并从 `installed.json` 中移除记录。对于 Bundled 插件，此命令会报错因为它们是只读的。 |
| `claw plugins validate <path>` | `claw plugins validate ./myplugin` |（可选实现）检查给定路径下的 `plugin.json` 是否符合 schema。 |

## 5. 完整使用示例（从零开始）

### 5.1 创建最小插件项目
```text
myhello/
├─ src/
│   └─ main.rs        # 生成一个可执行二进制 `myhello`
└─ plugin.json        # 插件元信息
```

**`myhello/src/main.rs`**（使用 `clap` 创建一个简单 CLI）
```rust
use clap::Parser;

#[derive(Parser)]
#[clap(name = "myhello", version = "0.1.0", author = "You")]
struct Opt {
    /// 要问好的对象
    name: String,
}

fn main() {
    let opt = Opt::parse();
    println!("Hello, {}!", opt.name);
}
```
编译得到可执行文件 `target/release/myhello`。

**`myhello/plugin.json`**
```json
{
  "name": "myhello",
  "version": "0.1.0",
  "description": "Provides a friendly hello command.",
  "defaultEnabled": true,
  "tools": [
    {
      "name": "hello",
      "description": "Print a greeting.",
      "command": "target/release/myhello",
      "permissions": ["read"]
    }
  ]
}
```
> **注意**：`command` 可以是相对路径（相对于插件根目录），`claw` 在运行时会 `std::path::Path::new(&plugin_root).join(&cmd)` 形成完整路径。

### 5.2 安装插件
```bash
claw plugins install ./myhello   # 本地路径
# 或者远程 Git 仓库
claw plugins install https://github.com/you/myhello-plugin.git
```
执行后会在 `~/.claw/plugins/installed/myhello/` 生成以下结构：
```
myhello/
├─ plugin.json
├─ target/
│   └─ release/
│       └─ myhello   # 可执行文件
└─ src/...                # 源码（可选保留）
```
`installed.json` 会写入类似记录：
```json
{
  "myhello": {
    "version": "0.1.0",
    "install_path": "${HOME}/.claw/plugins/installed/myhello",
    "source": "git",
    "url": "https://github.com/you/myhello-plugin.git",
    "installed_at": "2026-04-13T12:34:56Z"
  }
}
```
默认情况下因为 `defaultEnabled: true`，`settings.json` 会自动写入 `"myhello": true`。

### 5.3 启用 / 禁用
```bash
claw plugins disable myhello   # 关闭插件
claw plugins enable myhello    # 再次打开
```
此时 `claw hello Alice` 将调用 `myhello` 可执行文件并输出 `Hello, Alice!`。

### 5.4 更新插件
如果插件源码在 Git 上有新提交，只需执行：
```bash
claw plugins update myhello
```
`claw` 会重新 `git pull`（或重新 clone）到本地，并更新 `installed.json` 中的 `version` 与 `installed_at`。若版本号不变则提示 “already up‑to‑date”。

### 5.5 卸载插件
```bash
claw plugins uninstall myhello
```
此命令会删除 `~/.claw/plugins/installed/myhello/` 以及对应的 `installed.json` 条目，同时从 `settings.json` 中清除 `myhello` 键。

## 6. 高级配置
### 6.1 自定义插件根目录
如果你想把所有插件放在 `/opt/claw/plugins`，可以在 `settings.json` 加入：
```json
{
  "plugin_manager": {
    "install_root": "/opt/claw/plugins",
    "registry_path": "/opt/claw/plugins/installed.json",
    "bundled_root": "/path/to/your/repo/plugins/bundled",
    "external_dirs": ["/opt/claw/extra_plugins"]
  }
}
```
> **Tip**：`claw` 在启动时会首先读取环境变量 `CLAW_CONFIG_DIR`，如果该变量存在，它会优先使用 `${CLAW_CONFIG_DIR}/settings.json` 作为配置文件路径。

### 6.2 添加额外搜索目录
有时你想让 `claw` 同时搜索公司内部的插件仓库，只需在配置里写：
```json
"external_dirs": ["/opt/company/claw_plugins", "/home/me/.local/claw_plugins"]
```
这些目录会在 **External** 阶段被递归搜索，只要目录下有合法的 `plugin.json` 即可自动加入 `installed.json`（不需要 `install` 步骤，仅需要放置好目录结构）。

## 7. 常见错误与排查指南
| 错误现象 | 可能原因 | 排查步骤 |
|----------|----------|----------|
| `claw plugins install` 结束后仍提示 **"plugin.json not found"** | 插件根目录缺少 `plugin.json`，或文件名大小写错误。 | 1. `cat <plugin_root>/plugin.json` 确认文件存在。<br>2. 检查 JSON 是否符合 schema（字段完整性、SemVer 版本号）。 |
| 插件未出现在 `claw plugins list` | `installed.json` 没有对应记录，或 `settings.json` 中 `enabledPlugins` 为 `false`。 | 查看 `~/.claw/plugins/installed.json` 是否有条目；检查 `settings.json` 中对应键值。 |
| 调用插件子命令时报 `PermissionDenied` | `plugin.json` 中声明的 `write`/`execute` 权限未在全局配置中开启。 | 1. 查看 `claw config show` 中 `allow_workspace_write`、`allow_dangerous` 选项。<br>2. 若是测试环境，可 `claw config set allow_workspace_write true`（仅调试）。 |
| `claw plugins update` 失败，提示 **"not a git repository"** | 插件是本地目录安装的，未记录源码 URL。 | 只能对 **Git** 源安装的插件使用 `update`，本地路径插件请手动重新 `install`。

## 8. 小结
- **插件种类**：Builtin（内置），Bundled（随仓库提供），External（用户自行安装）。
- **插件功能**：通过 `plugin.json` 声明工具（子命令）以及所需权限，`claw` 在启动时自动将这些工具挂载到主 CLI。 
- **实现要点**：
  1. `PluginManager` 负责发现、加载、注册插件。
  2. `PluginRegistry` 维护 `installed.json` 和 `bundled` 同步逻辑。
  3. `settings.json` 保存用户的启用状态。
  4. CLI 子命令 `claw plugins …` 为用户提供完整的增删改查操作。

有了这份指南，您即可轻松在 Claw 中 **创建、管理、使用插件**，从而让 CLIs 具备可扩展的功能。

---

*本文件基于当前代码库自动生成，若项目结构有变动请同步更新相应章节。*