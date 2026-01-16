<script setup lang="ts">
import type { CustomPrompt, McpRequest } from '../../types/popup'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useSortable } from '@vueuse/integrations/useSortable'
import { useMessage } from 'naive-ui'
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { useKeyboard } from '../../composables/useKeyboard'

interface Props {
  request: McpRequest | null
  loading?: boolean
  submitting?: boolean
}

interface CachedIngredientItem {
  spiceId: string
  previewUrl: string
}

interface Emits {
  update: [data: {
    note: string
    toppings: string[]
    spiceIds: string[]
  }]
}

const props = withDefaults(defineProps<Props>(), {
  loading: false,
  submitting: false,
})

const emit = defineEmits<Emits>()

// 响应式数据
const note = ref('')
const toppings = ref<string[]>([])
const ingredients = ref<CachedIngredientItem[]>([])
const textareaRef = ref<HTMLTextAreaElement | null>(null)

// 自定义prompt相关状态
const customPrompts = ref<CustomPrompt[]>([])
const customPromptEnabled = ref(true)
const showInsertDialog = ref(false)
const pendingPromptContent = ref('')

// 移除条件性prompt状态管理，直接使用prompt的current_state

// 分离普通prompt和条件性prompt
const normalPrompts = computed(() =>
  customPrompts.value.filter(prompt => prompt.type === 'normal' || !prompt.type),
)

const conditionalPrompts = computed(() =>
  customPrompts.value.filter(prompt => prompt.type === 'conditional'),
)

// 拖拽排序相关状态
const promptContainer = ref<HTMLElement | null>(null)
const sortablePrompts = shallowRef<CustomPrompt[]>([])
const { start, stop } = useSortable(promptContainer, sortablePrompts, {
  animation: 200,
  ghostClass: 'sortable-ghost',
  chosenClass: 'sortable-chosen',
  dragClass: 'sortable-drag',
  handle: '.drag-handle',
  forceFallback: true,
  fallbackTolerance: 3,
  onStart: (evt) => {
    console.log('PopupInput: 拖拽开始:', evt)
    console.log('PopupInput: 拖拽开始时的容器:', evt.from)
    console.log('PopupInput: 拖拽开始时的元素:', evt.item)
  },
  onEnd: (evt) => {
    console.log('PopupInput: 拖拽排序完成:', evt)
    console.log('PopupInput: 从索引', evt.oldIndex, '移动到索引', evt.newIndex)
    console.log('PopupInput: 拖拽后的sortablePrompts:', sortablePrompts.value.map(p => ({ id: p.id, name: p.name })))

    // 检查是否真的发生了位置变化
    if (evt.oldIndex !== evt.newIndex && evt.oldIndex !== undefined && evt.newIndex !== undefined) {
      // 手动重新排列数组
      const newList = [...sortablePrompts.value]
      const [movedItem] = newList.splice(evt.oldIndex, 1)
      newList.splice(evt.newIndex, 0, movedItem)

      // 更新sortablePrompts
      sortablePrompts.value = newList
      console.log('PopupInput: 手动更新后的sortablePrompts:', sortablePrompts.value.map(p => ({ id: p.id, name: p.name })))

      // 立即更新 customPrompts 的顺序，确保数据同步
      // 保留条件性prompt，只更新普通prompt的顺序
      const conditionalPromptsList = customPrompts.value.filter(prompt => prompt.type === 'conditional')
      customPrompts.value = [...sortablePrompts.value, ...conditionalPromptsList]
      console.log('PopupInput: 位置发生变化，保存新排序')

      // 立即保存排序
      savePromptOrder()
    }
    else {
      console.log('PopupInput: 位置未发生变化，无需保存')
    }
  },
  onMove: (evt) => {
    console.log('PopupInput: 拖拽移动中:', evt)
    return true // 允许移动
  },
  onChoose: (evt) => {
    console.log('PopupInput: 选择拖拽元素:', evt)
  },
  onUnchoose: (evt) => {
    console.log('PopupInput: 取消选择拖拽元素:', evt)
  },
})

