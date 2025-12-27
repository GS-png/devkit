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

interface SpeedTestQueryDetail {
  query: string
  proxy_time_ms: number | null
  direct_time_ms: number | null
  success: boolean
  error: string | null
}

const proxyDetecting = ref(false)
const detectedProxies = ref<DetectedProxy[]>([])
const proxyTesting = ref(false)
const speedTestResult = ref<SpeedTestResult | null>(null)
const speedTestProgress = ref('')
const speedTestMode = ref<'proxy' | 'direct' | 'compare'>('compare')
const speedTestQuery = ref('代码搜索测试')
const multiQuerySearchDetails = ref<SpeedTestQueryDetail[]>([])
const multiQueryDetailsExpanded = ref(false)

const speedTestQueries = computed(() => {
  return (speedTestQuery.value || '')
    .split(/\r?\n/g)
    .flatMap(line => line.split(';'))
    .map(s => s.trim())
    .filter(Boolean)
    .slice(0, 5)
})

const multiQuerySearchSummary = computed(() => {
  const list = multiQuerySearchDetails.value
  if (list.length <= 1) {
    return null
  }

  const proxyTimes = list.map(i => i.proxy_time_ms).filter((v): v is number => v !== null)
  const directTimes = list.map(i => i.direct_time_ms).filter((v): v is number => v !== null)

  const proxyAvg = proxyTimes.length > 0
    ? Math.round(proxyTimes.reduce((a, b) => a + b, 0) / proxyTimes.length)
    : null

  const directAvg = directTimes.length > 0
    ? Math.round(directTimes.reduce((a, b) => a + b, 0) / directTimes.length)
    : null

  return {
    total: list.length,
    proxy_avg_ms: proxyAvg,
    direct_avg_ms: directAvg,
    proxy_ok: proxyTimes.length,
    direct_ok: directTimes.length,
  }
})

const speedTestMetricsForDisplay = computed(() => {
  const r = speedTestResult.value
  if (!r) {
    return []
  }

  const metrics = r.metrics || []

  // 多查询时：逐条搜索指标会比较多，默认只展示“搜索平均 + 其他指标”
  if (multiQuerySearchSummary.value) {
    const out = metrics.filter(m => m.metric_type !== 'search')

    // 兜底：如果没有“搜索平均”，保留第一条搜索指标，避免完全看不到搜索耗时
    if (!out.some(m => m.metric_type === 'search_multi_avg')) {
      const firstSearch = metrics.find(m => m.metric_type === 'search')
      if (firstSearch) {
        out.push(firstSearch)
      }
    }

    return out
  }

  return metrics
})
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
const debugLoading = ref(false)
const debugUseManualInput = ref(false) // 是否使用手动输入模式
const debugProjectOptions = ref<{ label: string, value: string }[]>([]) // 项目选择选项
const debugProjectOptionsLoading = ref(false) // 加载项目列表中

// 调试结果增强类型
interface DebugSearchResult {
  success: boolean
  result?: string
  error?: string
  request_time: string
  response_time: string
  total_duration_ms: number
  result_count?: number
  project_path: string
  query: string
}

const debugResultData = ref<DebugSearchResult | null>(null)

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

/** 加载调试用项目选择列表 */
async function loadDebugProjectOptions() {
  debugProjectOptionsLoading.value = true
  try {
    const statusResult = await invoke<{ projects: Record<string, { project_root: string, total_files: number }> }>('get_all_acemcp_index_status')
    const list = Object.values(statusResult.projects || {})
      .filter(p => (p.total_files || 0) > 0)
      .map(p => ({
        label: `${getProjectName(p.project_root)} (${p.total_files} 文件)`,
        value: p.project_root,
      }))
    debugProjectOptions.value = list
    
    // 如果列表不为空且当前未选择项目，自动选择第一个
    if (list.length > 0 && !debugProjectRoot.value) {
      debugProjectRoot.value = list[0].value
    }
  }
  catch (e) {
    console.error('加载项目列表失败:', e)
    debugProjectOptions.value = []
  }
  finally {
    debugProjectOptionsLoading.value = false
  }
}

async function runToolDebug() {
  if (!debugProjectRoot.value || !debugQuery.value) {
    message.warning('请填写项目路径和查询语句')
    return
  }

  debugLoading.value = true
  debugResultData.value = null

  try {
    const result = await invoke<DebugSearchResult>('debug_acemcp_search', {
      projectRootPath: debugProjectRoot.value,
      query: debugQuery.value,
    })

    debugResultData.value = result

    if (result.success) {
      message.success(`调试执行成功，耗时 ${result.total_duration_ms}ms`)
    }
    else {
      message.error(result.error || '调试失败')
    }
  }
  catch (e: any) {
    const msg = e?.message || String(e)
    // 创建错误结果
    debugResultData.value = {
      success: false,
      error: msg,
      request_time: new Date().toISOString(),
      response_time: new Date().toISOString(),
      total_duration_ms: 0,
      project_path: debugProjectRoot.value,
      query: debugQuery.value,
    }
    message.error(`调试异常: ${msg}`)
  }
  finally {
    debugLoading.value = false
  }
}

