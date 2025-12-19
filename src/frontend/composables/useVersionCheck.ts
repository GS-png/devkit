import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ref } from 'vue'

interface VersionInfo {
  current: string
  latest: string
  hasUpdate: boolean
  releaseUrl: string
  releaseNotes: string
  // 网络状态信息（新增）
  networkStatus?: NetworkStatus
}

// 网络状态信息接口
export interface NetworkStatus {
  // 当前 IP 的国家代码（如 "CN", "US"）
  country: string
  // 当前 IP 的城市（可选）
  city?: string
  // 当前 IP 地址
  ip?: string
  // 是否使用了代理
  using_proxy: boolean
  // 代理信息（如果使用了代理）
  proxy_host?: string
  proxy_port?: number
  proxy_type?: string
  // GitHub API 是否可达
  github_reachable: boolean
}

interface UpdateInfo {
  available: boolean
  current_version: string
  latest_version: string
  release_notes: string
  download_url: string
  // 网络状态信息（新增）
  network_status: NetworkStatus
}

interface UpdateProgress {
  chunk_length: number
  content_length?: number
  downloaded: number
  percentage: number
}

// 持久化存储的键名
const CANCELLED_VERSIONS_KEY = 'sanshu_cancelled_versions'

// 加载已取消的版本
function loadCancelledVersions(): Set<string> {
  try {
    const stored = localStorage.getItem(CANCELLED_VERSIONS_KEY)
    if (stored) {
      const versions = JSON.parse(stored) as string[]
      return new Set(versions)
    }
  }
  catch (error) {
    console.warn('加载已取消版本失败:', error)
  }
  return new Set()
}

// 保存已取消的版本
function saveCancelledVersions(versions: Set<string>) {
  try {
    const versionsArray = Array.from(versions)
    localStorage.setItem(CANCELLED_VERSIONS_KEY, JSON.stringify(versionsArray))
  }
  catch (error) {
    console.warn('保存已取消版本失败:', error)
  }
}

// 全局版本检查状态
const versionInfo = ref<VersionInfo | null>(null)
const isChecking = ref(false)
const lastCheckTime = ref<Date | null>(null)

// 网络状态（新增）
const networkStatus = ref<NetworkStatus | null>(null)

// 更新相关状态
const isUpdating = ref(false)
const updateProgress = ref<UpdateProgress | null>(null)
const updateStatus = ref<'idle' | 'checking' | 'downloading' | 'installing' | 'completed' | 'error'>('idle')

// 自动更新弹窗状态
const showUpdateModal = ref(false)
const autoCheckEnabled = ref(true)
// 记录用户取消的版本，避免重复弹窗（持久化存储）
const cancelledVersions = ref<Set<string>>(loadCancelledVersions())

// 比较版本号
function compareVersions(version1: string, version2: string): number {
  const v1Parts = version1.split('.').map(Number)
  const v2Parts = version2.split('.').map(Number)

  for (let i = 0; i < Math.max(v1Parts.length, v2Parts.length); i++) {
    const v1Part = v1Parts[i] || 0
    const v2Part = v2Parts[i] || 0

    if (v1Part > v2Part)
      return 1
    if (v1Part < v2Part)
      return -1
  }

  return 0
}

// 获取当前版本
async function getCurrentVersion(): Promise<string> {
  try {
    const appInfo = await invoke('get_app_info') as string
    const match = appInfo.match(/v(\d+\.\d+\.\d+)/)
    const version = match ? match[1] : '0.2.0'
    return version
  }
  catch (error) {
    console.error('获取当前版本失败:', error)
    return '0.2.0'
  }
}

// 将后端返回的 UpdateInfo 转换为前端使用的 VersionInfo
function mapUpdateInfoToVersionInfo(updateInfo: UpdateInfo): VersionInfo {
	  return {
	    current: updateInfo.current_version,
	    latest: updateInfo.latest_version,
	    hasUpdate: updateInfo.available,
	    // 这里直接使用后端提供的下载地址或 release 页面 URL
	    releaseUrl: updateInfo.download_url,
	    releaseNotes: updateInfo.release_notes,
	    networkStatus: updateInfo.network_status,
	  }
}