// 使用键盘快捷键 composable
const { pasteShortcut } = useKeyboard()

const message = useMessage()

let pasteTargetEl: HTMLTextAreaElement | null = null
let pasteListener: ((event: ClipboardEvent) => void) | null = null
let documentPasteListener: ((event: ClipboardEvent) => void) | null = null

function guessFileExtensionFromMime(mime: string): string {
  const normalized = mime.toLowerCase()
  if (normalized === 'image/png')
    return 'png'
  if (normalized === 'image/jpeg')
    return 'jpg'
  if (normalized === 'image/webp')
    return 'webp'
  if (normalized === 'image/gif')
    return 'gif'
  if (normalized === 'image/bmp')
    return 'bmp'
  return 'png'
}

function looksLikeImageFilePathList(text: string): boolean {
  const rawLines = text
    .split(/\r?\n/)
    .map(l => l.trim())
    .filter(Boolean)
  if (rawLines.length === 0)
    return false

  const lines = [...rawLines]
  if (lines[0] === 'copy' || lines[0] === 'cut')
    lines.shift()

  if (lines.length === 0)
    return false

  const imageExt = /(\.png|\.jpe?g|\.webp|\.gif|\.bmp|\.tiff?)(\?.*)?$/i
  return lines.some((line) => {
    if (line.startsWith('#'))
      return false
    if (line.startsWith('file://'))
      return imageExt.test(line)
    if (line.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(line))
      return imageExt.test(line)
    return false
  })
}

async function readIngredientsFromNavigatorClipboard(): Promise<File[]> {
  if (!navigator.clipboard || typeof navigator.clipboard.read !== 'function')
    return []

  const files: File[] = []
  try {
    const items = await navigator.clipboard.read()
    for (const item of items) {
      for (const type of item.types) {
        if (!type.startsWith('image/'))
          continue
        const blob = await item.getType(type)
        const ext = guessFileExtensionFromMime(type)
        files.push(new File([blob], `pasted-${Date.now()}.${ext}`, { type }))
      }
    }
  }
  catch (error) {
    console.debug('navigator.clipboard.read() 读取失败:', error)
  }
  return files
}

async function addIngredientsFromRustClipboard(silent = true): Promise<number> {
  try {
    const blocks = await invoke('read_clipboard_ingredients_cached') as unknown as any[]
    if (!Array.isArray(blocks) || blocks.length === 0)
      return 0

    let addedCount = 0
    for (const block of blocks) {
      const spiceId = block?.spice_id as string | undefined
      const bytes = block?.bytes as number[] | Uint8Array | undefined
      const dishType = block?.dish_type as string | undefined

      if (!spiceId || !bytes || !dishType)
        continue

      if (ingredients.value.some(b => b.spiceId === spiceId))
        continue

      const blob = new Blob([new Uint8Array(bytes as any)], { type: dishType })
      const previewUrl = URL.createObjectURL(blob)
      ingredients.value.push({ spiceId, previewUrl })
      addedCount += 1
    }

    if (addedCount > 0) {
      if (!silent)
        message.success(`已添加 ${addedCount} 个食材`)
      emitUpdate()
    }
    return addedCount
  }
  catch (error) {
    if (!silent)
      message.error(`读取剪贴板食材失败: ${(error as any)?.message || error}`)
    return 0
  }
}

function getTextareaElement(): HTMLTextAreaElement | null {
  try {
    const inputElement = (textareaRef.value as any)?.$el?.querySelector('textarea') || (textareaRef.value as any)?.inputElRef
    return inputElement || null
  }
  catch {
    return null
  }
}

