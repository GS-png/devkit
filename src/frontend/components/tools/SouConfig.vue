<script setup lang="ts">
/**
 * 代码搜索工具 (Acemcp/Sou) 配置组件
 * 包含：基础配置、高级配置、日志调试、索引管理
 */
import { invoke } from '@tauri-apps/api/core'
import { useDialog, useMessage } from 'naive-ui'
import { computed, onMounted, ref, watch } from 'vue'
import { useAcemcpSync } from '../../composables/useAcemcpSync'
import ConfigSection from '../common/ConfigSection.vue'
import ProjectIndexManager from '../settings/ProjectIndexManager.vue'

// Props
const props = defineProps<{
  active: boolean
}>()

const message = useMessage()
const dialog = useDialog()

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
  proxy_type: 'http' as 'http' | 'https' | 'socks5',
  proxy_username: '',
  proxy_password: '',
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
const extraDetectPortsText = ref('')
const proxyPickerVisible = ref(false)
const selectedProxyIndex = ref(0)

// 测速项目选择
type IndexStatus = 'idle' | 'indexing' | 'synced' | 'failed'

interface ProjectIndexStatusLite {
  project_root: string
  status: IndexStatus
  total_files: number
  last_success_time: string | null
}

const speedTestProjectRoot = ref('')
const projectPickerVisible = ref(false)
const projectPickerLoading = ref(false)
const projectPickerSelected = ref('')
const indexedProjects = ref<ProjectIndexStatusLite[]>([])

const addProjectVisible = ref(false)
const addProjectPath = ref('')
const addProjectIndexing = ref(false)

const projectUploadMode = ref<'sample' | 'full'>('sample')
const projectUploadMaxFiles = ref(200)

// 调试状态
const debugProjectRoot = ref('')
const debugQuery = ref('')
const debugResult = ref('')
const debugLoading = ref(false)

// 选项数据
const extOptions = ref([
  '.py',
  '.js',
  '.ts',
  '.jsx',
  '.tsx',
  '.java',
  '.go',
  '.rs',
  '.cpp',
  '.c',
  '.h',
  '.hpp',
  '.cs',
  '.rb',
  '.php',
  '.md',
  '.txt',
  '.json',
  '.yaml',
  '.yml',
  '.toml',
  '.xml',
  '.html',
  '.css',
  '.scss',
  '.sql',
  '.sh',
  '.bash',
].map(v => ({ label: v, value: v })))

const excludeOptions = ref([
  '.venv',
  'venv',
  '.env',
  'env',
  'node_modules',
  '.next',
  '.nuxt',
  '.output',
  'out',
  '.cache',
  '.turbo',
  '.vercel',
  '.netlify',
  '.swc',
  '.vite',
  '.parcel-cache',
  '.sass-cache',
  '.eslintcache',
  '.stylelintcache',
  'coverage',
  '.nyc_output',
  'tmp',
  'temp',
  '.tmp',
  '.temp',
  '.git',
  '.svn',
  '.hg',
  '__pycache__',
  '.pytest_cache',
  '.mypy_cache',
  '.tox',
  '.eggs',
  '*.egg-info',
  'dist',
  'build',
  '.idea',
  '.vscode',
  '.DS_Store',
  '*.pyc',
  '*.pyo',
  '*.pyd',
  '.Python',
  'pip-log.txt',
  'pip-delete-this-directory.txt',
  '.coverage',
  'htmlcov',
  '.gradle',
  'target',
  'bin',
  'obj',
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
      proxy_username: res.proxy_username || '',
      proxy_password: res.proxy_password || '',
    }

    // 确保选项存在
    const extSet = new Set(extOptions.value.map(o => o.value))
    for (const v of config.value.text_extensions) {
      if (!extSet.has(v)) {
        extOptions.value.push({ label: v, value: v })
      }
    }
    const exSet = new Set(excludeOptions.value.map(o => o.value))
    for (const v of config.value.exclude_patterns) {
      if (!exSet.has(v)) {
        excludeOptions.value.push({ label: v, value: v })
      }
    }
  }
  catch (err) {
    message.error(`加载配置失败: ${err}`)
  }
  finally {
    loadingConfig.value = false
  }
}

