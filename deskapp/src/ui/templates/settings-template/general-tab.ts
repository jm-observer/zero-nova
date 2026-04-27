export const SETTINGS_TEMPLATE_GENERAL_TAB = `
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
`;
