export const SETTINGS_TEMPLATE = `
<div class="settings-view-inner">
            <div class="settings-header">
              <h3 data-i18n="settings.title">设置</h3>
            </div>
            <!-- 工作模式选择器 -->
            <div class="working-mode-selector">
              <div class="working-mode-title" data-i18n="mode.title">工作模式</div>
              <div class="working-mode-cards">
                <div class="working-mode-card active" data-mode="standalone">
                  <div class="mode-card-icon">💻</div>
                  <div class="mode-card-info">
                    <div class="mode-card-name" data-i18n="mode.standalone">单机模式</div>
                    <div class="mode-card-desc" data-i18n="mode.standalone_desc">本地配置 + 独立运行</div>
                  </div>
                </div>
                <div class="working-mode-card" data-mode="router">
                  <div class="mode-card-icon">🔗</div>
                  <div class="mode-card-info">
                    <div class="mode-card-name" data-i18n="mode.router">团队模式</div>
                    <div class="mode-card-desc" data-i18n="mode.router_desc">Router 共享配置</div>
                  </div>
                </div>
              </div>
            </div>
            <!-- Tab 切换栏 -->
            <div class="settings-tabs">
              <button class="settings-tab active" data-tab="general" data-i18n="settings.tab_general">通用</button>
              <button class="settings-tab" data-tab="models" data-i18n="settings.tab_models">模型</button>
              <button class="settings-tab" data-tab="tools" data-i18n="settings.tab_tools">工具</button>

              <button class="settings-tab" data-tab="memory" data-i18n="settings.tab_memory">记忆</button>
              <button class="settings-tab" data-tab="connections" data-i18n="settings.tab_connections">Router</button>
              <button class="settings-tab" data-tab="weixin">微信</button>
              <button class="settings-tab" data-tab="evolution">进化</button>
            </div>
            <!-- 通用设置 Tab -->
            <div class="settings-body settings-tab-content active" id="settings-tab-general" data-tab="general">
              <!-- 外观与语言 -->
              <div class="settings-section-title" style="margin-top:0; padding-top:0; border-top:none;"
                data-i18n="settings.appearance_section">外观与语言</div>

              <!-- 界面语言 -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.language">界面语言</span>
                  <span class="settings-item-desc" data-i18n="settings.language_desc">切换客户端显示语言</span>
                </div>
                <select id="locale-select" class="settings-select" style="width:120px;">
                  <option value="zh">中文</option>
                  <option value="en">English</option>
                </select>
              </div>

              <!-- 输出目录 -->
              <div class="settings-item settings-item-column">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.output_dir">输出目录</span>
                  <span class="settings-item-desc" data-i18n="settings.output_dir_desc">Agent 生成文件的默认保存位置</span>
                </div>
                <div class="settings-path-row">
                  <input type="text" id="output-path-input" class="settings-path-input" readonly placeholder="加载中..." />
                  <button id="output-path-browse" class="settings-path-btn" data-i18n-title="settings.output_browse"
                    title="浏览">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
                    </svg>
                  </button>
                  <button id="output-path-reset" class="settings-path-btn" data-i18n-title="settings.output_reset"
                    title="重置默认">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <polyline points="1 4 1 10 7 10" />
                      <path d="M3.51 15a9 9 0 105.64-11.36L1 10" />
                    </svg>
                  </button>
                </div>
              </div>

              <!-- 语音设置 -->
              <div class="settings-section-title" data-i18n="settings.voice_section">语音</div>
              <!-- 语音不可用提示 -->
              <div id="voice-unavailable-notice" class="voice-unavailable-notice">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                  stroke-linecap="round" stroke-linejoin="round">
                  <circle cx="12" cy="12" r="10" />
                  <line x1="12" y1="8" x2="12" y2="12" />
                  <line x1="12" y1="16" x2="12.01" y2="16" />
                </svg>
                <span data-i18n="settings.voice_unavailable">语音功能当前不可用，需要下载语音模型后才能使用</span>
              </div>
              <!-- TTS 自动播放 -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.tts_autoplay">自动朗读回复</span>
                  <span class="settings-item-desc" data-i18n="settings.tts_autoplay_desc">助手回复完成后自动播放语音</span>
                </div>
                <label class="toggle-switch">
                  <input type="checkbox" id="tts-autoplay-toggle" />
                  <span class="toggle-slider"></span>
                </label>
              </div>

              <!-- TTS 语音选择 -->
              <div class="settings-item settings-item-column">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.tts_voice">语音角色</span>
                  <span class="settings-item-desc" data-i18n="settings.tts_voice_desc">TTS 朗读使用的语音</span>
                </div>
                <select id="tts-voice-select" class="settings-select">
                  <option value="zh-CN-XiaoxiaoNeural">晓晓 (zh-CN-Xiaoxiao)</option>
                  <option value="zh-CN-YunxiNeural">云希 (zh-CN-Yunxi)</option>
                  <option value="zh-CN-YunjianNeural">云健 (zh-CN-Yunjian)</option>
                  <option value="zh-CN-XiaoyiNeural">晓伊 (zh-CN-Xiaoyi)</option>
                  <option value="en-US-JennyNeural">Jenny (en-US)</option>
                  <option value="en-US-GuyNeural">Guy (en-US)</option>
                </select>
              </div>

              <!-- 高级 -->
              <div class="settings-section-title" data-i18n="settings.advanced_section">高级</div>
              <!-- Debug 模式 -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.debug_mode">Debug 模式</span>
                  <span class="settings-item-desc" data-i18n="settings.debug_mode_desc">在底部显示 Gateway 实时日志</span>
                </div>
                <label class="toggle-switch">
                  <input type="checkbox" id="debug-mode-toggle" />
                  <span class="toggle-slider"></span>
                </label>
              </div>
            </div>
            <!-- 模型设置 Tab -->
            <div class="settings-body settings-tab-content" id="settings-tab-models" data-tab="models">
              <!-- 模型配置 -->
              <div class="settings-section-title" style="margin-top:0; padding-top:0; border-top:none;"
                data-i18n="settings.model_config">模型配置</div>

              <!-- 编排模型组 -->
              <div class="settings-model-group">
                <div class="settings-model-group-header">
                  <span class="settings-model-group-icon">🧠</span>
                  <div class="settings-model-group-title">
                    <span class="settings-model-group-name" data-i18n="settings.orch_model">编排模型</span>
                    <span class="settings-model-group-desc" data-i18n="settings.orch_model_desc">主 Agent
                      推理、任务规划和路由决策</span>
                  </div>
                </div>
                <div class="settings-model-group-body">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label" data-i18n="settings.provider_label">供应商</label>
                    <select id="server-orch-provider" class="settings-select">
                      <option value="anthropic">Anthropic</option>
                      <option value="openai">OpenAI</option>
                      <option value="minimax">MiniMax</option>
                      <option value="deepseek">DeepSeek</option>
                      <option value="zhipu">智谱 (Zhipu)</option>
                      <option value="moonshot">Moonshot (Kimi)</option>
                      <option value="google">Google</option>
                      <option value="ollama">Ollama (本地)</option>
                      <option value="custom">自定义</option>
                    </select>
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label" data-i18n="settings.model_label">模型</label>
                    <select id="server-orch-model" class="settings-select">
                      <!-- 由 JS 根据供应商动态填充 -->
                    </select>
                    <input type="text" id="server-orch-model-custom" class="settings-input hidden"
                      placeholder="输入自定义模型名称" style="margin-top:6px;" />
                  </div>
                </div>
              </div>

              <!-- 执行模型组 -->
              <div class="settings-model-group">
                <div class="settings-model-group-header">
                  <span class="settings-model-group-icon">⚡</span>
                  <div class="settings-model-group-title">
                    <span class="settings-model-group-name" data-i18n="settings.exec_model">执行模型</span>
                    <span class="settings-model-group-desc" data-i18n="settings.exec_model_desc">SubAgent
                      工具调用和子任务执行</span>
                  </div>
                </div>
                <div class="settings-model-group-body">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">供应商</label>
                    <select id="server-exec-provider" class="settings-select">
                      <option value="anthropic">Anthropic</option>
                      <option value="openai">OpenAI</option>
                      <option value="minimax">MiniMax</option>
                      <option value="deepseek">DeepSeek</option>
                      <option value="zhipu">智谱 (Zhipu)</option>
                      <option value="moonshot">Moonshot (Kimi)</option>
                      <option value="google">Google</option>
                      <option value="ollama">Ollama (本地)</option>
                      <option value="custom">自定义</option>
                    </select>
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">模型</label>
                    <select id="server-exec-model" class="settings-select">
                      <!-- 由 JS 根据供应商动态填充 -->
                    </select>
                    <input type="text" id="server-exec-model-custom" class="settings-input hidden"
                      placeholder="输入自定义模型名称" style="margin-top:6px;" />
                  </div>
                </div>
              </div>


              <!-- 供应商密钥 -->
              <div class="settings-section-title" data-i18n="settings.provider_keys">供应商密钥</div>

              <div id="server-provider-keys" class="settings-provider-keys">
                <!-- 动态生成供应商密钥输入 -->
              </div>

              <!-- Agent 独立模型配置（仅单机模式） -->
              <div id="agent-model-section">
                <div class="settings-section-title"
                  data-i18n="agent.model_section">Agent Models</div>
                <div class="settings-item settings-item-column">
                  <div class="settings-item-info">
                    <span class="settings-item-label" data-i18n="agent.model_independent">Independent Model Config</span>
                    <span class="settings-item-desc" data-i18n="agent.model_independent_desc">Assign independent models
                      per Agent, falls back to global Orchestration model if not set</span>
                  </div>
                  <div id="agent-model-list" class="agent-model-list"></div>
                </div>
              </div>

              <!-- 保存按钮 -->
              <div class="settings-save-row">
                <span id="server-save-hint" class="settings-save-hint"></span>
                <button id="server-save-btn" class="primary-btn settings-save-btn"
                  data-i18n="common.save_config">保存配置</button>
              </div>
            </div>
            <!-- 工具设置 Tab -->
            <div class="settings-body settings-tab-content" id="settings-tab-tools" data-tab="tools">
              <!-- Web 搜索与获取 -->
              <div class="settings-section-title" style="margin-top:0; padding-top:0; border-top:none;"
                data-i18n="settings.web_section">Web 搜索与获取</div>

              <!-- Web Search 配置组 -->
              <div class="settings-model-group">
                <div class="settings-model-group-header">
                  <span class="settings-model-group-icon">🔍</span>
                  <div class="settings-model-group-title">
                    <span class="settings-model-group-name" data-i18n="settings.web_search">Web 搜索</span>
                    <span class="settings-model-group-desc" data-i18n="settings.web_search_desc">Agent
                      搜索互联网获取实时信息</span>
                  </div>
                </div>
                <div class="settings-model-group-body">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label" data-i18n="settings.search_provider">搜索提供商</label>
                    <select id="server-web-search-provider" class="settings-select">
                      <option value="brave">Brave Search</option>
                      <option value="perplexity">Perplexity</option>
                    </select>
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">API Key</label>
                    <div class="settings-provider-key-input-row">
                      <input type="password" id="server-web-search-apikey" class="settings-provider-key-input"
                        placeholder="输入搜索 API Key..." />
                      <button id="server-web-search-apikey-toggle" class="settings-provider-key-toggle" title="显示/隐藏">
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                          stroke-width="2">
                          <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                          <circle cx="12" cy="12" r="3" />
                        </svg>
                      </button>
                    </div>
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label" data-i18n="settings.search_max_results">最大结果数</label>
                    <input type="number" id="server-web-search-max-results" class="settings-input" min="1" max="10"
                      value="5" />
                  </div>
                </div>
              </div>

              <!-- Web Fetch 配置组 -->
              <div class="settings-model-group">
                <div class="settings-model-group-header">
                  <span class="settings-model-group-icon">🌐</span>
                  <div class="settings-model-group-title">
                    <span class="settings-model-group-name" data-i18n="settings.web_fetch">网页获取</span>
                    <span class="settings-model-group-desc" data-i18n="settings.web_fetch_desc">抓取网页正文内容供 Agent
                      分析</span>
                  </div>
                </div>
                <div class="settings-model-group-body">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label" data-i18n="settings.fetch_readability">Readability
                      提取</label>
                    <label class="toggle-switch" style="align-self:flex-start;">
                      <input type="checkbox" id="server-web-fetch-readability" checked />
                      <span class="toggle-slider"></span>
                    </label>
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label" data-i18n="settings.fetch_max_chars">最大字符数</label>
                    <input type="number" id="server-web-fetch-max-chars" class="settings-input" min="100"
                      value="50000" />
                  </div>
                </div>
              </div>

              <!-- MCP 外部工具 -->
              <div class="settings-section-title" data-i18n="settings.mcp_section">MCP 外部工具</div>
              <div class="settings-model-group-desc" style="margin:-4px 0 8px;opacity:0.6;font-size:12px;"
                data-i18n="settings.mcp_desc">通过 MCP 协议连接外部工具服务器，扩展 Agent 能力</div>

              <!-- MCP Server 列表（动态渲染） -->
              <div id="mcp-servers-list"></div>

              <!-- 添加 MCP Server 按钮 -->
              <button id="mcp-add-btn" class="mcp-add-btn">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <line x1="12" y1="5" x2="12" y2="19" />
                  <line x1="5" y1="12" x2="19" y2="12" />
                </svg>
                添加 MCP Server
              </button>

              <!-- MCP 添加/编辑表单（默认隐藏） -->
              <div id="mcp-form" class="mcp-form hidden">
                <div class="mcp-form-title" id="mcp-form-title">添加 MCP Server</div>
                <div class="settings-model-field">
                  <label class="settings-model-field-label">名称</label>
                  <input type="text" id="mcp-form-name" class="settings-input" placeholder="例如: my-tools" />
                </div>
                <div class="settings-model-field">
                  <label class="settings-model-field-label">运行位置</label>
                  <select id="mcp-form-location" class="settings-select">
                    <option value="server">服务端（Gateway 机器）</option>
                    <option value="client">客户端（本机）</option>
                  </select>
                </div>
                <div class="settings-model-field">
                  <label class="settings-model-field-label">传输方式</label>
                  <select id="mcp-form-transport" class="settings-select">
                    <option value="stdio">stdio（本地命令）</option>
                    <option value="sse">SSE（远程服务）</option>
                  </select>
                </div>
                <div id="mcp-form-stdio-fields">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">命令</label>
                    <input type="text" id="mcp-form-command" class="settings-input" placeholder="例如: npx, python" />
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">参数</label>
                    <input type="text" id="mcp-form-args" class="settings-input"
                      placeholder="空格分隔，例如: -m my_server --port 8080" />
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">环境变量</label>
                    <input type="text" id="mcp-form-env" class="settings-input" placeholder="KEY=VALUE 空格分隔" />
                  </div>
                </div>
                <div id="mcp-form-sse-fields" class="hidden">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">服务器 URL</label>
                    <input type="text" id="mcp-form-url" class="settings-input"
                      placeholder="http://localhost:8080/sse" />
                  </div>
                </div>
                <div class="mcp-form-actions">
                  <button id="mcp-form-cancel" class="mcp-form-btn secondary">取消</button>
                  <button id="mcp-form-submit" class="mcp-form-btn primary">确认</button>
                </div>
              </div>

              <!-- 安全与沙盒 -->
              <div class="settings-section-title" data-i18n="settings.security_section">安全与沙盒</div>

              <!-- 执行模式 -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.sandbox_mode">执行模式</span>
                  <span class="settings-item-desc" data-i18n="settings.sandbox_mode_desc">local: 仅代码加固（默认） / docker:
                    容器隔离</span>
                </div>
                <select id="server-sandbox-mode" class="settings-select" style="width:120px;">
                  <option value="local">本地 (local)</option>
                  <option value="docker">Docker</option>
                </select>
              </div>

              <!-- Docker 配置（仅 docker 模式显示） -->
              <div id="sandbox-docker-fields" class="settings-model-group hidden">
                <div class="settings-model-group-header">
                  <span class="settings-model-group-icon">🐳</span>
                  <div class="settings-model-group-title">
                    <span class="settings-model-group-name">Docker 配置</span>
                    <span class="settings-model-group-desc">需先构建镜像: docker build -f Dockerfile.sandbox -t
                      openflux-sandbox
                      .</span>
                  </div>
                </div>
                <div class="settings-model-group-body">
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">镜像名</label>
                    <input type="text" id="server-sandbox-docker-image" class="settings-input"
                      placeholder="openflux-sandbox" value="openflux-sandbox" />
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">内存限制</label>
                    <input type="text" id="server-sandbox-docker-memory" class="settings-input" placeholder="512m"
                      value="512m" />
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">CPU 限制</label>
                    <input type="text" id="server-sandbox-docker-cpu" class="settings-input" placeholder="1"
                      value="1" />
                  </div>
                  <div class="settings-model-field">
                    <label class="settings-model-field-label">网络模式</label>
                    <select id="server-sandbox-docker-network" class="settings-select">
                      <option value="none">断网 (none)</option>
                      <option value="bridge">桥接 (bridge)</option>
                      <option value="host">宿主机 (host)</option>
                    </select>
                  </div>
                </div>
              </div>

              <!-- 禁写扩展名 -->
              <div class="settings-item settings-item-column">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="settings.blocked_ext">禁止写入的文件类型</span>
                  <span class="settings-item-desc" data-i18n="settings.blocked_ext_desc">以逗号分隔，如 exe,bat,ps1,cmd</span>
                </div>
                <input type="text" id="server-sandbox-blocked-ext" class="settings-input"
                  placeholder="exe,bat,ps1,cmd,vbs,reg,msi" style="margin-top:6px;" />
              </div>

              <!-- 保存按钮 (tools tab shares the server save) -->
              <div class="settings-save-row">
                <span id="tools-save-hint" class="settings-save-hint"></span>
                <button id="tools-save-btn" class="primary-btn settings-save-btn"
                  data-i18n="common.save_config">保存配置</button>
              </div>
            </div>
            <!-- 记忆管理 Tab -->
            <div class="settings-body settings-tab-content" id="settings-tab-memory" data-tab="memory">
              <!-- 蒸馏系统 -->
              <div id="distillation-section" class="distillation-section hidden">
                <div class="settings-section-title" style="margin-top:0;padding-top:0;border-top:none;"
                  data-i18n="memory.distill_title">🌙 Memory Distillation</div>

                <!-- 蒸馏统计卡片 -->
                <div class="distill-stats-grid">
                  <div class="distill-stat-card micro">
                    <div class="distill-stat-value" id="distill-stat-micro">0</div>
                    <div class="distill-stat-label" data-i18n="memory.micro_cards">Micro Cards</div>
                  </div>
                  <div class="distill-stat-card mini">
                    <div class="distill-stat-value" id="distill-stat-mini">0</div>
                    <div class="distill-stat-label" data-i18n="memory.mini_cards">Mini Cards</div>
                  </div>
                  <div class="distill-stat-card macro">
                    <div class="distill-stat-value" id="distill-stat-macro">0</div>
                    <div class="distill-stat-label" data-i18n="memory.macro_cards">Macro Cards</div>
                  </div>
                  <div class="distill-stat-card topics">
                    <div class="distill-stat-value" id="distill-stat-topics">0</div>
                    <div class="distill-stat-label" data-i18n="memory.topics">Topics</div>
                  </div>
                </div>

                <!-- 调度器状态 -->
                <div class="distill-scheduler-status">
                  <span id="distill-scheduler-indicator" class="distill-status-dot off"></span>
                  <span id="distill-scheduler-text" data-i18n="memory.scheduler_disabled">调度器未启用</span>
                </div>

                <!-- 蒸馏配置面板 -->
                <div class="distill-config-panel">
                  <div class="distill-config-row">
                    <label class="distill-config-label" data-i18n="memory.distill_enable">启用蒸馏</label>
                    <label class="distill-toggle">
                      <input type="checkbox" id="distill-enabled">
                      <span class="distill-toggle-slider"></span>
                    </label>
                  </div>
                  <div class="distill-config-row">
                    <label class="distill-config-label" data-i18n="memory.distill_period">蒸馏时段</label>
                    <div class="distill-time-range">
                      <input type="time" id="distill-start-time" class="distill-time-input" value="02:00">
                      <span class="distill-time-sep">—</span>
                      <input type="time" id="distill-end-time" class="distill-time-input" value="06:00">
                    </div>
                  </div>
                  <div class="distill-config-row">
                    <label class="distill-config-label" data-i18n="memory.quality_threshold">质量阈值</label>
                    <input type="number" id="distill-quality-threshold" class="distill-number-input" min="0" max="100"
                      value="40">
                  </div>
                  <div class="distill-config-row">
                    <label class="distill-config-label" data-i18n="memory.session_density">会话密度阈值</label>
                    <input type="number" id="distill-session-density" class="distill-number-input" min="1" max="50"
                      value="5">
                  </div>
                  <div class="distill-config-row">
                    <label class="distill-config-label" data-i18n="memory.similarity_threshold">相似度阈值</label>
                    <input type="number" id="distill-similarity-threshold" class="distill-number-input" min="0" max="1"
                      step="0.01" value="0.85">
                  </div>
                  <div class="distill-config-actions">
                    <button id="distill-save-btn" class="primary-btn" data-i18n="common.save_config">保存配置</button>
                    <button id="distill-trigger-btn" class="memory-action-btn" data-i18n="memory.manual_distill">⚡
                      手动蒸馏</button>
                  </div>
                </div>

                <!-- 记忆卡片管理 -->
                <div class="distill-cards-section">
                  <div class="distill-cards-header">
                    <div class="distill-cards-tabs">
                      <button class="distill-tab active" data-layer="" data-i18n="memory.tab_all">全部</button>
                      <button class="distill-tab" data-layer="Micro">Micro</button>
                      <button class="distill-tab" data-layer="Mini">Mini</button>
                      <button class="distill-tab" data-layer="Macro">Macro</button>
                    </div>
                    <div class="distill-cards-actions">
                      <span id="distill-cards-count" class="distill-cards-count"></span>
                      <button id="distill-cards-refresh" class="memory-action-btn" data-i18n-title="common.refresh"
                        title="Refresh">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                          stroke-width="2">
                          <polyline points="23 4 23 10 17 10" />
                          <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                        </svg>
                      </button>
                    </div>
                  </div>
                  <div id="distill-cards-list" class="distill-cards-list"></div>
                  <div id="distill-cards-empty" class="distill-cards-empty hidden" data-i18n="memory.no_cards">暂无记忆卡片
                  </div>
                </div>
              </div>

              <!-- 记忆未启用提示 -->
              <div id="memory-disabled-notice" class="memory-disabled-notice hidden">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <circle cx="12" cy="12" r="10" />
                  <line x1="12" y1="8" x2="12" y2="12" />
                  <line x1="12" y1="16" x2="12.01" y2="16" />
                </svg>
                <span data-i18n-html="memory.disabled_notice">记忆系统未启用。请在 openflux.yaml 中配置
                  <code>memory.enabled: true</code></span>
              </div>

              <!-- 搜索栏 -->
              <div id="memory-search-bar" class="memory-search-bar">
                <input type="text" id="memory-search-input" class="memory-search-input"
                  data-i18n-placeholder="memory.search_placeholder" placeholder="搜索记忆（语义 + 关键词）..." />
                <button id="memory-search-btn" class="memory-search-btn" data-i18n-title="common.search" title="Search">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <circle cx="11" cy="11" r="8" />
                    <path d="M21 21l-4.35-4.35" />
                  </svg>
                </button>
                <button id="memory-search-clear" class="memory-search-btn hidden" data-i18n-title="memory.clear_search"
                  title="Clear search">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <line x1="18" y1="6" x2="6" y2="18" />
                    <line x1="6" y1="6" x2="18" y2="18" />
                  </svg>
                </button>
              </div>

              <!-- 记忆列表 -->
              <div id="memory-list" class="memory-list">
                <div class="memory-empty-state" data-i18n="memory.empty_loading">加载中...</div>
              </div>

              <!-- 分页 -->
              <div id="memory-pagination" class="memory-pagination hidden">
                <button id="memory-page-prev" class="memory-page-btn" disabled data-i18n="common.prev_page">上一页</button>
                <span id="memory-page-info" class="memory-page-info">1 / 1</span>
                <button id="memory-page-next" class="memory-page-btn" disabled data-i18n="common.next_page">下一页</button>
              </div>

              <!-- 操作区 -->
              <div class="memory-actions">
                <button id="memory-refresh-btn" class="memory-action-btn">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <polyline points="23 4 23 10 17 10" />
                    <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                  </svg>
                  <span data-i18n="common.refresh">Refresh</span>
                </button>
                <button id="memory-clear-btn" class="memory-action-btn danger">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <polyline points="3 6 5 6 21 6" />
                    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                  </svg>
                  <span data-i18n="memory.clear_all">Clear All Memories</span>
                </button>
                <!-- 系统信息按钮 -->
                <div class="memory-sysinfo-wrapper">
                  <button id="memory-sysinfo-btn" class="memory-action-btn memory-sysinfo-btn">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <circle cx="12" cy="12" r="10" />
                      <line x1="12" y1="16" x2="12" y2="12" />
                      <line x1="12" y1="8" x2="12.01" y2="8" />
                    </svg>
                    <span data-i18n="memory.system_info">System Info</span>
                  </button>
                  <!-- 系统信息弹层 -->
                  <div id="memory-sysinfo-panel" class="memory-sysinfo-panel hidden">
                    <div class="memory-sysinfo-header">
                      <span data-i18n="memory.system_info_title">Memory System Info</span>
                      <button id="memory-sysinfo-close" class="memory-sysinfo-close">&times;</button>
                    </div>
                    <div class="memory-sysinfo-grid">
                      <div class="memory-sysinfo-item">
                        <div class="memory-sysinfo-label" data-i18n="memory.total_count">Total Memories</div>
                        <div class="memory-sysinfo-value" id="memory-stat-count">—</div>
                      </div>
                      <div class="memory-sysinfo-item">
                        <div class="memory-sysinfo-label" data-i18n="memory.db_size">Database Size</div>
                        <div class="memory-sysinfo-value" id="memory-stat-size">—</div>
                      </div>
                      <div class="memory-sysinfo-item">
                        <div class="memory-sysinfo-label" data-i18n="memory.vector_dim">Vector Dimensions</div>
                        <div class="memory-sysinfo-value" id="memory-stat-dim">—</div>
                      </div>
                      <div class="memory-sysinfo-item">
                        <div class="memory-sysinfo-label" data-i18n="memory.embed_model">Embedding Model</div>
                        <div class="memory-sysinfo-value" id="memory-stat-model">—</div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <!-- 智能体设置 Tab -->
            <!-- 连接 Tab -->
            <div class="settings-body settings-tab-content" id="settings-tab-connections" data-tab="connections">
              <div class="settings-section-title" style="margin-top:0;padding-top:0;border-top:none;"
                data-i18n="cloud.account_title">OpenFlux Cloud Account
              </div>
              <!-- 未登录 -->
              <div id="openflux-settings-not-logged" class="openflux-logged-in">
                <div class="openflux-user-info">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--color-text-secondary)"
                    stroke-width="2">
                    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                    <circle cx="12" cy="7" r="4" />
                  </svg>
                  <span style="color:var(--color-text-secondary);" data-i18n="cloud.not_logged">未登录 —
                    请通过侧边栏底部按钮登录</span>
                </div>
              </div>
              <!-- 已登录 -->
              <div id="openflux-settings-logged" class="openflux-logged-in hidden">
                <div class="openflux-user-info">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="2">
                    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                    <circle cx="12" cy="7" r="4" />
                  </svg>
                  <span id="openflux-settings-username">—</span>
                  <button id="openflux-settings-logout-btn" class="openflux-logout-btn"
                    data-i18n="cloud.logout">登出</button>
                </div>
              </div>

              <!-- OpenFluxRouter 配置 -->
              <div class="settings-section-title" data-i18n="cloud.router_title">OpenFluxRouter 消息路由
                <span id="router-status-dot" class="router-status-dot disconnected"
                  data-i18n-title="cloud.status_disconnected" title="Disconnected"></span>
              </div>

              <!-- Router 地址 -->
              <div class="settings-item settings-item-column">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="cloud.router_url">Router URL</span>
                  <span class="settings-item-desc" data-i18n="cloud.router_url_desc">OpenFluxRouter WebSocket
                    endpoint</span>
                </div>
                <input type="text" id="router-url" class="settings-input" placeholder="ws://host:8080/ws/app"
                  style="margin-top:6px;" />
              </div>

              <!-- App ID -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label">App ID</span>
                  <span class="settings-item-desc" data-i18n="cloud.app_id_desc">Application ID registered in
                    Router</span>
                </div>
                <input type="text" id="router-app-id" class="settings-input"
                  data-i18n-placeholder="cloud.app_id_placeholder" placeholder="App ID" style="width:180px;" />
              </div>



              <!-- API Key -->
              <div class="settings-item settings-item-column">
                <div class="settings-item-info">
                  <span class="settings-item-label">API Key</span>
                  <span class="settings-item-desc" data-i18n="cloud.api_key_desc">Bearer Token for Router
                    authentication</span>
                </div>
                <input type="password" id="router-api-key" class="settings-input" placeholder="Bearer Token"
                  style="margin-top:6px;" />
              </div>

              <!-- App User ID -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label">App User ID</span>
                  <span class="settings-item-desc" data-i18n="cloud.app_user_id_desc">User identifier for this instance
                    (auto-generated)</span>
                </div>
                <div style="display:flex;align-items:center;gap:6px;">
                  <input type="text" id="router-app-user-id" class="settings-input"
                    data-i18n-placeholder="cloud.app_user_id_placeholder" placeholder="Auto-generated"
                    style="width:200px;" readonly />
                  <button id="router-regenerate-uid" class="secondary-btn"
                    style="white-space:nowrap;padding:6px 10px;font-size:0.8rem;" data-i18n-title="cloud.regenerate"
                    title="Regenerate">🔄</button>
                </div>
              </div>

              <!-- 启用连接 -->
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="cloud.enable_connection">启用连接</span>
                  <span class="settings-item-desc" data-i18n="cloud.enable_connection_desc">开启后自动连接 Router</span>
                </div>
                <label class="toggle-switch">
                  <input type="checkbox" id="router-enabled" />
                  <span class="toggle-slider"></span>
                </label>
              </div>

              <!-- 使用托管配置开关（团队模式下显示） -->
              <div id="router-managed-config" class="settings-item" style="display:none;">
                <div class="settings-item-info">
                  <span class="settings-item-label" data-i18n="cloud.use_managed">使用托管配置</span>
                  <span class="settings-item-desc" data-i18n="cloud.use_managed_desc">启用后将使用 Router 下发的模型和 Key，替代本地配置</span>
                </div>
                <label class="toggle-switch">
                  <input type="checkbox" id="llm-source-toggle" />
                  <span class="toggle-slider"></span>
                </label>
              </div>


              <!-- 保存按钮 -->
              <div class="settings-save-row">
                <span id="router-save-hint" class="settings-save-hint"></span>
                <button id="router-test-btn" class="secondary-btn settings-save-btn" style="margin-right:8px;"
                  data-i18n="common.test_connection">测试连接</button>
                <button id="router-save-btn" class="primary-btn settings-save-btn"
                  data-i18n="common.save_config">保存配置</button>
              </div>


            </div>
            <!-- 微信 iLink Tab -->
            <div class="settings-body settings-tab-content" id="settings-tab-weixin" data-tab="weixin">
              <!-- 连接状态 -->
              <div class="settings-section-title" style="margin-top:0;padding-top:0;border-top:none;">
                微信 iLink
                <span id="weixin-status-dot" class="router-status-dot disconnected" title="未连接"></span>
              </div>
              <div class="settings-model-group-desc" style="margin:-4px 0 12px;opacity:0.6;font-size:12px;">
                通过腾讯官方 iLink Bot API 接入微信个人号，将微信消息转发给 AI Agent 处理
              </div>

              <!-- 连接信息卡片 -->
              <div id="weixin-connected-info" class="settings-model-group" style="display:none;">
                <div class="settings-model-group-header">
                  <span class="settings-model-group-icon">✅</span>
                  <div class="settings-model-group-title">
                    <span class="settings-model-group-name">已连接</span>
                    <span class="settings-model-group-desc" id="weixin-account-label">Account: —</span>
                  </div>
                </div>
                <div class="settings-model-group-body">
                  <button id="weixin-disconnect-btn" class="secondary-btn" style="color:var(--color-danger);border-color:var(--color-danger);">
                    断开连接
                  </button>
                </div>
              </div>

              <!-- QR 登录区域（未连接时显示） -->
              <div id="weixin-login-section">
                <div class="settings-model-group">
                  <div class="settings-model-group-header">
                    <span class="settings-model-group-icon">📱</span>
                    <div class="settings-model-group-title">
                      <span class="settings-model-group-name">扫码登录</span>
                      <span class="settings-model-group-desc">使用微信扫描二维码连接个人号</span>
                    </div>
                  </div>
                  <div class="settings-model-group-body" style="text-align:center;">
                    <!-- QR 码展示区 -->
                    <div id="weixin-qr-container" style="display:none;margin:12px auto;">
                      <img id="weixin-qr-img" style="max-width:256px;max-height:256px;border-radius:8px;border:1px solid var(--color-border);" alt="QR Code" />
                      <div id="weixin-qr-status" style="margin-top:8px;font-size:0.85rem;color:var(--color-text-secondary);">等待扫码...</div>
                    </div>
                    <button id="weixin-qr-login-btn" class="primary-btn" style="margin-top:8px;">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="vertical-align:-2px;margin-right:4px;">
                        <rect x="3" y="3" width="7" height="7" />
                        <rect x="14" y="3" width="7" height="7" />
                        <rect x="3" y="14" width="7" height="7" />
                        <rect x="14" y="14" width="7" height="7" />
                      </svg>
                      获取微信登录二维码
                    </button>
                  </div>
                </div>
              </div>

              <!-- DM 策略 -->
              <div class="settings-section-title">消息策略</div>
              <div class="settings-item">
                <div class="settings-item-info">
                  <span class="settings-item-label">私聊消息</span>
                  <span class="settings-item-desc">控制哪些用户的私聊消息会被转发给 Agent</span>
                </div>
                <select id="weixin-dm-policy" class="settings-select" style="width:130px;">
                  <option value="open">全部接受</option>
                  <option value="allowlist">仅白名单</option>
                  <option value="disabled">关闭</option>
                </select>
              </div>

              <!-- 白名单 -->
              <div id="weixin-allowlist-section" class="settings-item settings-item-column" style="display:none;">
                <div class="settings-item-info">
                  <span class="settings-item-label">白名单用户</span>
                  <span class="settings-item-desc">每行一个微信用户 ID（iLink user_id）</span>
                </div>
                <textarea id="weixin-allowed-users" class="settings-input" rows="4" placeholder="每行一个用户 ID" style="margin-top:6px;font-family:monospace;font-size:12px;"></textarea>
              </div>

              <!-- 保存按钮 -->
              <div class="settings-save-row">
                <span id="weixin-save-hint" class="settings-save-hint"></span>
                <button id="weixin-test-btn" class="secondary-btn settings-save-btn" style="margin-right:8px;">测试连接</button>
                <button id="weixin-save-btn" class="primary-btn settings-save-btn">保存配置</button>
              </div>
            </div>
            <!-- 进化 Tab -->
            <div class="settings-body settings-tab-content" id="settings-tab-evolution" data-tab="evolution">
              <!-- 统计信息 -->
              <div class="settings-section-title" style="margin-top:0; padding-top:0; border-top:none;">进化统计</div>
              <div class="evo-stats-row" id="evo-settings-stats">
                <div class="evo-stats-card">
                  <div class="evo-stats-num" id="evo-stat-skills">0</div>
                  <div class="evo-stats-label">已安装技能</div>
                </div>
                <div class="evo-stats-card">
                  <div class="evo-stats-num" id="evo-stat-tools">0</div>
                  <div class="evo-stats-label">自定义工具</div>
                </div>
                <div class="evo-stats-card">
                  <div class="evo-stats-num" id="evo-stat-forged">0</div>
                  <div class="evo-stats-label">锻造技能 <span class="evo-tag evo-tag-beta">Beta</span></div>
                </div>
              </div>

              <!-- 已安装技能 -->
              <div class="settings-section-title">📚 已安装技能</div>
              <div id="evo-skills-list" class="evo-list">
                <div class="evo-empty-hint">暂无已安装技能<br><span class="evo-sub-hint">在对话中让 Agent 搜索并安装技能</span></div>
              </div>

              <!-- 自定义工具 -->
              <div class="settings-section-title">🛠️ 自定义工具</div>
              <div id="evo-tools-list" class="evo-list">
                <div class="evo-empty-hint">暂无自定义工具<br><span class="evo-sub-hint">在对话中让 Agent 创建工具</span></div>
              </div>

              <!-- 锻造技能 -->
              <div class="settings-section-title">✨ 锻造技能 <span class="evo-tag evo-tag-beta">Beta</span></div>
              <div id="evo-forged-list" class="evo-list">
                <div class="evo-empty-hint">暂无锻造技能<br><span class="evo-sub-hint">多轮对话后 Agent 会自动分析并建议技能</span></div>
              </div>

              <!-- 刷新按钮 -->
              <div class="settings-save-row">
                <span id="evo-refresh-hint" class="settings-save-hint"></span>
                <button id="evo-refresh-btn" class="primary-btn settings-save-btn">刷新数据</button>
              </div>
            </div>
`;