async function saveConfig() {
  try {
    if (!config.value.base_url || !/^https?:\/\//i.test(config.value.base_url)) {
      message.error('URL无效，需以 http(s):// 开头')
      return
    }

    // 支持用户直接粘贴完整代理地址（http(s)/socks5://user:pass@host:port）
    // 避免将完整 URL 误填入“代理地址(host)”导致后端拼接出无效代理 URL
    const proxyInput = (config.value.proxy_host || '').trim()
    if (proxyInput.includes('://')) {
      try {
        const u = new URL(proxyInput)
        const scheme = (u.protocol || '').replace(':', '')
        if (!['http', 'https', 'socks5'].includes(scheme)) {
          message.error('代理地址协议不支持，仅支持 http/https/socks5')
          return
        }

        config.value.proxy_type = scheme as 'http' | 'https' | 'socks5'
        config.value.proxy_host = u.hostname
        if (u.port) {
          config.value.proxy_port = Number(u.port)
        }
        if (u.username) {
          config.value.proxy_username = decodeURIComponent(u.username)
        }
        if (u.password) {
          config.value.proxy_password = decodeURIComponent(u.password)
        }
      }
      catch (e) {
        message.error(`代理地址格式无效: ${String(e)}`)
        return
      }
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
        proxyUsername: config.value.proxy_username,
        proxyPassword: config.value.proxy_password,
      },
    })
    message.success('配置已保存')
  }
  catch (err) {
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
    }) as {
      success: boolean
      message: string
    }

    if (result.success) {
      message.success(result.message)
    }
    else {
      message.error(result.message)
    }
  }
  catch (err) {
    message.error(`连接测试失败: ${err}`)
  }
  finally {
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
    }) as {
      success: boolean
      result?: string
      error?: string
    }

    if (result.success) {
      debugResult.value = result.result || '无返回结果'
      message.success('调试执行成功')
    }
    else {
      debugResult.value = result.error || '执行出错'
      message.error(result.error || '调试失败')
    }
  }
  catch (e: any) {
    const msg = e?.message || String(e)
    debugResult.value = `Error: ${msg}`
    message.error(`调试异常: ${msg}`)
  }
  finally {
    debugLoading.value = false
  }
}

async function viewLogs() {
  try {
    const lines = await invoke('read_acemcp_logs') as string[]
    if (lines.length > 0) {
      await navigator.clipboard.writeText(lines.join('\n'))
      message.success(`已复制 ${lines.length} 行日志`)
    }
    else {
      message.info('日志为空')
    }
  }
  catch (e) {
    message.error(`读取日志失败: ${e}`)
  }
}

async function clearCache() {
  try {
    message.loading('正在清除...')
    const res = await invoke('clear_acemcp_cache') as string
    message.success(res)
  }
  catch (e) {
    message.error(`清除失败: ${e}`)
  }
}

async function toggleAutoIndex() {
  try {
    await setAutoIndexEnabled(!autoIndexEnabled.value)
    message.success(`自动索引已${autoIndexEnabled.value ? '启用' : '禁用'}`)
  }
  catch (e) {
    message.error(String(e))
  }
}

// --- 代理检测和测速函数 ---

