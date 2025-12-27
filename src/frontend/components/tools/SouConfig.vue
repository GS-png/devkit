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
  // 代理配置
  proxy_enabled: false,
  proxy_host: '127.0.0.1',
  proxy_port: 7890,
  proxy_type: 'http' as 'http' | 'socks5',
})

const loadingConfig = ref(false)

// 代理检测和测速状态
interface DetectedProxy {
  host: string
  port: number
  proxy_type: string
  response_time_ms: number | null
}

interface SpeedTestMetric {
  name: string
  metric_type: string
  proxy_time_ms: number | null
  direct_time_ms: number | null
  success: boolean
  error: string | null
}

interface SpeedTestResult {
  mode: string
  proxy_info: DetectedProxy | null
  metrics: SpeedTestMetric[]
  timestamp: string
  recommendation: string
  success: boolean
}

const proxyDetecting = ref(false)
const detectedProxies = ref<DetectedProxy[]>([])
const proxyTesting = ref(false)
const speedTestResult = ref<SpeedTestResult | null>(null)
const speedTestMode = ref<'proxy' | 'direct' | 'compare'>('compare')
const speedTestQuery = ref('代码搜索测试')

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
      watch_debounce_minutes: Math.round((res.watch_debounce_ms || 180000) / 60000),
      // 代理配置
      proxy_enabled: res.proxy_enabled || false,
      proxy_host: res.proxy_host || '127.0.0.1',
      proxy_port: res.proxy_port || 7890,
      proxy_type: res.proxy_type || 'http',
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
        watchDebounceMs: config.value.watch_debounce_minutes * 60000,
        // 代理配置
        proxyEnabled: config.value.proxy_enabled,
        proxyHost: config.value.proxy_host,
        proxyPort: config.value.proxy_port,
        proxyType: config.value.proxy_type,
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

// --- 代理检测和测速函数 ---

/** 自动检测本地代理 */
async function detectProxy() {
  proxyDetecting.value = true
  detectedProxies.value = []
  try {
    const proxies = await invoke('detect_acemcp_proxy') as DetectedProxy[]
    detectedProxies.value = proxies
    
    if (proxies.length === 0) {
      message.warning('未检测到本地代理，请手动输入')
    } else if (proxies.length === 1) {
      // 自动填充
      config.value.proxy_host = proxies[0].host
      config.value.proxy_port = proxies[0].port
      config.value.proxy_type = proxies[0].proxy_type as 'http' | 'socks5'
      message.success(`已检测到代理 ${proxies[0].host}:${proxies[0].port}，建议测速验证`)
    } else {
      // 多个代理，选择响应最快的
      config.value.proxy_host = proxies[0].host
      config.value.proxy_port = proxies[0].port
      config.value.proxy_type = proxies[0].proxy_type as 'http' | 'socks5'
      message.success(`检测到 ${proxies.length} 个代理，已选择最快的 ${proxies[0].host}:${proxies[0].port}`)
    }
  } catch (err) {
    message.error(`代理检测失败: ${err}`)
  } finally {
    proxyDetecting.value = false
  }
}

/** 代理测速 */
async function runSpeedTest() {
  // 前置条件检查
  if (!config.value.base_url) {
    message.error('请先配置租户地址')
    return
  }
  if (!config.value.token) {
    message.error('请先配置 ACE Token')
    return
  }
  
  proxyTesting.value = true
  speedTestResult.value = null
  
  try {
    const result = await invoke('test_acemcp_proxy_speed', {
      testMode: speedTestMode.value,
      proxyHost: config.value.proxy_host,
      proxyPort: config.value.proxy_port,
      proxyType: config.value.proxy_type,
      testQuery: speedTestQuery.value,
      projectRootPath: '', // 可以留空，测速主要测试网络连接
    }) as SpeedTestResult
    
    speedTestResult.value = result
    
    if (result.success) {
      message.success('测速完成')
    } else {
      message.warning('测速完成，部分测试失败')
    }
  } catch (err) {
    message.error(`测速失败: ${err}`)
  } finally {
    proxyTesting.value = false
  }
}

/** 计算性能差异百分比 */
function calcDiff(proxyMs: number | null, directMs: number | null): string {
  if (proxyMs === null || directMs === null) return '-'
  if (directMs === 0) return '-'
  const diff = ((directMs - proxyMs) / directMs * 100).toFixed(0)
  if (Number(diff) > 0) return `⬇️${diff}%`
  if (Number(diff) < 0) return `⬆️${Math.abs(Number(diff))}%`
  return '0%'
}

