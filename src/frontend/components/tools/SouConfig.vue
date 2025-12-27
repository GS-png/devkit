<script setup lang="ts">
/**
 * 代码搜索工具 (Acemcp/Sou) 配置组件
 * 包含：基础配置、高级配置、日志调试、索引管理
 */
import { invoke } from '@tauri-apps/api/core'
import { useMessage } from 'naive-ui'
import { onMounted, ref, watch } from 'vue'
import { useAcemcpSync } from '../../composables/useAcemcpSync'
import ProjectIndexManager from '../settings/ProjectIndexManager.vue'
import ConfigSection from '../common/ConfigSection.vue'

// Props
const props = defineProps<{
  active: boolean
}>()

const message = useMessage()

// Acemcp 同步状态
const {
  autoIndexEnabled,
  fetchAutoIndexEnabled,
  setAutoIndexEnabled,
  fetchWatchingProjects,
} = useAcemcpSync()

// 配置状态
const config = ref({
  base_url: '',
  token: '',
  batch_size: 10,
  max_lines_per_blob: 800,
  text_extensions: [] as string[],
  exclude_patterns: [] as string[],
  watch_debounce_minutes: 3, // 文件监听防抖延迟（分钟），默认 3 分钟
})

const loadingConfig = ref(false)

// 调试状态
const debugProjectRoot = ref('')
const debugQuery = ref('')
const debugResult = ref('')
const debugLoading = ref(false)

// 选项数据
const extOptions = ref([
  '.py', '.js', '.ts', '.jsx', '.tsx', '.java', '.go', '.rs', 
  '.cpp', '.c', '.h', '.hpp', '.cs', '.rb', '.php', '.md', 
  '.txt', '.json', '.yaml', '.yml', '.toml', '.xml', '.html', 
  '.css', '.scss', '.sql', '.sh', '.bash'
].map(v => ({ label: v, value: v })))

const excludeOptions = ref([
  '.venv', 'venv', '.env', 'env', 'node_modules', '.next', '.nuxt', 
  '.output', 'out', '.cache', '.turbo', '.vercel', '.netlify', 
  '.swc', '.vite', '.parcel-cache', '.sass-cache', '.eslintcache', 
  '.stylelintcache', 'coverage', '.nyc_output', 'tmp', 'temp', 
  '.tmp', '.temp', '.git', '.svn', '.hg', '__pycache__', 
  '.pytest_cache', '.mypy_cache', '.tox', '.eggs', '*.egg-info', 
  'dist', 'build', '.idea', '.vscode', '.DS_Store', '*.pyc', 
  '*.pyo', '*.pyd', '.Python', 'pip-log.txt', 
  'pip-delete-this-directory.txt', '.coverage', 'htmlcov', 
  '.gradle', 'target', 'bin', 'obj'
].map(v => ({ label: v, value: v })))

// --- 操作函数 ---

async function loadAcemcpConfig() {
  loadingConfig.value = true
  try {
    const res = await invoke('get_acemcp_config') as any
    
    config.value = {
      base_url: res.base_url || '',
      token: res.token || '',
      batch_size: res.batch_size,
      max_lines_per_blob: res.max_lines_per_blob,
      text_extensions: res.text_extensions,
      exclude_patterns: res.exclude_patterns,
      watch_debounce_minutes: Math.round((res.watch_debounce_ms || 180000) / 60000), // 毫秒转分钟
    }

    // 确保选项存在
    const extSet = new Set(extOptions.value.map(o => o.value))
    for (const v of config.value.text_extensions) {
      if (!extSet.has(v)) extOptions.value.push({ label: v, value: v })
    }
    const exSet = new Set(excludeOptions.value.map(o => o.value))
    for (const v of config.value.exclude_patterns) {
      if (!exSet.has(v)) excludeOptions.value.push({ label: v, value: v })
    }
  } catch (err) {
    message.error(`加载配置失败: ${err}`)
  } finally {
    loadingConfig.value = false
  }
}

async function saveConfig() {
  try {
    if (!config.value.base_url || !/^https?:\/\//i.test(config.value.base_url)) {
      message.error('URL无效，需以 http(s):// 开头')
      return
    }
    
    await invoke('save_acemcp_config', {
      args: {
        baseUrl: config.value.base_url,
        token: config.value.token,
        batchSize: config.value.batch_size,
        maxLinesPerBlob: config.value.max_lines_per_blob,
        textExtensions: config.value.text_extensions,
        excludePatterns: config.value.exclude_patterns,
        watchDebounceMs: config.value.watch_debounce_minutes * 60000, // 分钟转毫秒
      },
    })
    message.success('配置已保存')
  } catch (err) {
    message.error(`保存失败: ${err}`)
  }
}