async function handleIngredientPaste(event: ClipboardEvent) {
  if (event.defaultPrevented)
    return

  const clipboardData = event.clipboardData
  if (!clipboardData)
    return

  const ingredientFiles: File[] = []

  if (clipboardData.files && clipboardData.files.length > 0) {
    for (const file of Array.from(clipboardData.files)) {
      if (file.type.startsWith('image/'))
        ingredientFiles.push(file)
    }
  }

  if (ingredientFiles.length === 0 && clipboardData.items) {
    for (const item of Array.from(clipboardData.items)) {
      if (item.kind === 'file' && item.type.startsWith('image/')) {
        const file = item.getAsFile()
        if (file)
          ingredientFiles.push(file)
      }
    }
  }

  if (ingredientFiles.length > 0) {
    event.preventDefault()
    await handleIngredientFiles(ingredientFiles)
    return
  }

  const html = clipboardData.getData('text/html')
  if (html) {
    const dataUrls: string[] = []
    const imgSrcRegex = /<img[^>]+src=["']([^"']+)["'][^>]*>/gi
    let match: RegExpExecArray | null
    while ((match = imgSrcRegex.exec(html)) !== null) {
      const src = match[1]
      if (src && src.startsWith('data:image/'))
        dataUrls.push(src)
    }

    if (dataUrls.length > 0) {
      event.preventDefault()
      let addedCount = 0
      for (const dataUrl of dataUrls) {
        try {
          const blob = await (await fetch(dataUrl)).blob()
          const detectedType = dataUrl.match(/^data:([^;,]+)[;,]/i)?.[1]
          const dishType = detectedType || blob.type || 'image/png'
          const spiceId = await invoke('stash_ingredient_base64_cmd', {
            base64: dataUrl,
            dishType,
            tag: `pasted-${Date.now()}.${guessFileExtensionFromMime(dishType)}`,
          }) as unknown as string

          if (!spiceId)
            continue
          if (ingredients.value.some(b => b.spiceId === spiceId))
            continue

          const previewUrl = URL.createObjectURL(blob)
          ingredients.value.push({ spiceId, previewUrl })
          addedCount += 1
        }
        catch (error) {
          console.debug('HTML 图片粘贴处理失败:', error)
        }
      }
      if (addedCount > 0) {
        message.success(`已添加 ${addedCount} 个食材`)
        emitUpdate()
      }
      return
    }
  }

  const plainText = clipboardData.getData('text/plain')
  if (typeof plainText === 'string' && plainText.length > 0) {
    if (looksLikeImageFilePathList(plainText)) {
      event.preventDefault()
      await addIngredientsFromRustClipboard(false)
      return
    }
    return
  }

  const fallbackFiles = await readIngredientsFromNavigatorClipboard()
  if (fallbackFiles.length > 0) {
    event.preventDefault()
    await handleIngredientFiles(fallbackFiles)
    return
  }

  await addIngredientsFromRustClipboard(false)
}

async function setupPasteListener() {
  await nextTick()
  const el = getTextareaElement()
  if (!el) {
    setTimeout(() => {
      void setupPasteListener()
    }, 120)
    return
  }
  if (pasteTargetEl === el)
    return

  cleanupPasteListener()
  pasteTargetEl = el
  pasteListener = (event: ClipboardEvent) => {
    void handleIngredientPaste(event)
  }
  pasteTargetEl.addEventListener('paste', pasteListener)
}

function setupDocumentPasteListener() {
  if (documentPasteListener)
    return
  documentPasteListener = (event: ClipboardEvent) => {
    if (event.defaultPrevented)
      return
    const textarea = getTextareaElement()
    if (!textarea)
      return

    if (pasteTargetEl === textarea)
      return

    const active = document.activeElement
    if (active === textarea)
      void handleIngredientPaste(event)
  }
  document.addEventListener('paste', documentPasteListener, true)
}

function cleanupPasteListener() {
  if (pasteTargetEl && pasteListener) {
    pasteTargetEl.removeEventListener('paste', pasteListener)
  }
  pasteTargetEl = null
  pasteListener = null
}

function cleanupDocumentPasteListener() {
  if (documentPasteListener)
    document.removeEventListener('paste', documentPasteListener, true)
  documentPasteListener = null
}