/** 获取差异颜色 */
function getDiffColor(proxyMs: number | null, directMs: number | null): string {
  if (proxyMs === null || directMs === null) return 'inherit'
  if (proxyMs < directMs) return '#22c55e' // 绿色 - 提升
  if (proxyMs > directMs) return '#ef4444' // 红色 - 下降
  return 'inherit'
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

            <!-- 代理设置 -->
            <ConfigSection title="代理设置" description="配置 HTTP/HTTPS 代理以优化网络连接">
              <n-space vertical size="medium">
                <!-- 启用代理开关 -->
                <div class="flex items-center justify-between py-2 px-3 rounded-lg bg-gradient-to-r from-slate-50 to-slate-100 dark:from-slate-800 dark:to-slate-700">
                  <div class="flex items-center gap-3">
                    <div class="i-carbon-network-3 text-lg text-blue-500" />
                    <div>
                      <div class="font-medium text-sm">启用代理</div>
                      <div class="text-xs text-gray-500">所有 ACE API 请求将通过代理</div>
                    </div>
                  </div>
                  <n-switch v-model:value="config.proxy_enabled" :round="false" />
                </div>

                <!-- 代理配置表单 -->
                <n-grid :x-gap="16" :y-gap="12" :cols="12">
                  <n-grid-item :span="5">
                    <n-form-item label="代理地址" size="small">
                      <n-input 
                        v-model:value="config.proxy_host" 
                        placeholder="127.0.0.1"
                        :disabled="!config.proxy_enabled"
                        clearable
                      />
                    </n-form-item>
                  </n-grid-item>
                  <n-grid-item :span="3">
                    <n-form-item label="端口" size="small">
                      <n-input-number 
                        v-model:value="config.proxy_port" 
                        :min="1" 
                        :max="65535"
                        :disabled="!config.proxy_enabled"
                        class="w-full"
                      />
                    </n-form-item>
                  </n-grid-item>
                  <n-grid-item :span="4">
                    <n-form-item label="类型" size="small">
                      <n-select 
                        v-model:value="config.proxy_type"
                        :options="[
                          { label: 'HTTP', value: 'http' },
                          { label: 'SOCKS5', value: 'socks5' },
                        ]"
                        :disabled="!config.proxy_enabled"
                      />
                    </n-form-item>
                  </n-grid-item>
                </n-grid>

                <!-- 操作按钮 -->
                <div class="flex gap-3">
                  <n-button 
                    secondary 
                    size="small"
                    :loading="proxyDetecting"
                    :disabled="!config.proxy_enabled"
                    @click="detectProxy"
                  >
                    <template #icon><div class="i-carbon-search" /></template>
                    自动检测
                  </n-button>
                  <n-button 
                    secondary 
                    size="small"
                    :loading="proxyTesting"
                    :disabled="!config.proxy_enabled || !config.base_url || !config.token"
                    @click="runSpeedTest"
                  >
                    <template #icon><div class="i-carbon-rocket" /></template>
                    测速
                  </n-button>
                </div>

                <!-- 检测到的代理列表 -->
                <n-collapse-transition :show="detectedProxies.length > 1">
                  <div class="mt-2 p-3 rounded-lg bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800">
                    <div class="text-xs font-medium text-blue-600 dark:text-blue-400 mb-2">检测到 {{ detectedProxies.length }} 个可用代理</div>
                    <div class="flex flex-wrap gap-2">
                      <n-tag 
                        v-for="(p, idx) in detectedProxies" 
                        :key="idx"
                        :type="config.proxy_port === p.port ? 'success' : 'default'"
                        size="small"
                        class="cursor-pointer"
                        @click="() => { config.proxy_host = p.host; config.proxy_port = p.port; config.proxy_type = p.proxy_type as 'http' | 'socks5' }"
                      >
                        {{ p.host }}:{{ p.port }} ({{ p.response_time_ms }}ms)
                      </n-tag>
                    </div>
                  </div>
                </n-collapse-transition>

                <!-- 测速结果 -->
                <n-collapse-transition :show="speedTestResult !== null">
                  <div v-if="speedTestResult" class="mt-2 p-4 rounded-lg bg-gradient-to-br from-slate-50 to-slate-100 dark:from-slate-800 dark:to-slate-700 border border-slate-200 dark:border-slate-600">
                    <div class="flex items-center justify-between mb-3">
                      <div class="text-sm font-medium">测速结果</div>
                      <n-tag :type="speedTestResult.success ? 'success' : 'warning'" size="small">
                        {{ speedTestResult.success ? '测试成功' : '部分失败' }}
                      </n-tag>
                    </div>
                    
                    <!-- 指标表格 -->
                    <div class="space-y-2">
                      <div 
                        v-for="(metric, idx) in speedTestResult.metrics" 
                        :key="idx"
                        class="flex items-center justify-between p-2 rounded bg-white dark:bg-slate-900"
                      >
                        <div class="flex items-center gap-2">
                          <span class="text-base">{{ metric.name.split(' ')[0] }}</span>
                          <span class="text-sm">{{ metric.name.split(' ').slice(1).join(' ') }}</span>
                        </div>
                        <div class="flex items-center gap-4 text-sm">
                          <div v-if="speedTestResult.mode !== 'direct'" class="text-center">
                            <div class="text-xs text-gray-500">代理</div>
                            <div :class="metric.proxy_time_ms !== null ? 'text-blue-600 font-medium' : 'text-gray-400'">
                              {{ metric.proxy_time_ms !== null ? `${metric.proxy_time_ms}ms` : '-' }}
                            </div>
                          </div>
                          <div v-if="speedTestResult.mode !== 'proxy'" class="text-center">
                            <div class="text-xs text-gray-500">直连</div>
                            <div :class="metric.direct_time_ms !== null ? 'text-orange-600 font-medium' : 'text-gray-400'">
                              {{ metric.direct_time_ms !== null ? `${metric.direct_time_ms}ms` : '-' }}
                            </div>
                          </div>
                          <div v-if="speedTestResult.mode === 'compare'" class="text-center min-w-[50px]">
                            <div class="text-xs text-gray-500">差异</div>
                            <div 
                              class="font-medium"
                              :style="{ color: getDiffColor(metric.proxy_time_ms, metric.direct_time_ms) }"
                            >
                              {{ calcDiff(metric.proxy_time_ms, metric.direct_time_ms) }}
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>

                    <!-- 推荐建议 -->
                    <div class="mt-3 pt-3 border-t border-slate-200 dark:border-slate-600">
                      <div class="text-sm">{{ speedTestResult.recommendation }}</div>
                    </div>
                  </div>
                </n-collapse-transition>
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