async function testConnection() {
  const loadingMsg = message.loading('正在测试连接...', { duration: 0 })
  try {
    const result = await invoke('test_acemcp_connection', {
      args: {
        baseUrl: config.value.base_url,
        token: config.value.token,
      },
    }) as { success: boolean; message: string }

    if (result.success) {
      message.success(result.message)
    } else {
      message.error(result.message)
    }
  } catch (err) {
    message.error(`连接测试失败: ${err}`)
  } finally {
    loadingMsg.destroy()
  }
}

async function runToolDebug() {
  if (!debugProjectRoot.value || !debugQuery.value) {
    message.warning('请填写项目路径和查询语句')
    return
  }
  
  debugLoading.value = true
  debugResult.value = ''
  
  try {
    const result = await invoke('debug_acemcp_search', {
      projectRootPath: debugProjectRoot.value,
      query: debugQuery.value,
    }) as { success: boolean; result?: string; error?: string }

    if (result.success) {
      debugResult.value = result.result || '无返回结果'
      message.success('调试执行成功')
    } else {
      debugResult.value = result.error || '执行出错'
      message.error(result.error || '调试失败')
    }
  } catch (e: any) {
    const msg = e?.message || String(e)
    debugResult.value = `Error: ${msg}`
    message.error(`调试异常: ${msg}`)
  } finally {
    debugLoading.value = false
  }
}

async function viewLogs() {
  try {
    const lines = await invoke('read_acemcp_logs') as string[]
    if (lines.length > 0) {
      await navigator.clipboard.writeText(lines.join('\n'))
      message.success(`已复制 ${lines.length} 行日志`)
    } else {
      message.info('日志为空')
    }
  } catch (e) {
    message.error(`读取日志失败: ${e}`)
  }
}

async function clearCache() {
  try {
    message.loading('正在清除...')
    const res = await invoke('clear_acemcp_cache') as string
    message.success(res)
  } catch (e) {
    message.error(`清除失败: ${e}`)
  }
}

async function toggleAutoIndex() {
  try {
    await setAutoIndexEnabled(!autoIndexEnabled.value)
    message.success(`自动索引已${autoIndexEnabled.value ? '启用' : '禁用'}`)
  } catch (e) {
    message.error(String(e))
  }
}

// 监听扩展名变化，自动规范化
watch(() => config.value.text_extensions, (list) => {
  const norm = Array.from(new Set((list || []).map(s => {
    const t = s.trim().toLowerCase()
    return t ? (t.startsWith('.') ? t : `.${t}`) : ''
  }).filter(Boolean)))
  
  if (norm.join(',') !== list.join(',')) {
    config.value.text_extensions = norm
  }
}, { deep: true })

// 组件挂载
onMounted(async () => {
  if (props.active) {
    await loadAcemcpConfig()
    await Promise.all([
      fetchAutoIndexEnabled(),
      fetchWatchingProjects()
    ])
  }
})

defineExpose({ saveConfig })
</script>