// 计算属性
const hasOptions = computed(() => (props.request?.menu?.length ?? 0) > 0)
const canSubmit = computed(() => {
  const hasOptionsSelected = toppings.value.length > 0
  const hasInputText = note.value.trim().length > 0
  const hasBlocks = ingredients.value.length > 0

  if (hasOptions.value) {
    return hasOptionsSelected || hasInputText || hasBlocks
  }
  return hasInputText || hasBlocks
})

// 工具栏状态文本
const statusText = computed(() => {
  // 检查是否有任何输入内容
  const hasInput = toppings.value.length > 0
    || ingredients.value.length > 0
    || note.value.trim().length > 0

  // 如果有任何输入内容，返回空字符串让 PopupActions 显示快捷键
  if (hasInput) {
    return ''
  }

  return '等待输入...'
})

// 发送更新事件
function emitUpdate() {
  // 获取条件性prompt的追加内容
  const conditionalContent = generateConditionalContent()

  // 将条件性内容追加到用户输入
  const finalNote = note.value + conditionalContent

  emit('update', {
    note: finalNote,
    toppings: toppings.value,
    spiceIds: ingredients.value.map(b => b.spiceId),
  })
}

watch(note, () => {
  emitUpdate()
})

// 处理选项变化
function handleOptionChange(option: string, checked: boolean) {
  if (checked) {
    toppings.value.push(option)
  }
  else {
    const idx = toppings.value.indexOf(option)
    if (idx > -1)
      toppings.value.splice(idx, 1)
  }
  emitUpdate()
}

// 处理选项切换（整行点击）
function handleOptionToggle(option: string) {
  const idx = toppings.value.indexOf(option)
  if (idx > -1) {
    toppings.value.splice(idx, 1)
  }
  else {
    toppings.value.push(option)
  }
  emitUpdate()
}

async function handleIngredientFiles(files: FileList | File[]): Promise<void> {
  console.log('=== 处理食材文件 ===')
  console.log('文件数量:', files.length)

  const fileToDataUrl = (file: File): Promise<string> => {
    return new Promise((resolve, reject) => {
      const reader = new FileReader()
      reader.onerror = () => reject(reader.error || new Error('读取文件失败'))
      reader.onload = () => resolve(String(reader.result || ''))
      reader.readAsDataURL(file)
    })
  }

  for (const file of files) {
    console.log('处理文件:', file.name, '类型:', file.type, '大小:', file.size)

    if (file.type.startsWith('image/')) {
      try {
        const dataUrl = await fileToDataUrl(file)
        const spiceId = await invoke('stash_ingredient_base64_cmd', {
          base64: dataUrl,
          dishType: file.type || 'image/png',
          tag: file.name,
        }) as unknown as string

        if (!spiceId)
          continue

        if (ingredients.value.some(b => b.spiceId === spiceId)) {
          message.warning(`食材 ${file.name} 已存在`)
          continue
        }

        const previewUrl = URL.createObjectURL(file)
        ingredients.value.push({ spiceId, previewUrl })
        message.success(`食材 ${file.name} 已添加`)
        emitUpdate()
      }
      catch (error) {
        console.error('食材处理失败:', error)
        const errMsg = (() => {
          const e: any = error
          if (e && typeof e.message === 'string' && e.message.length > 0)
            return e.message
          try {
            return typeof e === 'string' ? e : JSON.stringify(e)
          }
          catch {
            return `${e}`
          }
        })()
        message.error(`食材 ${file.name} 处理失败: ${errMsg}`)
      }
    }
    else {
      console.log('跳过非食材文件:', file.type)
    }
  }

  console.log('=== 食材文件处理完成 ===')
}

function removeIngredient(index: number) {
  const removed = ingredients.value.splice(index, 1)[0]
  if (removed?.previewUrl)
    URL.revokeObjectURL(removed.previewUrl)
  if (removed?.spiceId)
    void invoke('discard_spice_cmd', { spice_id: removed.spiceId })
  emitUpdate()
}

// 移除自定义食材预览功能，改用 Naive UI 的内置预览