/** 自动检测本地代理 */
async function detectProxy() {
  proxyDetecting.value = true
  detectedProxies.value = []
  try {
    const extraPorts = parseExtraPorts(extraDetectPortsText.value)
    const proxies = await invoke('detect_acemcp_proxy', {
      extraPorts,
    }) as DetectedProxy[]
    detectedProxies.value = proxies

    if (proxies.length === 0) {
      message.warning('未检测到本地代理，请手动输入')
    }
    else if (proxies.length === 1) {
      // 自动填充
      applyProxy(proxies[0])
      message.success(`已检测到代理 ${proxies[0].host}:${proxies[0].port}，建议测速验证`)
    }
    else {
      // 多个代理：打开选择器对话框，让用户选择
      selectedProxyIndex.value = 0
      proxyPickerVisible.value = true
      message.success(`检测到 ${proxies.length} 个代理，请选择一个`)
    }
  }
  catch (err) {
    message.error(`代理检测失败: ${err}`)
  }
  finally {
    proxyDetecting.value = false
  }
}

/** 解析额外检测端口（支持逗号/空格分隔，自动去重） */
function parseExtraPorts(input: string): number[] {
  const parts = (input || '')
    .split(/[,，\s]+/g)
    .map(s => s.trim())
    .filter(Boolean)

  const nums = parts
    .map(s => Number(s))
    .filter(n => Number.isInteger(n) && n >= 1 && n <= 65535)

  return Array.from(new Set(nums))
}

/** 应用选中的代理到配置（不自动启用代理，符合“先测速再启用”流程） */
function applyProxy(p: DetectedProxy) {
  config.value.proxy_host = p.host
  config.value.proxy_port = p.port
  config.value.proxy_type = p.proxy_type as 'http' | 'https' | 'socks5'
}

function confirmProxySelection() {
  const p = detectedProxies.value[selectedProxyIndex.value]
  if (!p) {
    message.warning('请先选择一个代理')
    return
  }

  applyProxy(p)
  proxyPickerVisible.value = false
  message.success(`已选择代理 ${p.host}:${p.port}`)
}

function getProjectName(projectRoot: string): string {
  const parts = (projectRoot || '').replace(/\\/g, '/').split('/').filter(Boolean)
  return parts.length > 0 ? parts[parts.length - 1] : projectRoot
}

function formatIndexTime(ts: string | null): string {
  if (!ts) {
    return '未完成'
  }
  try {
    return new Date(ts).toLocaleString()
  }
  catch {
    return ts
  }
}

async function loadIndexedProjectsForSpeedTest() {
  projectPickerLoading.value = true
  try {
    const statusResult = await invoke<{ projects: Record<string, ProjectIndexStatusLite> }>('get_all_acemcp_index_status')
    const list = Object.values(statusResult.projects || {})
      .filter(p => (p.total_files || 0) > 0)

    indexedProjects.value = list
  }
  catch (e) {
    message.error(`加载已索引项目失败: ${e}`)
    indexedProjects.value = []
  }
  finally {
    projectPickerLoading.value = false
  }
}

async function openProjectPicker() {
  await loadIndexedProjectsForSpeedTest()

  if (indexedProjects.value.length === 0) {
    dialog.warning({
      title: '需要索引项目',
      content: '测速功能需要至少一个已索引的项目。是否现在添加项目并开始索引？',
      positiveText: '是',
      negativeText: '否',
      onPositiveClick: () => {
        addProjectVisible.value = true
      },
    })
    return
  }

  projectPickerSelected.value = speedTestProjectRoot.value || indexedProjects.value[0].project_root
  projectPickerVisible.value = true
}

async function confirmProjectSelectionAndRun() {
  if (!projectPickerSelected.value) {
    message.warning('请选择一个测试项目')
    return
  }

  speedTestProjectRoot.value = projectPickerSelected.value
  projectPickerVisible.value = false

  await runSpeedTest()
}