<template>
  <div class="sou-config">
    <n-tabs type="line" animated>
      <!-- 基础配置 -->
      <n-tab-pane name="basic" tab="基础配置">
        <n-scrollbar class="tab-scrollbar">
          <n-space vertical size="large" class="tab-content">
            <ConfigSection title="连接设置" description="配置代码搜索服务的连接信息">
              <n-grid :x-gap="24" :y-gap="16" :cols="1">
                <n-grid-item>
                  <n-form-item label="API端点URL">
                    <n-input v-model:value="config.base_url" placeholder="https://api.example.com" clearable />
                  </n-form-item>
                </n-grid-item>
                <n-grid-item>
                  <n-form-item label="认证令牌">
                    <n-input
                      v-model:value="config.token"
                      type="password"
                      show-password-on="click"
                      placeholder="输入认证令牌"
                      clearable
                    />
                  </n-form-item>
                </n-grid-item>
              </n-grid>
            </ConfigSection>

            <ConfigSection title="性能参数" description="调整处理批量和文件大小限制">
              <n-grid :x-gap="24" :cols="2">
                <n-grid-item>
                  <n-form-item label="批处理大小">
                    <n-input-number v-model:value="config.batch_size" :min="1" :max="100" class="w-full" />
                  </n-form-item>
                </n-grid-item>
                <n-grid-item>
                  <n-form-item label="最大行数/块">
                    <n-input-number v-model:value="config.max_lines_per_blob" :min="100" :max="5000" class="w-full" />
                  </n-form-item>
                </n-grid-item>
              </n-grid>
            </ConfigSection>
            
            <div class="flex justify-end">
              <n-button type="primary" @click="saveConfig">
                <template #icon><div class="i-carbon-save" /></template>
                保存配置
              </n-button>
            </div>
          </n-space>
        </n-scrollbar>
      </n-tab-pane>

      <!-- 高级配置 -->
      <n-tab-pane name="advanced" tab="高级配置">
        <n-scrollbar class="tab-scrollbar">
          <n-space vertical size="large" class="tab-content">
            <ConfigSection title="文件过滤" description="设置需索引的文件类型和排除规则">
              <n-space vertical size="medium">
                <n-form-item label="包含扩展名">
                  <n-select
                    v-model:value="config.text_extensions"
                    :options="extOptions"
                    multiple tag filterable clearable
                    placeholder="输入或选择扩展名 (.py)"
                  />
                  <template #feedback>
                    <span class="form-feedback">小写，点开头，自动去重</span>
                  </template>
                </n-form-item>

                <n-form-item label="排除模式">
                  <n-select
                    v-model:value="config.exclude_patterns"
                    :options="excludeOptions"
                    multiple tag filterable clearable
                    placeholder="输入或选择排除模式 (node_modules)"
                  />
                  <template #feedback>
                    <span class="form-feedback">支持 glob 通配符</span>
                  </template>
                </n-form-item>
              </n-space>
            </ConfigSection>

            <div class="flex justify-end">
              <n-button type="primary" @click="saveConfig">
                <template #icon><div class="i-carbon-save" /></template>
                保存配置
              </n-button>
            </div>
          </n-space>
        </n-scrollbar>
      </n-tab-pane>

      <!-- 日志与调试 -->
      <n-tab-pane name="debug" tab="日志与调试">
        <n-scrollbar class="tab-scrollbar">
          <n-space vertical size="large" class="tab-content">
            <ConfigSection title="工具状态" :no-card="true">
              <n-alert type="info" :bordered="false" class="info-alert">
                <template #icon><div class="i-carbon-terminal" /></template>
                日志路径: <code class="code-inline">~/.sanshu/log/acemcp.log</code>
              </n-alert>
              
              <n-space class="mt-3">
                <n-button size="small" secondary @click="testConnection">
                  <template #icon><div class="i-carbon-connection-signal" /></template>
                  测试连接
                </n-button>
                <n-button size="small" secondary @click="viewLogs">
                  <template #icon><div class="i-carbon-document" /></template>
                  查看日志
                </n-button>
                <n-button size="small" secondary @click="clearCache">
                  <template #icon><div class="i-carbon-clean" /></template>
                  清除缓存
                </n-button>
              </n-space>
            </ConfigSection>

            <ConfigSection title="搜索调试" description="模拟搜索请求以验证配置">
              <n-space vertical size="medium">
                <n-form-item label="项目根路径" :show-feedback="false">
                  <n-input v-model:value="debugProjectRoot" placeholder="/abs/path/to/project" />
                </n-form-item>
                <n-form-item label="查询语句" :show-feedback="false">
                  <n-input v-model:value="debugQuery" type="textarea" :rows="2" placeholder="输入搜索意图..." />
                </n-form-item>
                
                <n-button
                  type="primary"
                  ghost
                  :loading="debugLoading"
                  :disabled="!debugProjectRoot || !debugQuery"
                  @click="runToolDebug"
                >
                  <template #icon><div class="i-carbon-play" /></template>
                  运行调试
                </n-button>

                <div v-if="debugResult" class="debug-result">
                  <div class="result-label">结果输出:</div>
                  <div class="result-content">{{ debugResult }}</div>
                </div>
              </n-space>
            </ConfigSection>
          </n-space>
        </n-scrollbar>
      </n-tab-pane>

      <!-- 索引管理 -->
      <n-tab-pane name="index" tab="索引管理">
        <n-scrollbar class="tab-scrollbar">
          <n-space vertical size="large" class="tab-content">
            <ConfigSection title="全局策略">
              <div class="auto-index-toggle">
                <div class="toggle-info">
                  <div class="toggle-icon">
                    <div class="i-carbon-automatic w-5 h-5 text-primary-500" />
                  </div>
                  <div>
                    <div class="toggle-title">自动索引</div>
                    <div class="toggle-desc">文件变更时自动更新索引</div>
                  </div>
                </div>
                <n-switch :value="autoIndexEnabled" @update:value="toggleAutoIndex" />
              </div>

              <n-divider class="my-3" />

              <n-form-item label="防抖延迟时间" :show-feedback="false">
                <div class="debounce-input-wrapper">
                  <n-input-number
                    v-model:value="config.watch_debounce_minutes"
                    :min="1"
                    :max="30"
                    :step="1"
                    class="debounce-input"
                  />
                  <span class="debounce-unit">分钟</span>
                </div>
                <template #label>
                  <div class="form-label-with-desc">
                    <span>防抖延迟时间</span>
                    <n-tooltip trigger="hover">
                      <template #trigger>
                        <div class="i-carbon-help text-xs opacity-50 ml-1" />
                      </template>
                      文件修改后等待指定时间无新修改才触发索引更新
                    </n-tooltip>
                  </div>
                </template>
              </n-form-item>

              <div class="flex justify-end mt-3">
                <n-button type="primary" size="small" @click="saveConfig">
                  <template #icon><div class="i-carbon-save" /></template>
                  保存配置
                </n-button>
              </div>
            </ConfigSection>

            <n-scrollbar class="project-list-scrollbar">
              <ProjectIndexManager />
            </n-scrollbar>
          </n-space>
        </n-scrollbar>
      </n-tab-pane>
    </n-tabs>
  </div>