/** 格式化调试时间显示 */
function formatDebugTime(isoTime: string): string {
  try {
    const date = new Date(isoTime)
    return date.toLocaleString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      fractionalSecondDigits: 3,
    })
  }
  catch {
    return isoTime
  }
}

/** 复制调试结果到剪贴板 */
async function copyDebugResult() {
  if (!debugResultData.value?.result) {
    message.warning('没有可复制的内容')
    return
  }
  try {
    await navigator.clipboard.writeText(debugResultData.value.result)
    message.success('已复制到剪贴板')
  }
  catch (e) {
    message.error(`复制失败: ${e}`)
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
  speedTestProgress.value = '正在准备测速...'
  multiQuerySearchDetails.value = []
  multiQueryDetailsExpanded.value = false

  try {
    // 与后端保持一致：按换行/分号分隔，最多 5 条
    const rawQueryCount = (speedTestQuery.value || '')
      .split(/\r?\n/g)
      .flatMap(line => line.split(';'))
      .map(s => s.trim())
      .filter(Boolean)
      .length

    if (rawQueryCount > 5) {
      message.info('测试查询过多，已按前 5 条执行')
    }

    const uploadMaxFiles = projectUploadMode.value === 'sample'
      ? Math.max(1, Number(projectUploadMaxFiles.value) || 200)
      : undefined

    // 后端已支持多查询（换行/分号分隔），因此前端只需调用一次
    const effectiveTestQuery = (speedTestQuery.value || '').trim()
      ? speedTestQuery.value
      : '代码搜索测试'

    const result = await invoke('test_acemcp_proxy_speed', {
      testMode: speedTestMode.value,
      proxyHost: config.value.proxy_host,
      proxyPort: config.value.proxy_port,
      proxyType: config.value.proxy_type,
      proxyUsername: config.value.proxy_username,
      proxyPassword: config.value.proxy_password,
      testQuery: effectiveTestQuery,
      projectRootPath: speedTestProjectRoot.value,
      projectUploadMode: projectUploadMode.value,
      projectUploadMaxFiles: uploadMaxFiles,
    }) as SpeedTestResult

    // 从后端返回的 metrics 中解析多条搜索指标（与输入顺序对齐）
    const effectiveQueries = speedTestQueries.value.length > 0
      ? speedTestQueries.value
      : ['代码搜索测试']

    const searchMetrics = (result.metrics || []).filter(m => m.metric_type === 'search')
    multiQuerySearchDetails.value = effectiveQueries.map((q, idx) => {
      const m = searchMetrics[idx]
      return {
        query: q,
        proxy_time_ms: m?.proxy_time_ms ?? null,
        direct_time_ms: m?.direct_time_ms ?? null,
        success: m?.success ?? false,
        error: m?.error ?? (m ? null : '未返回搜索指标'),
      }
    })

    // 多查询：在卡片里补充“语义搜索（平均）”指标
    const s = multiQuerySearchSummary.value
    if (s) {
      const avgMetric: SpeedTestMetric = {
        name: `🔎 语义搜索（${s.total} 条平均）`,
        metric_type: 'search_multi_avg',
        proxy_time_ms: s.proxy_avg_ms,
        direct_time_ms: s.direct_avg_ms,
        success: true,
        error: null,
      }

      // 如果某一侧完全没有成功数据，则标记为失败并给出原因
      if (speedTestMode.value !== 'direct' && s.proxy_ok === 0) {
        avgMetric.success = false
        avgMetric.error = '代理侧无有效搜索耗时（全部失败或未返回）'
      }
      if (speedTestMode.value !== 'proxy' && s.direct_ok === 0) {
        avgMetric.success = false
        avgMetric.error = [avgMetric.error, '直连侧无有效搜索耗时（全部失败或未返回）'].filter(Boolean).join('；')
      }

      result.metrics.push(avgMetric)
    }

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
    speedTestProgress.value = ''
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

/** 复制单条多查询明细到剪贴板（JSON，不包含 token 与密码） */
async function copyQueryDetail(detail: SpeedTestQueryDetail, idx: number) {
  if (!speedTestResult.value) {
    message.warning('暂无测速结果可复制')
    return
  }

  const payload = {
    tool: 'sou',
    timestamp: speedTestResult.value.timestamp,
    query_index: idx + 1,
    query: detail.query,
    mode: speedTestResult.value.mode,
    project: {
      root: speedTestProjectRoot.value,
      name: getProjectName(speedTestProjectRoot.value),
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
    metric: {
      proxy_time_ms: detail.proxy_time_ms,
      direct_time_ms: detail.direct_time_ms,
      success: detail.success,
      error: detail.error,
    },
  }

  try {
    await navigator.clipboard.writeText(JSON.stringify(payload, null, 2))
    message.success(`已复制 Q${idx + 1} 明细（JSON）`)
  }
  catch (e) {
    message.error(`复制失败: ${e}`)
  }
}

/** 复制单个指标结果到剪贴板 */
async function copyMetricResult(metric: SpeedTestMetric) {
  if (!speedTestResult.value) {
    message.warning('暂无测速结果')
    return
  }

  const report = {
    tool: 'sou',
    timestamp: speedTestResult.value.timestamp,
    mode: speedTestResult.value.mode,
    project: speedTestProjectRoot.value || '未选择',
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
    metric: {
      name: metric.name,
      type: metric.metric_type,
      proxy_time_ms: metric.proxy_time_ms,
      direct_time_ms: metric.direct_time_ms,
      success: metric.success,
      error: metric.error,
    },
  }

  try {
    await navigator.clipboard.writeText(JSON.stringify(report, null, 2))
    message.success(`已复制指标 "${metric.name}" 结果`)
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
                      <div class="font-medium text-sm">
                        启用代理
                      </div>
                      <div class="text-xs text-gray-500">
                        启用后，所有 ACE API 请求将通过代理
                      </div>
                    </div>
                  </div>
                  <n-tooltip>
                    <template #trigger>
                      <n-switch v-model:value="config.proxy_enabled" :round="false" />
                    </template>
                    <span>建议先完成测速验证后再开启；仅直连测速无需开启代理</span>
                  </n-tooltip>
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
                      <template #feedback>
                        <span class="form-feedback">
                          提示：用户名/密码会随配置保存到本地；复制/导出报告不会包含密码。
                        </span>
                      </template>
                    </n-form-item>
                  </n-grid-item>
                </n-grid>

                <!-- 测速配置 -->
                <div class="p-3 rounded-lg bg-slate-50 dark:bg-slate-800/50 border border-slate-200 dark:border-slate-700">
                  <div class="text-xs font-medium text-slate-600 dark:text-slate-300 mb-2">
                    测速配置
                  </div>
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
                        <template #feedback>
                          <span class="form-feedback">
                            每行 1 条（或用 <code class="code-inline">;</code> 分隔），最多 5 条；多条查询时：Ping/上传只测一次，语义搜索会逐条测试并汇总。
                            <span v-if="speedTestQueries.length > 1">
                              当前：{{ speedTestQueries.length }} 条
                            </span>
                          </span>
                        </template>
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
                        <template #feedback>
                          <span class="form-feedback">
                            采样更快且副作用更小；全量可能耗时较长并消耗更多网络流量。
                          </span>
                        </template>
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
                        <template #feedback>
                          <span class="form-feedback">
                            仅采样模式生效。
                          </span>
                        </template>
                      </n-form-item>
                    </n-grid-item>
                  </n-grid>
                </div>

                <!-- 检测配置 -->
                <div class="p-3 rounded-lg bg-slate-50 dark:bg-slate-800/50 border border-slate-200 dark:border-slate-700">
                  <div class="text-xs font-medium text-slate-600 dark:text-slate-300 mb-2">
                    检测配置
                  </div>
                  <n-form-item label="额外端口（可选）" size="small">
                    <n-input
                      v-model:value="extraDetectPortsText"
                      placeholder="例如：8888, 8081（逗号/空格分隔）"
                      clearable
                    />
                    <template #feedback>
                      <span class="form-feedback">
                        会同时尝试 HTTP 与 SOCKS5
                      </span>
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
                    <template #icon>
                      <div class="i-carbon-search" />
                    </template>
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
                          <template #icon>
                            <div class="i-carbon-rocket" />
                          </template>
                          测速
                        </n-button>
                      </span>
                    </template>
                    <span>{{ speedTestDisabledReason || '请完善测速前置条件' }}</span>
                  </n-tooltip>
                </div>

                <div
                  v-if="proxyTesting"
                  class="mt-2 text-xs text-gray-500 dark:text-gray-300"
                >
                  {{ speedTestProgress || `正在测速…（${speedTestQueries.length || 1} 条查询）` }}
                </div>

                <!-- 检测到的代理列表 -->
                <n-collapse-transition :show="detectedProxies.length > 1">
                  <div class="mt-2 p-3 rounded-lg bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800">
                    <div class="text-xs font-medium text-blue-600 dark:text-blue-400 mb-2">
                      检测到 {{ detectedProxies.length }} 个可用代理
                    </div>
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
                    <div class="text-xs text-gray-500">
                      检测到多个可用代理，请选择一个用于填充配置（建议先测速再启用）。
                    </div>
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
                      <n-button size="small" secondary @click="proxyPickerVisible = false">
                        取消
                      </n-button>
                      <n-button type="primary" size="small" @click="confirmProxySelection">
                        使用该代理
                      </n-button>
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
                    <div class="text-xs text-gray-500">
                      请选择一个已索引项目用于上传测速（索引时间/文件数来自本地状态）。
                    </div>
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
                      <n-button
                        size="small"
                        secondary
                        :disabled="projectPickerLoading"
                        @click="projectPickerVisible = false"
                      >
                        取消
                      </n-button>
                      <n-button
                        size="small"
                        secondary
                        :disabled="projectPickerLoading"
                        @click="addProjectVisible = true"
                      >
                        添加项目
                      </n-button>
                      <n-button
                        type="primary"
                        size="small"
                        :loading="projectPickerLoading"
                        @click="confirmProjectSelectionAndRun"
                      >
                        开始测速
                      </n-button>
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
                    <div class="text-xs text-gray-500">
                      索引完成后将自动开始测速。
                    </div>
                    <div class="flex justify-end gap-2">
                      <n-button
                        size="small"
                        secondary
                        :disabled="addProjectIndexing"
                        @click="addProjectVisible = false"
                      >
                        取消
                      </n-button>
                      <n-button
                        type="primary"
                        size="small"
                        :loading="addProjectIndexing"
                        @click="addProjectAndIndexAndRun"
                      >
                        开始索引并测速
                      </n-button>
                    </div>
                  </n-space>
                </n-modal>

                <!-- 测速结果 -->
                <n-collapse-transition :show="speedTestResult !== null">
                  <div v-if="speedTestResult" class="mt-2 p-4 rounded-lg bg-gradient-to-br from-slate-50 to-slate-100 dark:from-slate-800 dark:to-slate-700 border border-slate-200 dark:border-slate-600">
                    <div class="flex items-center justify-between mb-3">
                      <div class="text-sm font-medium">
                        测速结果
                      </div>
                      <div class="flex items-center gap-2">
                        <n-button
                          size="tiny"
                          secondary
                          :loading="proxyTesting"
                          :disabled="proxyTesting"
                          @click="runSpeedTest"
                        >
                          <template #icon>
                            <div class="i-carbon-renew" />
                          </template>
                          重新测试
                        </n-button>
                        <n-tooltip>
                          <template #trigger>
                            <n-button size="tiny" secondary @click="copySpeedTestReport">
                              <template #icon>
                                <div class="i-carbon-copy" />
                              </template>
                              复制报告
                            </n-button>
                          </template>
                          <span>报告不包含租户 Token 与代理密码</span>
                        </n-tooltip>
                        <n-tooltip>
                          <template #trigger>
                            <n-button size="tiny" secondary @click="downloadSpeedTestReport">
                              <template #icon>
                                <div class="i-carbon-download" />
                              </template>
                              导出报告
                            </n-button>
                          </template>
                          <span>导出 JSON，不包含租户 Token 与代理密码</span>
                        </n-tooltip>
                        <n-tag :type="speedTestResult.success ? 'success' : 'warning'" size="small">
                          {{ speedTestResult.success ? '测试成功' : '部分失败' }}
                        </n-tag>
                      </div>
                    </div>

                    <!-- 测试环境信息 -->
                    <div class="mb-3 p-2 rounded bg-white/60 dark:bg-slate-900/40 border border-slate-200 dark:border-slate-700">
                      <div class="text-xs text-gray-600 dark:text-gray-300 space-y-1">
                        <div>
                          时间：{{ formatSpeedTestTime(speedTestResult.timestamp) }}
                        </div>
                        <div>
                          项目：
                          <code class="code-inline">{{ speedTestProjectRoot || '（未选择）' }}</code>
                        </div>
                        <div v-if="speedTestResult.mode !== 'direct'">
                          代理：{{ config.proxy_type.toUpperCase() }} {{ config.proxy_host }}:{{ config.proxy_port }}
                          <span v-if="config.proxy_username">（用户：{{ config.proxy_username }}）</span>
                        </div>
                      </div>
                    </div>

                    <!-- 多查询搜索汇总（仅多行查询时显示） -->
                    <div
                      v-if="multiQuerySearchSummary"
                      class="mb-3 p-3 rounded-lg bg-white/60 dark:bg-slate-900/40 border border-slate-200 dark:border-slate-700"
                    >
                      <div class="flex items-center justify-between gap-2">
                        <div class="text-xs font-medium text-slate-700 dark:text-slate-200">
                          多查询搜索汇总（{{ multiQuerySearchSummary.total }} 条）
                        </div>
                        <div class="flex items-center gap-2">
                          <n-tag type="info" size="small">
                            Ping/上传只测一次
                          </n-tag>
                          <n-button
                            size="tiny"
                            quaternary
                            @click="multiQueryDetailsExpanded = !multiQueryDetailsExpanded"
                          >
                            {{ multiQueryDetailsExpanded ? '收起明细' : '展开明细' }}
                          </n-button>
                        </div>
                      </div>

                      <div class="mt-2 text-xs text-gray-600 dark:text-gray-300">
                        <span v-if="speedTestResult.mode !== 'direct'">
                          代理平均：{{ multiQuerySearchSummary.proxy_avg_ms !== null ? `${multiQuerySearchSummary.proxy_avg_ms}ms` : '-' }}
                          （成功 {{ multiQuerySearchSummary.proxy_ok }}/{{ multiQuerySearchSummary.total }}）
                        </span>
                        <span v-if="speedTestResult.mode === 'compare'"> · </span>
                        <span v-if="speedTestResult.mode !== 'proxy'">
                          直连平均：{{ multiQuerySearchSummary.direct_avg_ms !== null ? `${multiQuerySearchSummary.direct_avg_ms}ms` : '-' }}
                          （成功 {{ multiQuerySearchSummary.direct_ok }}/{{ multiQuerySearchSummary.total }}）
                        </span>
                        <span v-if="speedTestResult.mode === 'compare'">
                          · 差异：{{ calcDiff(multiQuerySearchSummary.proxy_avg_ms, multiQuerySearchSummary.direct_avg_ms) }}
                        </span>
                      </div>

                      <n-collapse-transition :show="multiQueryDetailsExpanded">
                        <div class="mt-2 space-y-2">
                          <div
                            v-for="(d, idx) in multiQuerySearchDetails"
                            :key="idx"
                            class="p-2 rounded bg-white dark:bg-slate-900 border border-slate-200/60 dark:border-slate-700/60"
                          >
                            <div class="flex items-start justify-between gap-2">
                              <div class="text-xs text-gray-500 break-words">
                                Q{{ idx + 1 }}：{{ d.query }}
                              </div>
                              <n-tooltip>
                                <template #trigger>
                                  <n-button
                                    size="tiny"
                                    quaternary
                                    @click="copyQueryDetail(d, idx)"
                                  >
                                    <template #icon>
                                      <div class="i-carbon-copy" />
                                    </template>
                                  </n-button>
                                </template>
                                <span>复制该条明细（JSON，不包含 token 与密码）</span>
                              </n-tooltip>
                            </div>

                            <div class="mt-1 flex items-center justify-between text-sm">
                              <div v-if="speedTestResult.mode !== 'direct'" class="text-blue-600">
                                代理：{{ d.proxy_time_ms !== null ? `${d.proxy_time_ms}ms` : '-' }}
                              </div>
                              <div v-if="speedTestResult.mode !== 'proxy'" class="text-orange-600 text-right">
                                直连：{{ d.direct_time_ms !== null ? `${d.direct_time_ms}ms` : '-' }}
                              </div>
                            </div>
                            <div v-if="d.error" class="mt-1 text-xs text-red-500 break-words">
                              {{ d.error }}
                            </div>
                          </div>
                        </div>
                      </n-collapse-transition>
                    </div>

                    <!-- 指标卡片 -->
                    <n-grid :x-gap="12" :y-gap="12" :cols="12">
                      <n-grid-item
                        v-for="(metric, idx) in speedTestMetricsForDisplay"
                        :key="idx"
                        :span="6"
                      >
                        <div class="p-3 rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700">
                          <div class="flex items-center justify-between gap-2">
                            <div class="text-sm font-medium">
                              {{ metric.name }}
                            </div>
                            <div class="flex items-center gap-1">
                              <n-tooltip>
                                <template #trigger>
                                  <n-button
                                    size="tiny"
                                    text
                                    @click="copyMetricResult(metric)"
                                  >
                                    <template #icon>
                                      <div class="i-carbon-copy text-gray-400 hover:text-gray-600" />
                                    </template>
                                  </n-button>
                                </template>
                                <span>复制该指标结果（JSON）</span>
                              </n-tooltip>
                              <n-tag :type="metric.success ? 'success' : 'error'" size="small">
                                {{ metric.success ? 'OK' : '失败' }}
                              </n-tag>
                            </div>
                          </div>

                          <div class="mt-2 flex items-end justify-between gap-3">
                            <div v-if="speedTestResult.mode !== 'direct'" class="min-w-[80px]">
                              <div class="text-xs text-gray-500">
                                代理
                              </div>
                              <div :class="metric.proxy_time_ms !== null ? 'text-blue-600 font-semibold' : 'text-gray-400'">
                                {{ metric.proxy_time_ms !== null ? `${metric.proxy_time_ms}ms` : '-' }}
                              </div>
                            </div>
                            <div v-if="speedTestResult.mode !== 'proxy'" class="min-w-[80px] text-right">
                              <div class="text-xs text-gray-500">
                                直连
                              </div>
                              <div :class="metric.direct_time_ms !== null ? 'text-orange-600 font-semibold' : 'text-gray-400'">
                                {{ metric.direct_time_ms !== null ? `${metric.direct_time_ms}ms` : '-' }}
                              </div>
                            </div>
                            <div v-if="speedTestResult.mode === 'compare'" class="min-w-[80px] text-right">
                              <div class="text-xs text-gray-500">
                                差异
                              </div>
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
                      <div class="text-sm">
                        {{ speedTestResult.recommendation }}
                      </div>
                    </div>
                  </div>
                </n-collapse-transition>
              </n-space>
            </ConfigSection>

            <div class="flex justify-end">
              <n-button type="primary" @click="saveConfig">
                <template #icon>
                  <div class="i-carbon-save" />
                </template>
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
                    <span class="form-feedback">
                      支持 glob 通配符
                    </span>
                  </template>
                </n-form-item>
              </n-space>
            </ConfigSection>

            <div class="flex justify-end">
              <n-button type="primary" @click="saveConfig">
                <template #icon>
                  <div class="i-carbon-save" />
                </template>
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
                <template #icon>
                  <div class="i-carbon-terminal" />
                </template>
                日志路径: <code class="code-inline">~/.sanshu/log/acemcp.log</code>
              </n-alert>

              <n-space class="mt-3">
                <n-button size="small" secondary @click="testConnection">
                  <template #icon>
                    <div class="i-carbon-connection-signal" />
                  </template>
                  测试连接
                </n-button>
                <n-button size="small" secondary @click="viewLogs">
                  <template #icon>
                    <div class="i-carbon-document" />
                  </template>
                  查看日志
                </n-button>
                <n-button size="small" secondary @click="clearCache">
                  <template #icon>
                    <div class="i-carbon-clean" />
                  </template>
                  清除缓存
                </n-button>
              </n-space>
            </ConfigSection>

            <!-- 状态信息卡片 -->
            <ConfigSection title="运行状态" :no-card="true">
              <n-grid :x-gap="12" :y-gap="12" :cols="12">
                <n-grid-item :span="4">
                  <div class="status-card">
                    <div class="status-icon">
                      <div :class="config.proxy_enabled ? 'i-carbon-checkmark-outline text-emerald-500' : 'i-carbon-close-outline text-slate-400'" />
                    </div>
                    <div class="status-info">
                      <div class="status-title">代理状态</div>
                      <div class="status-value">
                        {{ config.proxy_enabled ? '已启用' : '未启用' }}
                      </div>
                      <div v-if="config.proxy_enabled" class="status-detail">
                        {{ config.proxy_host }}:{{ config.proxy_port }}
                      </div>
                    </div>
                  </div>
                </n-grid-item>
                <n-grid-item :span="4">
                  <div class="status-card">
                    <div class="status-icon">
                      <div class="i-carbon-folder-shared text-blue-500" />
                    </div>
                    <div class="status-info">
                      <div class="status-title">索引项目</div>
                      <div class="status-value">
                        {{ debugProjectOptions.length }} 个
                      </div>
                    </div>
                  </div>
                </n-grid-item>
                <n-grid-item :span="4">
                  <div class="status-card">
                    <div class="status-icon">
                      <div :class="debugResultData?.success ? 'i-carbon-checkmark-filled text-emerald-500' : (debugResultData === null ? 'i-carbon-pending text-slate-400' : 'i-carbon-warning-alt text-amber-500')" />
                    </div>
                    <div class="status-info">
                      <div class="status-title">上次调试</div>
                      <div class="status-value">
                        {{ debugResultData ? (debugResultData.success ? '成功' : '失败') : '未执行' }}
                      </div>
                      <div v-if="debugResultData" class="status-detail">
                        {{ debugResultData.total_duration_ms }}ms
                      </div>
                    </div>
                  </div>
                </n-grid-item>
              </n-grid>
            </ConfigSection>

            <ConfigSection title="搜索调试" description="模拟搜索请求以验证配置">
              <n-space vertical size="medium">
                <!-- 项目选择 -->
                <n-form-item :show-feedback="false">
                  <template #label>
                    <div class="flex items-center gap-2">
                      <span>项目路径</span>
                      <n-button
                        text
                        size="tiny"
                        type="primary"
                        @click="debugUseManualInput = !debugUseManualInput"
                      >
                        {{ debugUseManualInput ? '选择已索引' : '手动输入' }}
                      </n-button>
                    </div>
                  </template>
                  <n-select
                    v-if="!debugUseManualInput"
                    v-model:value="debugProjectRoot"
                    :options="debugProjectOptions"
                    :loading="debugProjectOptionsLoading"
                    placeholder="选择已索引的项目..."
                    filterable
                    clearable
                    @focus="loadDebugProjectOptions"
                  />
                  <n-input
                    v-else
                    v-model:value="debugProjectRoot"
                    placeholder="/abs/path/to/project"
                    clearable
                  />
                </n-form-item>

                <n-form-item label="查询语句" :show-feedback="false">
                  <n-input
                    v-model:value="debugQuery"
                    type="textarea"
                    :rows="2"
                    placeholder="输入搜索意图..."
                  />
                </n-form-item>

                <n-button
                  type="primary"
                  ghost
                  :loading="debugLoading"
                  :disabled="!debugProjectRoot || !debugQuery"
                  @click="runToolDebug"
                >
                  <template #icon>
                    <div class="i-carbon-play" />
                  </template>
                  运行调试
                </n-button>

                <!-- 骨架屏加载态 -->
                <div v-if="debugLoading" class="debug-skeleton">
                  <n-skeleton text :repeat="3" />
                  <n-skeleton text style="width: 60%" />
                </div>

                <!-- 结构化结果展示 -->
                <n-collapse-transition :show="debugResultData !== null && !debugLoading">
                  <div v-if="debugResultData" class="debug-result-panel">
                    <!-- 请求信息 -->
                    <div class="result-section">
                      <div class="section-header">
                        <div class="i-carbon-send text-blue-500" />
                        <span>请求信息</span>
                      </div>
                      <div class="section-content">
                        <div class="info-row">
                          <span class="info-label">项目:</span>
                          <code class="info-value">{{ debugResultData.project_path }}</code>
                        </div>
                        <div class="info-row">
                          <span class="info-label">查询:</span>
                          <span class="info-value">{{ debugResultData.query }}</span>
                        </div>
                        <div class="info-row">
                          <span class="info-label">发送时间:</span>
                          <span class="info-value">{{ formatDebugTime(debugResultData.request_time) }}</span>
                        </div>
                      </div>
                    </div>

                    <!-- 性能指标 -->
                    <div class="result-section">
                      <div class="section-header">
                        <div class="i-carbon-timer text-amber-500" />
                        <span>性能指标</span>
                      </div>
                      <div class="section-content">
                        <n-grid :x-gap="12" :cols="12">
                          <n-grid-item :span="4">
                            <div class="metric-item">
                              <div class="metric-value" :class="debugResultData.success ? 'text-emerald-500' : 'text-red-500'">
                                {{ debugResultData.total_duration_ms }}ms
                              </div>
                              <div class="metric-label">总耗时</div>
                            </div>
                          </n-grid-item>
                          <n-grid-item :span="4">
                            <div class="metric-item">
                              <div class="metric-value">
                                {{ debugResultData.result_count ?? '-' }}
                              </div>
                              <div class="metric-label">结果数</div>
                            </div>
                          </n-grid-item>
                          <n-grid-item :span="4">
                            <div class="metric-item">
                              <n-tag :type="debugResultData.success ? 'success' : 'error'" size="small">
                                {{ debugResultData.success ? '成功' : '失败' }}
                              </n-tag>
                              <div class="metric-label">状态</div>
                            </div>
                          </n-grid-item>
                        </n-grid>
                      </div>
                    </div>

                    <!-- 响应数据 / 错误信息 -->
                    <div class="result-section">
                      <div class="section-header">
                        <div :class="debugResultData.success ? 'i-carbon-document text-emerald-500' : 'i-carbon-warning text-red-500'" />
                        <span>{{ debugResultData.success ? '响应数据' : '错误信息' }}</span>
                        <n-button
                          v-if="debugResultData.success && debugResultData.result"
                          size="tiny"
                          text
                          class="ml-auto"
                          @click="copyDebugResult"
                        >
                          <template #icon>
                            <div class="i-carbon-copy" />
                          </template>
                          复制
                        </n-button>
                      </div>
                      <div class="section-content">
                        <div v-if="debugResultData.error" class="error-content">
                          {{ debugResultData.error }}
                        </div>
                        <n-scrollbar v-else style="max-height: 200px">
                          <pre class="result-pre">{{ debugResultData.result || '无返回结果' }}</pre>
                        </n-scrollbar>
                      </div>
                    </div>
                  </div>
                </n-collapse-transition>
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
                    <div class="toggle-title">
                      自动索引
                    </div>
                    <div class="toggle-desc">
                      文件变更时自动更新索引
                    </div>
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
                  <template #icon>
                    <div class="i-carbon-save" />
                  </template>
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

/* 调试界面 - 状态卡片 */
.status-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-radius: 10px;
  background: var(--color-container, rgba(128, 128, 128, 0.06));
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.12));
  transition: all 0.2s ease;
}

.status-card:hover {
  border-color: rgba(99, 102, 241, 0.3);
}

:root.dark .status-card {
  background: rgba(30, 30, 35, 0.6);
  border-color: rgba(255, 255, 255, 0.08);
}

.status-icon {
  font-size: 20px;
}

.status-info {
  flex: 1;
  min-width: 0;
}

.status-title {
  font-size: 11px;
  color: var(--color-on-surface-muted, #9ca3af);
  margin-bottom: 2px;
}

.status-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--color-on-surface, #111827);
}

:root.dark .status-value {
  color: #f3f4f6;
}

.status-detail {
  font-size: 11px;
  color: var(--color-on-surface-muted, #9ca3af);
  margin-top: 2px;
  font-family: ui-monospace, monospace;
}

/* 调试界面 - 骨架屏 */
.debug-skeleton {
  padding: 16px;
  border-radius: 10px;
  background: var(--color-container, rgba(128, 128, 128, 0.06));
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.12));
}

:root.dark .debug-skeleton {
  background: rgba(30, 30, 35, 0.6);
  border-color: rgba(255, 255, 255, 0.08);
}

/* 调试界面 - 结果面板 */
.debug-result-panel {
  border-radius: 10px;
  background: var(--color-container, rgba(128, 128, 128, 0.04));
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.12));
  overflow: hidden;
}