// 仅使用浏览器直接访问 GitHub 的兜底实现
// 默认情况下优先走 Tauri 后端（带代理/网络检测），只有在后端不可用时才会调用本函数
async function checkLatestVersionViaGithub(): Promise<VersionInfo | null> {
	  try {
	    const response = await fetch('https://api.github.com/repos/yuaotian/sanshu/releases/latest', {
	      headers: {
	        Accept: 'application/vnd.github.v3+json',
	      },
	    })

	    if (!response.ok) {
	      throw new Error(`GitHub API请求失败: ${response.status}`)
	    }

	    const release = await response.json()
	    // 提取版本号，处理中文tag的情况
	    let latestVersion = release.tag_name
	    // 移除前缀 v 和中文字符，只保留数字和点
	    latestVersion = latestVersion.replace(/^v/, '').replace(/[^\d.]/g, '')
	    const currentVersion = await getCurrentVersion()

	    const hasUpdate = compareVersions(latestVersion, currentVersion) > 0

	    const info: VersionInfo = {
	      current: currentVersion,
	      latest: latestVersion,
	      hasUpdate,
	      releaseUrl: release.html_url,
	      releaseNotes: release.body || '暂无更新说明',
	    }

	    versionInfo.value = info
	    lastCheckTime.value = new Date()

	    return info
	  }
	  catch (error) {
	    // 这里使用 warn 级别，避免在控制台产生过多红色错误
	    console.warn('通过 GitHub API 检查更新失败:', error)
	    return null
	  }
	}

// 统一的版本检查入口
// 优先通过 Tauri 后端 `check_for_updates`（带智能代理检测，如 7890 等端口），
// 只有在后端不可用时才退回到浏览器直连 GitHub
async function checkLatestVersion(): Promise<VersionInfo | null> {
	  if (isChecking.value) {
	    return versionInfo.value
	  }

	  try {
	    isChecking.value = true

	    // 1. 首选后端 Tauri 更新检查（包含代理和网络状态探测）
	    const updateInfo = await checkForUpdatesWithTauri()
	    if (updateInfo) {
	      const info = mapUpdateInfoToVersionInfo(updateInfo)
	      versionInfo.value = info
	      lastCheckTime.value = new Date()
	      // 同步网络状态，便于前端展示
	      if (updateInfo.network_status)
	        networkStatus.value = updateInfo.network_status
	      return info
	    }

	    // 2. 后端不可用时，兜底采用浏览器直接访问 GitHub
	    return await checkLatestVersionViaGithub()
	  }
	  finally {
	    isChecking.value = false
	  }
	}

// 自动检查更新并弹窗（应用启动时调用）
async function autoCheckUpdate(): Promise<boolean> {
  // 如果禁用自动检查，跳过
  if (!autoCheckEnabled.value) {
    return false
  }

  // 如果最近1小时内已经检查过，跳过
  if (lastCheckTime.value && Date.now() - lastCheckTime.value.getTime() < 60 * 60 * 1000) {
    const hasUpdate = versionInfo.value?.hasUpdate || false
    // 如果有更新且未显示弹窗，且用户未取消该版本，则显示弹窗
    if (hasUpdate && !showUpdateModal.value && versionInfo.value?.latest && !cancelledVersions.value.has(versionInfo.value.latest)) {
      showUpdateModal.value = true
    }
    return hasUpdate
  }

  try {
    const info = await checkLatestVersion()

    // 如果检测到新版本且用户未取消该版本，自动显示更新弹窗
    if (info?.hasUpdate && !cancelledVersions.value.has(info.latest)) {
      showUpdateModal.value = true
      return true
    }

    return false
  }
  catch (error) {
    console.warn('自动检查更新失败:', error)
    return false
  }
}

// 静默检查更新（不弹窗，保持兼容性）
async function silentCheckUpdate(): Promise<boolean> {
  const originalAutoCheck = autoCheckEnabled.value
  autoCheckEnabled.value = false

  try {
    const info = await checkLatestVersion()
    return info?.hasUpdate || false
  }
  finally {
    autoCheckEnabled.value = originalAutoCheck
  }
}

// 获取版本信息（如果没有则初始化）
async function getVersionInfo(): Promise<VersionInfo | null> {
  if (!versionInfo.value) {
    const currentVersion = await getCurrentVersion()
    versionInfo.value = {
      current: currentVersion,
      latest: currentVersion,
      hasUpdate: false,
      releaseUrl: '',
      releaseNotes: '',
    }
  }
  return versionInfo.value
}

// 简化的安全打开链接函数
async function safeOpenUrl(url: string): Promise<void> {
  try {
    // 使用已导入的invoke函数
    await invoke('open_external_url', { url })
  }
  catch {
    // 如果Tauri方式失败，复制到剪贴板
    try {
      await navigator.clipboard.writeText(url)
      throw new Error(`无法自动打开链接，已复制到剪贴板，请手动打开: ${url}`)
    }
    catch {
      throw new Error(`无法打开链接，请手动访问: ${url}`)
    }
  }
}

// 打开下载页面
async function openDownloadPage(): Promise<void> {
  await safeOpenUrl('https://github.com/yuaotian/sanshu/releases/latest')
}