</template>

<style scoped>
.sou-config {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.tab-scrollbar {
  max-height: 58vh;
}

.tab-content {
  padding-right: 12px;
  padding-bottom: 16px;
}

/* 表单反馈文字 */
.form-feedback {
  font-size: 11px;
  color: var(--color-on-surface-muted, #9ca3af);
}

/* 信息提示 */
.info-alert {
  border-radius: 8px;
}

/* 代码样式 */
.code-inline {
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 12px;
  font-family: ui-monospace, monospace;
  background: var(--color-container, rgba(128, 128, 128, 0.1));
}

:root.dark .code-inline {
  background: rgba(255, 255, 255, 0.1);
}

/* 调试结果 */
.debug-result {
  margin-top: 8px;
}

.result-label {
  font-size: 12px;
  color: var(--color-on-surface-secondary, #6b7280);
  margin-bottom: 6px;
}

:root.dark .result-label {
  color: #9ca3af;
}

.result-content {
  padding: 12px;
  border-radius: 8px;
  font-size: 12px;
  font-family: ui-monospace, monospace;
  white-space: pre-wrap;
  max-height: 200px;
  overflow-y: auto;
  background: var(--color-container, rgba(128, 128, 128, 0.08));
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.2));
}

:root.dark .result-content {
  background: rgba(24, 24, 28, 0.8);
  border-color: rgba(255, 255, 255, 0.08);
}

/* 自动索引开关 */
.auto-index-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.toggle-info {
  display: flex;
  align-items: center;
  gap: 12px;
}

.toggle-icon {
  padding: 8px;
  border-radius: 8px;
  background: rgba(20, 184, 166, 0.1);
}

:root.dark .toggle-icon {
  background: rgba(20, 184, 166, 0.15);
}

.toggle-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--color-on-surface, #111827);
}

:root.dark .toggle-title {
  color: #e5e7eb;
}

.toggle-desc {
  font-size: 12px;
  color: var(--color-on-surface-secondary, #6b7280);
}

:root.dark .toggle-desc {
  color: #9ca3af;
}

/* 项目列表滚动容器 */
.project-list-scrollbar {
  max-height: 55vh;
}

/* 防抖延迟输入 */
.debounce-input-wrapper {
  display: flex;
  align-items: center;
  gap: 8px;
}

.debounce-input {
  width: 100px;
}

.debounce-unit {
  font-size: 13px;
  color: var(--color-on-surface-secondary, #6b7280);
}

:root.dark .debounce-unit {
  color: #9ca3af;
}

/* 带描述的表单标签 */
.form-label-with-desc {
  display: flex;
  align-items: center;
}
</style>