// 加载自定义prompt配置
async function loadCustomPrompts() {
  try {
    console.log('PopupInput: 开始加载自定义prompt配置')
    const config = await invoke('get_custom_prompt_config')
    if (config) {
      const promptConfig = config as any

      // 按sort_order排序
      customPrompts.value = (promptConfig.prompts || []).sort((a: CustomPrompt, b: CustomPrompt) => a.sort_order - b.sort_order)
      customPromptEnabled.value = promptConfig.enabled ?? true
      console.log('PopupInput: 加载到的prompt数量:', customPrompts.value.length)
      console.log('PopupInput: 条件性prompt列表:', customPrompts.value.filter(p => p.type === 'conditional'))

      // 同步到拖拽列表（只包含普通prompt）
      sortablePrompts.value = [...normalPrompts.value]
      console.log('PopupInput: 同步到sortablePrompts:', sortablePrompts.value.length)

      // 延迟初始化拖拽功能，等待组件完全挂载
      if (customPrompts.value.length > 0) {
        console.log('PopupInput: 准备启动拖拽功能')
        initializeDragSort()
      }
      else {
        console.log('PopupInput: 没有prompt，跳过拖拽初始化')
      }
    }
  }
  catch (error) {
    console.error('PopupInput: 加载自定义prompt失败:', error)
  }
}

async function initializeDragSort() {
  await nextTick()
  await nextTick()
  if (!promptContainer.value)
    return
  start()
}

async function savePromptOrder() {
  try {
    const promptIds = sortablePrompts.value.map(p => p.id)
    await invoke('update_custom_prompt_order', { promptIds })
    message.success('排序已保存')
  }
  catch (error) {
    console.error('保存排序失败:', error)
    message.error('保存排序失败')
    loadCustomPrompts()
  }
}

// 处理自定义prompt点击
function handlePromptClick(prompt: CustomPrompt) {
  // 如果prompt内容为空或只有空格，直接清空输入框
  if (!prompt.content || prompt.content.trim() === '') {
    note.value = ''
    emitUpdate()
    return
  }

  if (note.value.trim()) {
    // 如果输入框有内容，显示插入选择对话框
    pendingPromptContent.value = prompt.content
    showInsertDialog.value = true
  }
  else {
    // 如果输入框为空，直接插入
    insertPromptContent(prompt.content)
  }
}

// 处理引用消息内容
function handleQuoteMessage(messageContent: string) {
  if (note.value.trim()) {
    // 输入框有内容，显示插入选择对话框
    pendingPromptContent.value = messageContent
    showInsertDialog.value = true
  }
  else {
    // 输入框为空，直接插入
    insertPromptContent(messageContent)
    message.success('原文内容已引用到输入框')
  }
}

// 插入prompt内容
function insertPromptContent(content: string, mode: 'replace' | 'append' = 'replace') {
  if (mode === 'replace') {
    note.value = content
  }
  else {
    note.value = note.value.trim() + (note.value.trim() ? '\n\n' : '') + content
  }

  // 聚焦到输入框
  setTimeout(() => {
    if (textareaRef.value) {
      textareaRef.value.focus()
      // 尝试将光标移到末尾（对于Naive UI组件）
      try {
        const inputElement = textareaRef.value.$el?.querySelector('textarea') || textareaRef.value.inputElRef
        if (inputElement && typeof inputElement.setSelectionRange === 'function') {
          inputElement.setSelectionRange(inputElement.value.length, inputElement.value.length)
        }
      }
      catch (error) {
        console.log('设置光标位置失败:', error)
      }
    }
  }, 100)

  emitUpdate()
}

// 处理插入模式选择
function handleInsertMode(mode: 'replace' | 'append') {
  insertPromptContent(pendingPromptContent.value, mode)
  showInsertDialog.value = false
  pendingPromptContent.value = ''
}