// 打开发布页面
async function openReleasePage(): Promise<void> {
  if (versionInfo.value?.releaseUrl) {
    await safeOpenUrl(versionInfo.value.releaseUrl)
  }
}

	// 使用改进的更新检查（避免Tauri原生updater的中文tag问题）
	async function checkForUpdatesWithTauri(): Promise<UpdateInfo | null> {
	  try {
	    const updateInfo = await invoke('check_for_updates') as UpdateInfo
	    console.log('✅ Tauri 更新检查成功:', updateInfo)

	    // 保存网络状态信息（新增）
	    if (updateInfo.network_status) {
	      networkStatus.value = updateInfo.network_status
	      console.log('🌐 网络状态:', updateInfo.network_status)
	    }

	    return updateInfo
	  }
	  catch (error) {
	    console.error('❌ Tauri更新检查失败，将尝试 GitHub API 兜底:', error)

	    // 如果Tauri检查失败，fallback到前端 GitHub API 检查（不再递归调用 checkLatestVersion）
	    const githubInfo = await checkLatestVersionViaGithub()
	
	    if (githubInfo?.hasUpdate) {
	      // 创建默认的网络状态（fallback 模式）
	      const defaultNetworkStatus: NetworkStatus = {
	        country: 'UNKNOWN',
	        using_proxy: false,
	        github_reachable: true, // 如果能获取到 GitHub 信息，说明可达
	      }
	
	      return {
	        available: true,
	        current_version: githubInfo.current,
	        latest_version: githubInfo.latest,
	        release_notes: githubInfo.releaseNotes,
	        download_url: githubInfo.releaseUrl,
	        network_status: defaultNetworkStatus,
	      }
	    }
	
	    return null
	  }
	}

// 一键更新功能
async function performOneClickUpdate(): Promise<void> {
  if (isUpdating.value) {
    console.log('⚠️ 更新已在进行中，跳过')
    return
  }

  try {
    isUpdating.value = true
    updateStatus.value = 'checking'
    updateProgress.value = null

    // 首先检查是否有更新
    const updateInfo = await checkForUpdatesWithTauri()

    if (!updateInfo?.available) {
      throw new Error('没有可用的更新')
    }

    // 设置事件监听器
    const unlistenProgress = await listen('update_download_progress', (event) => {
      updateProgress.value = event.payload as UpdateProgress
      updateStatus.value = 'downloading'
    })

    const unlistenInstallStart = await listen('update_install_started', () => {
      updateStatus.value = 'installing'
    })

    const unlistenInstallFinish = await listen('update_install_finished', () => {
      updateStatus.value = 'completed'
    })

    const unlistenManualDownload = await listen('update_manual_download_required', (event) => {
      console.log('🔗 需要手动下载，URL:', event.payload)
    })

    try {
      // 开始下载和安装
      updateStatus.value = 'downloading'
      await invoke('download_and_install_update')
      updateStatus.value = 'completed'
    }
    catch (backendError) {
      console.error('🔴 后端更新调用失败:', backendError)
      throw backendError
    }
    finally {
      // 清理事件监听器
      unlistenProgress()
      unlistenInstallStart()
      unlistenInstallFinish()
      unlistenManualDownload()
    }
  }
  catch (error) {
    console.error('🔥 更新失败:', error)
    updateStatus.value = 'error'
    throw error
  }
  finally {
    isUpdating.value = false
  }
}

// 重启应用
async function restartApp(): Promise<void> {
  try {
    await invoke('restart_app')
  }
  catch (error) {
    console.error('重启应用失败:', error)
    throw error
  }
}

// 关闭更新弹窗
function closeUpdateModal() {
  showUpdateModal.value = false
}

// 关闭更新弹窗（不再自动弹出该版本的更新提醒）
function dismissUpdate() {
  if (versionInfo.value?.latest) {
    cancelledVersions.value.add(versionInfo.value.latest)
    saveCancelledVersions(cancelledVersions.value)
    console.log(`🚫 用户关闭了版本 ${versionInfo.value.latest} 的更新弹窗`)
  }
  showUpdateModal.value = false
}

// 手动检查更新（重置取消状态）
async function manualCheckUpdate(): Promise<VersionInfo | null> {
  // 清空取消的版本记录，因为这是用户主动检查
  cancelledVersions.value.clear()
  saveCancelledVersions(cancelledVersions.value)
  console.log('🔄 手动检查更新，清空取消记录')

  const info = await checkLatestVersion()

  // 如果有更新，显示弹窗
  if (info?.hasUpdate) {
    showUpdateModal.value = true
  }

  return info
}

export function useVersionCheck() {
  return {
    versionInfo,
    isChecking,
    lastCheckTime,
    isUpdating,
    updateProgress,
    updateStatus,
    showUpdateModal,
    autoCheckEnabled,
    networkStatus, // 新增：网络状态
    checkLatestVersion,
    autoCheckUpdate,
    silentCheckUpdate,
    getVersionInfo,
    openDownloadPage,
    openReleasePage,
    checkForUpdatesWithTauri,
    performOneClickUpdate,
    restartApp,
    closeUpdateModal,
    dismissUpdate,
    manualCheckUpdate,
    compareVersions,
    safeOpenUrl,
  }
}
