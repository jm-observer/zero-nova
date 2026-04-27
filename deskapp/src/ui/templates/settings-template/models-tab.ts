export const SETTINGS_TEMPLATE_MODELS_TAB = `
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

              <div id="server-provider-keys" class="settings-provider-keys" data-item-id="provider-keys">
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
                  <div id="agent-model-list" class="agent-model-list" data-item-id="agent-model-list"></div>
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
`;
