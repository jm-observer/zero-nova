export const SETTINGS_TEMPLATE_MEMORY_TAB = `
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
                <input type="text" id="memory-search-input" class="memory-search-input" data-item-id="memory-search-input"
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
`;