// 处理条件性prompt开关变化
async function handleConditionalToggle(promptId: string, value: boolean) {
  // 先更新本地状态
  const prompt = customPrompts.value.find(p => p.id === promptId)
  if (prompt) {
    prompt.current_state = value
  }

  // 保存到后端
  try {
    await invoke('update_conditional_prompt_state', {
      promptId,
      newState: value,
    })
    message.success('上下文追加状态已保存')
  }
  catch (error) {
    console.error('保存条件性prompt状态失败:', error)
    message.error(`保存设置失败: ${(error as any)?.message}` || error)

    // 回滚本地状态
    if (prompt) {
      prompt.current_state = !value
    }
  }
}

// 生成条件性prompt的追加内容
function generateConditionalContent(): string {
  const conditionalTexts: string[] = []

  conditionalPrompts.value.forEach((prompt) => {
    const isEnabled = prompt.current_state ?? false
    const template = isEnabled ? prompt.template_true : prompt.template_false

    if (template && template.trim()) {
      conditionalTexts.push(template.trim())
    }
  })

  return conditionalTexts.length > 0 ? `\n\n${conditionalTexts.join('\n')}` : ''
}

// 获取条件性prompt的自适应描述
function getConditionalDescription(prompt: CustomPrompt): string {
  const isEnabled = prompt.current_state ?? false
  const template = isEnabled ? prompt.template_true : prompt.template_false

  // 如果有对应状态的模板，显示模板内容，否则显示原始描述
  if (template && template.trim()) {
    return template.trim()
  }

  return prompt.description || ''
}

// 移除拖拽相关的监听器

// 事件监听器引用
let unlistenCustomPromptUpdate: (() => void) | null = null
let unlistenWindowMove: (() => void) | null = null

// 修复输入法候选框位置的函数
function fixIMEPosition() {
  if (textareaRef.value) {
    try {
      // 获取实际的 textarea 元素（Naive UI 的 n-input）
      const inputElement = (textareaRef.value as any).$el?.querySelector('textarea') || (textareaRef.value as any).inputElRef
      if (inputElement && document.activeElement === inputElement) {
        // 先失焦再聚焦，让输入法重新计算位置
        inputElement.blur()
        setTimeout(() => {
          inputElement.focus()
        }, 10)
      }
    }
    catch (error) {
      console.debug('修复IME位置失败:', error)
    }
  }
}

// 设置窗口移动监听器
async function setupWindowMoveListener() {
  try {
    const webview = getCurrentWebviewWindow()
    // 监听窗口移动事件
    unlistenWindowMove = await webview.onMoved(() => {
      // 窗口移动后修复输入法位置
      fixIMEPosition()
    })
    console.log('窗口移动监听器已设置')
  }
  catch (error) {
    console.error('设置窗口移动监听器失败:', error)
  }
}

// 组件挂载时加载自定义prompt
onMounted(async () => {
  console.log('组件挂载，开始加载prompt')
  await loadCustomPrompts()

  await setupPasteListener()
  setupDocumentPasteListener()

  // 监听自定义prompt更新事件
  unlistenCustomPromptUpdate = await listen('custom-prompt-updated', () => {
    console.log('收到自定义prompt更新事件，重新加载数据')
    loadCustomPrompts()
  })
  // 设置窗口移动监听器
  setupWindowMoveListener()
})

onUnmounted(() => {
  cleanupPasteListener()
  cleanupDocumentPasteListener()
  // 清理事件监听器
  if (unlistenCustomPromptUpdate) {
    unlistenCustomPromptUpdate()
  }
  // 清理窗口移动监听器
  if (unlistenWindowMove) {
    unlistenWindowMove()
  }

  for (const b of ingredients.value) {
    if (b.previewUrl)
      URL.revokeObjectURL(b.previewUrl)
    if (b.spiceId)
      void invoke('discard_spice_cmd', { spice_id: b.spiceId })
  }

  // 停止拖拽功能
  stop()
})

// 重置数据
function reset() {
  note.value = ''
  toppings.value = []
  for (const b of ingredients.value) {
    if (b.previewUrl)
      URL.revokeObjectURL(b.previewUrl)
    if (b.spiceId)
      void invoke('discard_spice_cmd', { spice_id: b.spiceId })
  }
  ingredients.value = []
  emitUpdate()
}