:root.dark .debug-result-panel {
  background: rgba(20, 20, 25, 0.5);
  border-color: rgba(255, 255, 255, 0.08);
}

.result-section {
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-border, rgba(128, 128, 128, 0.1));
}

.result-section:last-child {
  border-bottom: none;
}

:root.dark .result-section {
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.section-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 500;
  color: var(--color-on-surface, #374151);
  margin-bottom: 10px;
}

:root.dark .section-header {
  color: #e5e7eb;
}

.section-content {
  font-size: 12px;
}

.info-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-bottom: 6px;
}

.info-row:last-child {
  margin-bottom: 0;
}

.info-label {
  color: var(--color-on-surface-muted, #9ca3af);
  white-space: nowrap;
  min-width: 60px;
}

.info-value {
  color: var(--color-on-surface, #374151);
  word-break: break-all;
}

:root.dark .info-value {
  color: #d1d5db;
}

/* 调试界面 - 性能指标 */
.metric-item {
  text-align: center;
  padding: 8px;
  border-radius: 8px;
  background: rgba(128, 128, 128, 0.04);
}

:root.dark .metric-item {
  background: rgba(255, 255, 255, 0.04);
}

.metric-value {
  font-size: 18px;
  font-weight: 700;
  color: var(--color-on-surface, #111827);
  margin-bottom: 4px;
}

:root.dark .metric-value {
  color: #f3f4f6;
}

.metric-label {
  font-size: 11px;
  color: var(--color-on-surface-muted, #9ca3af);
}

/* 调试界面 - 错误内容 */
.error-content {
  padding: 12px;
  border-radius: 8px;
  background: rgba(239, 68, 68, 0.08);
  color: #dc2626;
  font-size: 12px;
  line-height: 1.5;
}

:root.dark .error-content {
  background: rgba(239, 68, 68, 0.15);
  color: #fca5a5;
}

/* 调试界面 - 结果预览 */
.result-pre {
  margin: 0;
  padding: 12px;
  font-size: 12px;
  font-family: ui-monospace, monospace;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  background: rgba(128, 128, 128, 0.04);
  border-radius: 8px;
  color: var(--color-on-surface, #374151);
}

:root.dark .result-pre {
  background: rgba(0, 0, 0, 0.3);
  color: #d1d5db;
}
</style>
