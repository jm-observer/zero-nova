export const SETTINGS_TEMPLATE_TOOLS_TAB = `
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
              <div id="mcp-servers-list" data-item-id="mcp-servers-list"></div>

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
`;