// 更新数据（用于外部同步）
function updateData(data: { note?: string, toppings?: string[], spiceIds?: string[] }) {
  if (data.note !== undefined) {
    note.value = data.note
  }
  if (data.toppings !== undefined) {
    toppings.value = data.toppings
  }
  if (data.spiceIds !== undefined) {
    // 父组件现在只会传 spiceId 列表；这里不做反向同步（预览只能由 PopupInput 自己维护）
  }

  emitUpdate()
}

// 移除了文件选择和测试食材功能

// 暴露方法给父组件
defineExpose({
  reset,
  canSubmit,
  statusText,
  updateData,
  handleQuoteMessage,
})
</script>

<template>
  <div class="space-y-3">
    <!-- 预定义选项 -->
    <div v-if="!loading && hasOptions" class="space-y-3" data-guide="predefined-options">
      <h4 class="text-sm font-medium text-white">
        请选择选项
      </h4>
      <n-space vertical size="small">
        <div
          v-for="(option, index) in request!.menu"
          :key="`option-${index}`"
          class="rounded-lg p-3 border border-gray-600 bg-gray-100 cursor-pointer hover:opacity-80 transition-opacity"
          @click="handleOptionToggle(option)"
        >
          <n-checkbox
            :value="option"
            :checked="toppings.includes(option)"
            :disabled="submitting"
            size="medium"
            @update:checked="(checked: boolean) => handleOptionChange(option, checked)"
            @click.stop
          >
            {{ option }}
          </n-checkbox>
        </div>
      </n-space>
    </div>

    <!-- 食材预览区域 -->
    <div v-if="!loading && ingredients.length > 0" class="space-y-3">
      <h4 class="text-sm font-medium text-white">
        已添加的食材 ({{ ingredients.length }})
      </h4>

      <!-- 使用 Naive UI 的食材组件，支持预览和放大 -->
      <n-image-group>
        <div class="flex flex-wrap gap-3">
          <div
            v-for="(block, index) in ingredients"
            :key="`ingredient-${index}`"
            class="relative"
          >
            <!-- 使用 n-image 组件，启用预览功能 -->
            <n-image
              :src="block.previewUrl"
              width="100"
              height="100"
              object-fit="cover"
              class="rounded-lg border-2 border-gray-300 hover:border-primary-400 transition-all duration-200 cursor-pointer"
            />

            <!-- 删除按钮 -->
            <n-button
              class="absolute -top-2 -right-2 z-10"
              size="tiny"
              type="error"
              circle
              @click="removeIngredient(index)"
            >
              <template #icon>
                <div class="i-carbon-close w-3 h-3" />
              </template>
            </n-button>

            <!-- 序号 -->
            <div class="absolute bottom-1 left-1 w-5 h-5 bg-primary-500 text-white text-xs rounded-full flex items-center justify-center font-bold shadow-sm z-5">
              {{ index + 1 }}
            </div>
          </div>
        </div>
      </n-image-group>
    </div>

    <!-- 文本输入区域 -->
    <div v-if="!loading" class="space-y-3">
      <h4 class="text-sm font-medium text-white">
        {{ hasOptions ? '补充说明 (可选)' : '请输入您的回复' }}
      </h4>

      <!-- 自定义prompt按钮区域 -->
      <div v-if="customPromptEnabled && customPrompts.length > 0" class="space-y-2" data-guide="custom-prompts">
        <div class="text-xs text-on-surface-secondary flex items-center gap-2">
          <div class="i-carbon-bookmark w-3 h-3 text-primary-500" />
          <span>快捷模板 (拖拽调整顺序):</span>
        </div>
        <div
          ref="promptContainer"
          data-prompt-container
          class="flex flex-wrap gap-2"
        >
          <div
            v-for="prompt in sortablePrompts"
            :key="prompt.id"
            :title="prompt.description || (prompt.content.trim() ? prompt.content : '清空输入框')"
            class="inline-flex items-center gap-1 px-2 py-1 text-xs bg-container-secondary hover:bg-container-tertiary rounded transition-all duration-200 select-none border border-gray-600 text-on-surface sortable-item"
          >
            <!-- 拖拽手柄 -->
            <div class="drag-handle cursor-move p-0.5 rounded hover:bg-container-tertiary transition-colors">
              <div class="i-carbon-drag-horizontal w-3 h-3 text-on-surface-secondary" />
            </div>

            <!-- 按钮内容 -->
            <div
              class="inline-flex items-center cursor-pointer"
              @click="handlePromptClick(prompt)"
            >
              <span>{{ prompt.name }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 上下文追加区域 -->
      <div v-if="customPromptEnabled && conditionalPrompts.length > 0" class="space-y-2" data-guide="context-append">
        <div class="text-xs text-on-surface-secondary flex items-center gap-2">
          <div class="i-carbon-settings-adjust w-3 h-3 text-primary-500" />
          <span>上下文追加:</span>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <div
            v-for="prompt in conditionalPrompts"
            :key="prompt.id"
            class="flex items-center justify-between p-2 bg-container-secondary rounded border border-gray-600 hover:bg-container-tertiary transition-colors text-xs"
          >
            <div class="flex-1 min-w-0 mr-2">
              <div class="text-xs text-on-surface truncate font-medium" :title="prompt.condition_text || prompt.name">
                {{ prompt.condition_text || prompt.name }}
              </div>
              <div v-if="getConditionalDescription(prompt)" class="text-xs text-primary-600 dark:text-primary-400 opacity-50 dark:opacity-60 mt-0.5 truncate leading-tight" :title="getConditionalDescription(prompt)">
                {{ getConditionalDescription(prompt) }}
              </div>
            </div>
            <n-switch
              :value="prompt.current_state ?? false"
              size="small"
              @update:value="(value: boolean) => handleConditionalToggle(prompt.id, value)"
            />
          </div>
        </div>
      </div>

      <!-- 食材提示区域 -->
      <div v-if="ingredients.length === 0" class="text-center">
        <div class="text-xs text-on-surface-secondary">
          💡 提示：可以在输入框中粘贴食材 ({{ pasteShortcut }})
        </div>
      </div>

      <!-- 文本输入框 -->
      <n-input
        ref="textareaRef"
        v-model:value="note"
        type="textarea"
        size="small"
        :placeholder="hasOptions ? `您可以在这里添加补充说明... (支持粘贴食材 ${pasteShortcut})` : `请输入您的回复... (支持粘贴食材 ${pasteShortcut})`"
        :disabled="submitting"
        :autosize="{ minRows: 3, maxRows: 6 }"
        data-guide="popup-input"
      />
    </div>

    <!-- 插入模式选择对话框 -->
    <n-modal v-model:show="showInsertDialog" preset="dialog" title="插入模式选择">
      <template #header>
        <div class="flex items-center gap-2">
          <div class="i-carbon-text-creation w-4 h-4" />
          <span>插入Prompt</span>
        </div>
      </template>
      <div class="space-y-4">
        <p class="text-sm text-on-surface-secondary">
          输入框中已有内容，请选择插入模式：
        </p>
        <div class="bg-container-secondary p-3 rounded text-sm">
          {{ pendingPromptContent }}
        </div>
      </div>
      <template #action>
        <div class="flex gap-2">
          <n-button @click="showInsertDialog = false">
            取消
          </n-button>
          <n-button type="warning" @click="handleInsertMode('replace')">
            替换内容
          </n-button>
          <n-button type="primary" @click="handleInsertMode('append')">
            追加内容
          </n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<style scoped>
/* Sortable.js 拖拽样式 */
.sortable-ghost {
  opacity: 0.5;
  transform: scale(0.95);
}

.sortable-chosen {
  cursor: grabbing !important;
}

.sortable-drag {
  opacity: 0.8;
  transform: rotate(5deg);
}
</style>
