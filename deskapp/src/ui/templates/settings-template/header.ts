export const SETTINGS_TEMPLATE_HEADER = `
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
              </div>
            </div>
            <!-- Tab 切换栏 -->
            <div class="settings-tabs">
              <button class="settings-tab active" data-tab="general" data-i18n="settings.tab_general">通用</button>
              <button class="settings-tab" data-tab="models" data-i18n="settings.tab_models">模型</button>
              <button class="settings-tab" data-tab="tools" data-i18n="settings.tab_tools">工具</button>

              <button class="settings-tab" data-tab="memory" data-i18n="settings.tab_memory">记忆</button>
            </div>
            <!-- 通用设置 Tab -->
`;