async function addProjectAndIndexAndRun() {
  const path = addProjectPath.value.trim()
  if (!path) {
    message.error('请输入项目根路径')
    return
  }

  addProjectIndexing.value = true
  try {
    const exists = await invoke<boolean>('check_directory_exists', {
      directoryPath: path,
    })

    if (!exists) {
      message.error('目录不存在或不可访问，请检查路径')
      return
    }

    await invoke<string>('trigger_acemcp_index_update', {
      projectRootPath: path,
    })

    message.success('索引完成')
    speedTestProjectRoot.value = path
    addProjectVisible.value = false
    addProjectPath.value = ''

    await runSpeedTest()
  }
  catch (e) {
    message.error(`索引失败: ${e}`)
  }
  finally {
    addProjectIndexing.value = false
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
  if (!speedTestProjectRoot.value) {
    await openProjectPicker()
    return
  }

  proxyTesting.value = true
  speedTestResult.value = null

  try {
    const uploadMaxFiles = projectUploadMode.value === 'sample'
      ? Math.max(1, Number(projectUploadMaxFiles.value) || 200)
      : undefined

    const result = await invoke('test_acemcp_proxy_speed', {
      testMode: speedTestMode.value,
      proxyHost: config.value.proxy_host,
      proxyPort: config.value.proxy_port,
      proxyType: config.value.proxy_type,
      proxyUsername: config.value.proxy_username,
      proxyPassword: config.value.proxy_password,
      testQuery: speedTestQuery.value,
      projectRootPath: speedTestProjectRoot.value,
      projectUploadMode: projectUploadMode.value,
      projectUploadMaxFiles: uploadMaxFiles,
    }) as SpeedTestResult

    speedTestResult.value = result

    if (result.success) {
      message.success('测速完成')
    }
    else {
      message.warning('测速完成，部分测试失败')
    }
  }
  catch (err) {
    message.error(`测速失败: ${err}`)
  }
  finally {
    proxyTesting.value = false
  }
}

// 测速按钮禁用逻辑（允许“先测速再启用代理”的流程）
const speedTestDisabled = computed(() => {
  if (!config.value.base_url || !config.value.token) {
    return true
  }
  // 仅直连模式不需要代理信息
  if (speedTestMode.value === 'direct') {
    return false
  }
  return !config.value.proxy_host || !config.value.proxy_port
})

// 测速按钮禁用原因（用于 Tooltip 提示）
const speedTestDisabledReason = computed(() => {
  if (!config.value.base_url) {
    return '请先配置租户地址'
  }
  if (!config.value.token) {
    return '请先配置 ACE Token'
  }
  if (speedTestMode.value === 'direct') {
    return ''
  }
  if (!config.value.proxy_host) {
    return '请先填写代理地址（或使用自动检测）'
  }
  if (!config.value.proxy_port) {
    return '请先填写代理端口'
  }
  return ''
})

function formatSpeedTestTime(ts: string): string {
  if (!ts) {
    return '-'
  }
  try {
    return new Date(ts).toLocaleString()
  }
  catch {
    return ts
  }
}

function buildSpeedTestReportPayload() {
  if (!speedTestResult.value) {
    return null
  }

  const uploadMaxFiles = projectUploadMode.value === 'sample'
    ? Math.max(1, Number(projectUploadMaxFiles.value) || 200)
    : undefined

  return {
    tool: 'sou',
    timestamp: speedTestResult.value.timestamp,
    mode: speedTestResult.value.mode,
    query: speedTestQuery.value,
    project: {
      root: speedTestProjectRoot.value,
      name: getProjectName(speedTestProjectRoot.value),
      upload_mode: projectUploadMode.value,
      upload_max_files: uploadMaxFiles,
    },
    proxy: speedTestResult.value.mode === 'direct'
      ? { enabled: false }
      : {
          enabled: true,
          type: config.value.proxy_type,
          host: config.value.proxy_host,
          port: config.value.proxy_port,
          username: config.value.proxy_username || undefined,
          password_set: Boolean(config.value.proxy_password),
        },
    config: {
      base_url: config.value.base_url,
      token_set: Boolean(config.value.token),
    },
    result: speedTestResult.value,
  }
}

/** 复制测速报告到剪贴板（JSON，不包含 token 与密码） */
async function copySpeedTestReport() {
  const report = buildSpeedTestReportPayload()
  if (!report) {
    message.warning('暂无测速结果可复制')
    return
  }

  try {
    await navigator.clipboard.writeText(JSON.stringify(report, null, 2))
    message.success('已复制测速报告（JSON）')
  }
  catch (e) {
    message.error(`复制失败: ${e}`)
  }
}

/** 导出测速报告到文件（JSON 下载，不包含 token 与密码） */
async function downloadSpeedTestReport() {
  const report = buildSpeedTestReportPayload()
  if (!report) {
    message.warning('暂无测速结果可导出')
    return
  }

  try {
    const ts = speedTestResult.value?.timestamp || new Date().toISOString()
    const safeTs = ts.replace(/[:.]/g, '-').replace('T', '_').replace('Z', '')
    const filename = `sou-speedtest-${safeTs}.json`

    const blob = new Blob([JSON.stringify(report, null, 2)], { type: 'application/json;charset=utf-8' })
    const url = URL.createObjectURL(blob)

    const a = document.createElement('a')
    a.href = url
    a.download = filename
    a.click()

    // 释放 URL，避免内存泄露
    setTimeout(() => URL.revokeObjectURL(url), 0)
    message.success(`已导出测速报告: ${filename}`)
  }
  catch (e) {
    message.error(`导出失败: ${e}`)
  }
}

/** 计算性能差异百分比 */
function calcDiff(proxyMs: number | null, directMs: number | null): string {
  if (proxyMs === null || directMs === null) {
    return '-'
  }
  if (directMs === 0) {
    return '-'
  }
  const diff = ((directMs - proxyMs) / directMs * 100).toFixed(0)
  if (Number(diff) > 0) {
    return `⬇️${diff}%`
  }
  if (Number(diff) < 0) {
    return `⬆️${Math.abs(Number(diff))}%`
  }
  return '0%'
}

/** 获取差异颜色 */
function getDiffColor(proxyMs: number | null, directMs: number | null): string {
  if (proxyMs === null || directMs === null) {
    return 'inherit'
  }
  if (proxyMs < directMs) {
    return '#22c55e' // 绿色 - 提升
  }
  if (proxyMs > directMs) {
    return '#ef4444' // 红色 - 下降
  }
  return 'inherit'
}

// 监听扩展名变化，自动规范化
watch(() => config.value.text_extensions, (list) => {
  const norm = Array.from(new Set((list || []).map((s) => {
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
      fetchWatchingProjects(),
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
                      <div class="text-xs text-gray-500">启用后，所有 ACE API 请求将通过代理</div>
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
                        placeholder="127.0.0.1 或 http(s)/socks5://user:pass@host:port"
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
                          { label: 'HTTPS', value: 'https' },
                          { label: 'SOCKS5', value: 'socks5' },
                        ]"
                      />
                    </n-form-item>
                  </n-grid-item>
                </n-grid>

                <!-- 代理认证（可选） -->
                <n-grid :x-gap="16" :y-gap="12" :cols="12">
                  <n-grid-item :span="6">
                    <n-form-item label="用户名（可选）" size="small">
                      <n-input
                        v-model:value="config.proxy_username"
                        placeholder="留空表示无需认证"
                        clearable
                      />
                    </n-form-item>
                  </n-grid-item>
                  <n-grid-item :span="6">
                    <n-form-item label="密码（可选）" size="small">
                      <n-input
                        v-model:value="config.proxy_password"
                        type="password"
                        show-password-on="click"
                        placeholder="留空表示无需认证"
                        clearable
                      />
                    </n-form-item>
                  </n-grid-item>
                </n-grid>

                <!-- 测速配置 -->
                <div class="p-3 rounded-lg bg-slate-50 dark:bg-slate-800/50 border border-slate-200 dark:border-slate-700">
                  <div class="text-xs font-medium text-slate-600 dark:text-slate-300 mb-2">测速配置</div>
                  <n-grid :x-gap="16" :y-gap="12" :cols="12">
                    <n-grid-item :span="4">
                      <n-form-item label="模式" size="small">
                        <n-select
                          v-model:value="speedTestMode"
                          :options="[
                            { label: '对比（代理 vs 直连）', value: 'compare' },
                            { label: '仅代理', value: 'proxy' },
                            { label: '仅直连', value: 'direct' },
                          ]"
                        />
                      </n-form-item>
                    </n-grid-item>
                    <n-grid-item :span="8">
                      <n-form-item label="测试查询" size="small">
                        <n-input
                          v-model:value="speedTestQuery"
                          type="textarea"
                          :rows="2"
                          placeholder="每行一个关键词（最多 5 行），例如：\n函数定义\n类名\n变量名"
                          clearable
                        />
                      </n-form-item>
                    </n-grid-item>
                    <n-grid-item :span="12">
                      <n-form-item label="测试项目" size="small">
                        <n-input-group>
                          <n-input
                            v-model:value="speedTestProjectRoot"
                            readonly
                            placeholder="请选择已索引项目（用于上传测速）"
                          />
                          <n-button secondary @click="openProjectPicker">
                            选择
                          </n-button>
                        </n-input-group>
                      </n-form-item>
                    </n-grid-item>
                    <n-grid-item :span="6">
                      <n-form-item label="项目上传模式" size="small">
                        <n-select
                          v-model:value="projectUploadMode"
                          :options="[
                            { label: '采样', value: 'sample' },
                            { label: '全量（可能很慢）', value: 'full' },
                          ]"
                        />
                      </n-form-item>
                    </n-grid-item>
                    <n-grid-item :span="6">
                      <n-form-item label="采样文件上限" size="small">
                        <n-input-number
                          v-model:value="projectUploadMaxFiles"
                          :min="1"
                          :disabled="projectUploadMode === 'full'"
                          class="w-full"
                        />
                      </n-form-item>
                    </n-grid-item>
                  </n-grid>
                </div>

                <!-- 检测配置 -->
                <div class="p-3 rounded-lg bg-slate-50 dark:bg-slate-800/50 border border-slate-200 dark:border-slate-700">
                  <div class="text-xs font-medium text-slate-600 dark:text-slate-300 mb-2">检测配置</div>
                  <n-form-item label="额外端口（可选）" size="small">
                    <n-input
                      v-model:value="extraDetectPortsText"
                      placeholder="例如：8888, 8081（逗号/空格分隔）"
                      clearable
                    />
                    <template #feedback>
                      <span class="form-feedback">会同时尝试 HTTP 与 SOCKS5</span>
                    </template>
                  </n-form-item>
                </div>

                <!-- 操作按钮 -->
                <div class="flex gap-3">
                  <n-button
                    secondary
                    size="small"
                    :loading="proxyDetecting"
                    :disabled="proxyDetecting"
                    @click="detectProxy"
                  >
                    <template #icon><div class="i-carbon-search" /></template>
                    自动检测
                  </n-button>
                  <n-tooltip :disabled="!speedTestDisabled">
                    <template #trigger>
                      <span class="inline-flex">
                        <n-button
                          secondary
                          size="small"
                          :loading="proxyTesting"
                          :disabled="speedTestDisabled"
                          @click="runSpeedTest"
                        >
                          <template #icon><div class="i-carbon-rocket" /></template>
                          测速
                        </n-button>
                      </span>
                    </template>
                    <span>{{ speedTestDisabledReason || '请完善测速前置条件' }}</span>
                  </n-tooltip>
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
                        @click="applyProxy(p)"
                      >
                        {{ p.host }}:{{ p.port }} ({{ p.response_time_ms }}ms)
                      </n-tag>
                    </div>
                  </div>
                </n-collapse-transition>

                <!-- 多代理选择器对话框 -->
                <n-modal
                  v-model:show="proxyPickerVisible"
                  preset="card"
                  title="选择代理"
                  :style="{ width: '520px' }"
                >
                  <n-space vertical size="medium">
                    <div class="text-xs text-gray-500">检测到多个可用代理，请选择一个用于填充配置（建议先测速再启用）。</div>
                    <n-radio-group v-model:value="selectedProxyIndex">
                      <n-space vertical size="small">
                        <n-radio
                          v-for="(p, idx) in detectedProxies"
                          :key="idx"
                          :value="idx"
                        >
                          {{ p.host }}:{{ p.port }} · {{ p.proxy_type.toUpperCase() }} · {{ p.response_time_ms ?? '-' }}ms
                        </n-radio>
                      </n-space>
                    </n-radio-group>
                    <div class="flex justify-end gap-2">
                      <n-button size="small" secondary @click="proxyPickerVisible = false">取消</n-button>
                      <n-button type="primary" size="small" @click="confirmProxySelection">使用该代理</n-button>
                    </div>
                  </n-space>
                </n-modal>

                <!-- 测速项目选择器 -->
                <n-modal
                  v-model:show="projectPickerVisible"
                  preset="card"
                  title="选择测试项目"
                  :style="{ width: '640px' }"
                >
                  <n-space vertical size="medium">
                    <div class="text-xs text-gray-500">请选择一个已索引项目用于上传测速（索引时间/文件数来自本地状态）。</div>
                    <n-radio-group v-model:value="projectPickerSelected">
                      <n-space vertical size="small">
                        <n-radio
                          v-for="p in indexedProjects"
                          :key="p.project_root"
                          :value="p.project_root"
                        >
                          {{ getProjectName(p.project_root) }} · {{ p.total_files }} 文件 · {{ formatIndexTime(p.last_success_time) }}
                        </n-radio>
                      </n-space>
                    </n-radio-group>
                    <div class="flex justify-end gap-2">
                      <n-button size="small" secondary :disabled="projectPickerLoading" @click="projectPickerVisible = false">取消</n-button>
                      <n-button size="small" secondary :disabled="projectPickerLoading" @click="addProjectVisible = true">添加项目</n-button>
                      <n-button type="primary" size="small" :loading="projectPickerLoading" @click="confirmProjectSelectionAndRun">开始测速</n-button>
                    </div>
                  </n-space>
                </n-modal>

                <!-- 添加项目并索引（完成后自动测速） -->
                <n-modal
                  v-model:show="addProjectVisible"
                  preset="card"
                  title="添加项目并索引"
                  :style="{ width: '560px' }"
                >
                  <n-space vertical size="medium">
                    <n-form-item label="项目根路径" size="small">
                      <n-input
                        v-model:value="addProjectPath"
                        placeholder="例如：D:\\workspace\\myproj"
                        clearable
                      />
                    </n-form-item>
                    <div class="text-xs text-gray-500">索引完成后将自动开始测速。</div>
                    <div class="flex justify-end gap-2">
                      <n-button size="small" secondary :disabled="addProjectIndexing" @click="addProjectVisible = false">取消</n-button>
                      <n-button type="primary" size="small" :loading="addProjectIndexing" @click="addProjectAndIndexAndRun">开始索引并测速</n-button>
                    </div>
                  </n-space>
                </n-modal>

                <!-- 测速结果 -->
                <n-collapse-transition :show="speedTestResult !== null">
                  <div v-if="speedTestResult" class="mt-2 p-4 rounded-lg bg-gradient-to-br from-slate-50 to-slate-100 dark:from-slate-800 dark:to-slate-700 border border-slate-200 dark:border-slate-600">
                    <div class="flex items-center justify-between mb-3">
                      <div class="text-sm font-medium">测速结果</div>
                      <div class="flex items-center gap-2">
                        <n-button
                          size="tiny"
                          secondary
                          :loading="proxyTesting"
                          :disabled="proxyTesting"
                          @click="runSpeedTest"
                        >
                          <template #icon><div class="i-carbon-renew" /></template>
                          重新测试
                        </n-button>
                        <n-button size="tiny" secondary @click="copySpeedTestReport">
                          <template #icon><div class="i-carbon-copy" /></template>
                          复制报告
                        </n-button>
                        <n-button size="tiny" secondary @click="downloadSpeedTestReport">
                          <template #icon><div class="i-carbon-download" /></template>
                          导出报告
                        </n-button>
                        <n-tag :type="speedTestResult.success ? 'success' : 'warning'" size="small">
                          {{ speedTestResult.success ? '测试成功' : '部分失败' }}
                        </n-tag>
                      </div>
                    </div>

                    <!-- 测试环境信息 -->
                    <div class="mb-3 p-2 rounded bg-white/60 dark:bg-slate-900/40 border border-slate-200 dark:border-slate-700">
                      <div class="text-xs text-gray-600 dark:text-gray-300 space-y-1">
                        <div>时间：{{ formatSpeedTestTime(speedTestResult.timestamp) }}</div>
                        <div>项目：<code class="code-inline">{{ speedTestProjectRoot || '（未选择）' }}</code></div>
                        <div v-if="speedTestResult.mode !== 'direct'">
                          代理：{{ config.proxy_type.toUpperCase() }} {{ config.proxy_host }}:{{ config.proxy_port }}
                          <span v-if="config.proxy_username">（用户：{{ config.proxy_username }}）</span>
                        </div>
                      </div>
                    </div>

                    <!-- 指标卡片 -->
                    <n-grid :x-gap="12" :y-gap="12" :cols="12">
                      <n-grid-item
                        v-for="(metric, idx) in speedTestResult.metrics"
                        :key="idx"
                        :span="6"
                      >
                        <div class="p-3 rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700">
                          <div class="flex items-center justify-between gap-2">
                            <div class="text-sm font-medium">
                              {{ metric.name }}
                            </div>
                            <n-tag :type="metric.success ? 'success' : 'error'" size="small">
                              {{ metric.success ? 'OK' : '失败' }}
                            </n-tag>
                          </div>

                          <div class="mt-2 flex items-end justify-between gap-3">
                            <div v-if="speedTestResult.mode !== 'direct'" class="min-w-[80px]">
                              <div class="text-xs text-gray-500">代理</div>
                              <div :class="metric.proxy_time_ms !== null ? 'text-blue-600 font-semibold' : 'text-gray-400'">
                                {{ metric.proxy_time_ms !== null ? `${metric.proxy_time_ms}ms` : '-' }}
                              </div>
                            </div>
                            <div v-if="speedTestResult.mode !== 'proxy'" class="min-w-[80px] text-right">
                              <div class="text-xs text-gray-500">直连</div>
                              <div :class="metric.direct_time_ms !== null ? 'text-orange-600 font-semibold' : 'text-gray-400'">
                                {{ metric.direct_time_ms !== null ? `${metric.direct_time_ms}ms` : '-' }}
                              </div>
                            </div>
                            <div v-if="speedTestResult.mode === 'compare'" class="min-w-[80px] text-right">
                              <div class="text-xs text-gray-500">差异</div>
                              <div
                                class="font-semibold"
                                :style="{ color: getDiffColor(metric.proxy_time_ms, metric.direct_time_ms) }"
                              >
                                {{ calcDiff(metric.proxy_time_ms, metric.direct_time_ms) }}
                              </div>
                            </div>
                          </div>

                          <div v-if="metric.error" class="mt-2 text-xs text-red-500 break-words">
                            {{ metric.error }}
                          </div>
                        </div>
                      </n-grid-item>
                    </n-grid>

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
