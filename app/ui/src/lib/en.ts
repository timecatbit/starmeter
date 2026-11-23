// English for the panel (see i18n.svelte.ts): whole phrases first, then rules for the engine's text with numbers,
// dates and names in it. Keys are the Chinese exactly as the engine or the panel writes it; {name} in a key marks a
// value the panel puts in. New text in the engine or the panel needs its English here: `npx playwright test -g English`
// lists what is missing.

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const WEEKDAYS: Record<string, string> = { 一: 'Mon', 二: 'Tue', 三: 'Wed', 四: 'Thu', 五: 'Fri', 六: 'Sat', 日: 'Sun', 天: 'Sun' };
const plural = (n: string, one: string, many: string) => (n === '1' ? one : many);

export const phrases: Record<string, string> = {
  // Ranges, groupings and the header
  今天: 'Today', 昨天: 'Yesterday', '近 24 小时': 'Last 24 hours', '近 7 天': 'Last 7 days', '近 30 天': 'Last 30 days',
  本周: 'This week', 本月: 'This month', 全部: 'All time', 工具: 'Tool', 模型: 'Model', 项目: 'Project', 会话: 'Sessions',
  统计范围: 'Range', 分享用量图: 'Share usage picture', 主页模块与设置: 'Modules and settings', 钉住面板: 'Pin the panel',
  取消钉住面板: 'Unpin the panel', 取消钉住: 'Unpin', '钉住，每 {n} 秒刷新': 'Pin, refreshing every {n} s', 返回: 'Back', 完成: 'Done',
  'API 估算': 'API estimate', '已知 API 估算': 'Known API estimate', 'Token 用量': 'Tokens', 请求: 'Requests',
  每小时: 'Hourly', 每天: 'Daily', 部分图表数据不可用: 'Some chart data is unavailable', 此时段暂无调用: 'No calls in this range',
  实时: 'Live', 现在: 'Now', '1 分钟前': '1 min ago', '正在读取第一笔数据…': 'Reading the first figures…', 实时用量图: 'Live usage chart',
  实时图显示: 'Live chart shows', 用量趋势图: 'Usage trend', 按客户端自己的记录: 'Clients\' own logs',
  // Cards
  已同步: 'Synced', 运行正常: 'Running', 待同步: 'Waiting to sync', 等待可用数据: 'Waiting for data', 费用待定价: 'Not priced yet',
  未定价: 'Unpriced', 刷新失败: 'Refresh failed', 保留原数据: 'keeping earlier data', 部分记录未读取: 'Some records weren\'t read', 本机记录未读取: 'This computer\'s records weren\'t read', '本机记录未计入 Summary': 'This computer\'s records not in Summary',
  读取失败: 'Read failed', 数据暂不可用: 'Data unavailable for now', '未计入 Summary': 'Not in Summary', 'Token 未记录': 'Tokens not logged',
  '已由 Magpie 计入': 'Counted by Magpie', 未命名对话: 'Untitled chat', 'ChatGPT 对话': 'ChatGPT chats', 'ChatGPT 项目': 'ChatGPT project', 未标注项目: 'No project',
  未标注: 'Unlabelled', 服务部分降级: 'Partial outage', 服务大范围故障: 'Major outage', 服务严重故障: 'Critical outage', 服务维护中: 'Maintenance',
  客户端: 'Clients', '{n} 请求': '{n} requests', '{n} 请求|1': '{n} request', '{n} 会话': '{n} sessions', '{n} 会话|1': '{n} session', '输出 {n}': 'Output {n}', '缓存 —': 'Cache —',
  '缓存 {n}%': 'Cache {n}%', '已定价 {n}%': '{n}% priced', '{n} 次未定价': '{n} unpriced', 缓存: 'Cache', 输出: 'Output', 费用: 'Cost', 会话数: 'Sessions', 输入: 'Input', 缓存读: 'Cache read', 缓存写: 'Cache write',
  // Why a tool's or a plan's figures can't be read (the engine's statuses)
  '未找到 Claude 用量记录': 'No Claude usage records found', 未找到记录目录: 'Records folder not found', 请选择记录目录: 'Choose the records folder',
  暂无用量记录: 'No usage records yet', '未找到 Gemini 用量记录': 'No Gemini usage records found', '暂无 Gemini 用量记录': 'No Gemini usage records yet',
  '未找到 OpenCode 数据库': 'OpenCode\'s database not found', 记录暂不可用: 'Records unavailable for now', '未配置 OpenCodex': 'OpenCodex isn\'t set up',
  用量记录暂不可用: 'Usage records unavailable for now', 不支持的日志版本: 'Unsupported log version', 未开启用量读取: 'Usage reading is off', 用量记录格式异常: 'The usage records are in an unexpected format',
  用量记录过大: 'The usage records are too large', '无法连接 cursor.com': 'Can\'t reach cursor.com',
  'cursor.com 拒绝读取用量记录': 'cursor.com refused to share the usage records', 'cursor.com 请求过于频繁，稍后重试': 'Too many requests to cursor.com; trying again later',
  '未找到 Cursor 登录，请先登录 Cursor 桌面版或 Cursor CLI': 'No Cursor sign-in found: sign in to the Cursor app or Cursor CLI first',
  'Cursor 登录已失效，请打开 Cursor 重新登录': 'Cursor\'s sign-in no longer works: open Cursor and sign in again',
  'Cursor 登录已过期，请打开 Cursor 重新登录': 'Cursor\'s sign-in has expired: open Cursor and sign in again',
  'Cursor 登录已加密保存，暂时无法读取': 'Cursor keeps its sign-in encrypted; it can\'t be read for now', '无法识别 Cursor 登录信息': 'Cursor\'s sign-in couldn\'t be recognized',
  客户端接口不可用: 'The client\'s interface is unavailable', '无法启动 ChatGPT 编程客户端': 'Couldn\'t start the ChatGPT coding client',
  无法读取客户端登录状态: 'Couldn\'t read the client\'s sign-in', 额度暂不可用: 'Limits unavailable for now', 暂时无法读取数据: 'Can\'t read the data for now',
  // Heatmap
  暂无用量: 'No usage yet', 无用量: 'No usage', 部分小时数据不可用: 'Some hourly data is unavailable', 每格: 'per dot',
  '{n} Token': '{n} tokens', Token: 'Tokens', '{unit} Token': '{unit} tokens',
  // Subscriptions
  来源: 'Source', 本机登录: 'local sign-in', 编程工具额度: 'coding limits', '5 小时': '5 hours', 更新时间: 'Updated',
  '订阅月费…': 'Monthly price…', 'Code 登录': 'Code sign-in', 额外用量: 'Extra usage', 立即刷新用量: 'Refresh usage now',
  打开订阅用量: 'Open plan usage', 'Cursor 账户（cursor.com）': 'Cursor account (cursor.com)', 本周期: 'This cycle',
  套餐额度: 'Plan allowance', 按需用量: 'On-demand usage', 'Credits 不限': 'Unlimited credits', 对数: 'log', 免费重置: 'Free resets', 不限: 'Unlimited',
  窗口用完后按用量扣除: 'Spent once the windows are used up', '一次补满窗口 · 在 Codex 里使用': 'Refills the windows at once · use it in Codex',
  在用额外用量: 'On extra usage', 在用按需用量: 'On on-demand usage', 打开用量与账单: 'Open usage and billing',
  'Qoder 桌面 App 的本机日志': 'the Qoder desktop app\'s local logs', '套餐 Credits': 'Plan Credits', '加购 Credits': 'Add-on Credits',
  '在 Qoder 窗口右下角打开 Credits 用量后更新': 'Updates once you open Credits usage at the bottom right of the Qoder window',
  '手动标记，未验证套餐': 'Marked by hand; the plan isn\'t verified', 本机标记文件: 'Local mark file', 清除配置标记: 'Clear the mark',
  每周: 'Weekly', 'Claude 和 GPT': 'Claude and GPT', 'Antigravity 本机额度': 'Antigravity on this computer',
  'Antigravity 开着时每分钟更新；关着时保留最近一次读到的额度': 'Updates every minute while Antigravity is open; closed, the last figures read stay',
  '打开 Antigravity': 'Open Antigravity', '打开 Antigravity 后显示': 'Shows once Antigravity is open', '打开 Antigravity 后更新': 'Updates once Antigravity is open',
  '未发现 Antigravity': 'No Antigravity found', '装好 Antigravity 并登录后显示额度': 'Install Antigravity and sign in to see the quota',
  'Antigravity 没开，打开后更新': 'Antigravity is closed; it updates once it is open', 'Antigravity 没有运行': 'Antigravity isn\'t running',
  'Antigravity 没有给出额度': 'Antigravity gave no quota',
  打开订阅管理: 'Open subscription settings', 刚刚更新: 'Just now', 未开启: 'Off', 未填写: 'Not set', 已标记: 'Marked',
  标记为已配置: 'Mark as set up', 本月已用: 'Spent this month', 与匀速持平: 'on even pace', 未开启读取: 'Reading is off',
  // API keys
  默认: 'Default', '今天已用（UTC）': 'Spent today (UTC)', '测试 Key': 'Test key', '重命名…': 'Rename…', '删除…': 'Delete…',
  '添加 Key…': 'Add key…', '余额提醒…': 'Balance alert…', 国际站: 'International', '人民币余额提醒…': 'Yuan balance alert…',
  '美元余额提醒…': 'Dollar balance alert…', 'Key 可用': 'Key works', '选择密钥文件…': 'Choose key file…', 已配置: 'Set up', 未配置: 'Not set up',
  '保存或测试时校验一次 Key，只表示 Key 可用': 'Checks the key once when saved or tested; it only shows the key works',
  'Google 没有余额接口：保存或测试时校验一次 Key，只表示 Key 可用': 'Google has no balance API: checks the key once when saved or tested; it only shows the key works',
  '需要 xAI 管理 Key 和 Team ID，显示团队余额': 'Needs an xAI management key and Team ID; shows the team\'s balance',
  '需要 MiniMax Token Plan 的 Key，显示套餐各窗口的用量': 'Needs a MiniMax Token Plan key; shows each window of the plan',
  '仅团队版/企业版：组织管理员的 OpenAPI Key 和组织 ID，显示组织共享的加购 Credits；个人账号没有 API':
    'Teams and Enterprise only: an organisation admin\'s OpenAPI key and organisation ID, showing the organisation\'s shared add-on Credits; personal accounts have no API',
  // Sessions
  更早开始: 'Started earlier', 这一轮完成时提醒: 'Notify when a turn finishes', 运行中的会话这一轮完成时发通知: 'A notification when a running session finishes its turn',
  没有会话: 'No sessions', 金额为会话累计: 'amounts are each session’s total',
  运行中: 'Running', 空闲: 'Idle', 本轮: 'This turn', 上一轮: 'Last turn', 等待输入: 'Waiting',
  // Monitoring and updates
  'Komari 监控': 'Komari status', '打开 Komari 面板': 'Open the Komari panel', 刷新监控数据: 'Refresh monitoring data', 在线: 'Online',
  离线: 'Offline', '到期 长期': 'No expiry', 工具更新: 'Tool updates', 立即检查更新: 'Check for updates now',
  '查看最近 CLI 更新日志': 'Show the latest CLI update log', '选择工具…': 'Choose tools…',
  更新记录: 'Changelog', '点击打开 App 更新': 'Click to update in the app', 'Claude 官方': 'Official', 'Homebrew Cask': 'Homebrew Cask',
  // A tool's update status, the line under it and why a check failed (updates.rs, more_rows.rs)
  有新版本: 'Update available', 已是最新: 'Up to date', 等待检查: 'Waiting to check', 公开渠道暂无更新: 'Nothing new on the public channel',
  未安装: 'Not installed', 更新渠道待确认: 'Update channel not known yet', 安装渠道待确认: 'Install channel not known yet',
  '在 App 内检查更新': 'Check in the app', 在官网查看更新: 'Check on the website', 尚未接入自动版本检查: 'No automatic version check yet',
  版本待确认: 'Version not known yet', 已下载: 'Downloaded', 客户端更新源: 'The client\'s update feed',
  'Homebrew 自定义 Cask': 'Custom Homebrew cask', 'Homebrew 自定义 Formula': 'Custom Homebrew formula',
  '检查失败，稍后重试': 'Check failed; trying again later', 读取本机版本失败: 'Couldn\'t read the installed version',
  版本信息无法识别: 'Version information not recognized', '证书验证失败（Python 缺少根证书）': 'Certificate check failed (Python lacks root certificates)',
  '连接超时（检查网络或代理）': 'Connection timed out (check the network or proxy)', '域名无法解析（检查网络或代理）': 'Couldn\'t resolve the host (check the network or proxy)',
  '连接被拒绝（检查代理）': 'Connection refused (check the proxy)', '无法连接（检查网络或代理）': 'Couldn\'t connect (check the network or proxy)',
  // OpenCodex updating itself
  自动更新中: 'Updating automatically', 正在自动更新: 'Updating automatically', 等待自动更新: 'Waiting to update automatically',
  正在确认更新任务: 'Checking the update job', 正在重启并确认版本: 'Restarting and checking the version',
  '新版本已启动，正在确认运行状态': 'The new version started; checking that it runs', 此安装方式需手动更新: 'This install is updated by hand',
  '自动更新未完成，1 小时后重试': 'The automatic update didn\'t finish; trying again in 1 hour',
  '自动更新暂未启动，1 小时后重试': 'The automatic update hasn\'t started; trying again in 1 hour',
  '更新结果暂未确认，1 小时后重新检查': 'The update isn\'t confirmed yet; checking again in 1 hour',
  // Starmeter updating itself, and why an update failed (self_update.rs)
  正在更新: 'Updating', 更新失败: 'Update failed', '打开 GitHub 发布页': 'Open the release on GitHub', 查看更新内容: 'What\'s new',
  '正在下载并安装，完成后自动刷新': 'Downloading and installing; refreshes when done', 日志: 'Log', 安装失败: 'Install failed',
  '安装失败：下载失败': 'Install failed: the download failed', 安装包大小异常: 'The package\'s size is wrong',
  安装包结构异常: 'The package\'s layout is wrong', '正在下载新版本，校验后自动替换并重新打开': 'Downloading the new version; once checked it takes this one\'s place and opens again',
  这个版本不能在应用内更新: 'This copy can\'t update itself', 新版本的签名校验不通过: 'The new version\'s signature didn\'t check out',
  '安装包里的 App 不是 Starmeter': 'The package doesn\'t hold Starmeter', '安装包打不开（DMG 挂载失败）': 'The package wouldn\'t open (the DMG didn\'t mount)',
  更新程序没能启动: 'The updater didn\'t start', '这份 Starmeter 不能在原处更新，请从发布页下载': 'This copy of Starmeter can\'t be updated in place; download it from the release page',
  '安装失败：写入临时文件失败': 'Install failed: couldn\'t write a temporary file', 安装包校验失败: 'The package failed its checksum',
  '下载失败：GitHub 和镜像都连不上': 'Download failed: neither GitHub nor its mirrors could be reached',
  '正在读取…': 'Reading…',
  新版本的签名者与已安装的不一致: 'The new version isn\'t signed by the same developer as the installed one',
  发布中没有安装包: 'The release has no package', 发布中没有校验值: 'The release has no checksum', 校验值格式错误: 'The checksum is malformed',
  // The settings: 外观, 通用, 提醒 and 数据
  允许折叠: 'Allow folding', '卡片折成两行，点卡片就地展开': 'Cards fold to two lines and open where they are', 外观: 'Appearance', 通用: 'General', 提醒: 'Alerts', 跟随系统: 'System', 浅色: 'Light', 深色: 'Dark', 实色: 'Solid',
  磨砂: 'Frosted', 玻璃: 'Glass', 显示: 'Display', '语言 Language': 'Language', 金额显示: 'Currency', '美元（$）': 'US dollar ($)',
  '人民币（¥）': 'Chinese yuan (¥)', '汇率…': 'Exchange rate…', 美元: 'US dollar', 人民币: 'Chinese yuan', 每天自动更新: 'Updated daily',
  手动: 'set by hand', 菜单栏显示: 'Menu bar', 只显示星星: 'Star only', '今天的 API 估算': 'Today\'s API estimate',
  最紧张的订阅额度: 'Tightest subscription limit', 钉住时刷新: 'Refresh when pinned', 用量提醒: 'Usage alerts',
  重置前提醒: 'Reset reminder', 服务状态: 'Service status', 数据: 'Data', 保留已清理记录的用量: 'Keep usage from cleaned-up logs', 导出用量: 'Export usage',
  'CSV（每天每个工具一行）…': 'CSV (a row per tool per day)…', 'JSON（每个工具的合计与每天）…': 'JSON (each tool\'s totals and days)…',
  模块与配置: 'Modules & setup', 自动检测: 'Auto-detect', '开始检测…': 'Start detecting…', '高级配置文件…': 'Advanced config file…',
  恢复默认模块: 'Restore default modules', 关闭: 'Off',
  主页顺序: 'Homepage order', 自定义: 'Custom', 恢复默认顺序: 'Restore the default order',
  '在主页按住一行，拖到想要的位置；键盘选中一行后按 ⌥↑ 或 ⌥↓。订阅、API 和其他各自排序，工具卡片按用量排':
    'Hold a row on the homepage and drag it where you want it, or select it with the keyboard and press ⌥↑ or ⌥↓. Plans, APIs and the rest each keep their own order; tool cards go by usage',
  '在主页按住一行，拖到想要的位置；键盘选中一行后按 Alt+↑ 或 Alt+↓。订阅、API 和其他各自排序，工具卡片按用量排':
    'Hold a row on the homepage and drag it where you want it, or select it with the keyboard and press Alt+↑ or Alt+↓. Plans, APIs and the rest each keep their own order; tool cards go by usage',
  '把新找到的加到主页，已勾选的不变；不读取对话正文，不显示 Key': 'Adds what it newly finds to the homepage and keeps what is ticked; reads no conversations and shows no keys',
  没有新找到的模块: 'Nothing new found', '这台电脑上能找到的都已经在主页上了。': 'Everything found on this computer is on the homepage already.',
  '只加不减：已勾选的模块保持不变。只检查本机目录与已有配置，不读取对话正文，不显示 Key。':
    'Only adds: the modules ticked stay as they are. Looks at folders on this computer and the settings saved, reads no conversations and shows no keys.',
  加到主页: 'Add to the homepage', 重新查找: 'Look again',
  '刚才找到了，已加到主页': 'Found just now, and added to the homepage', '刚才找到了，已在主页上': 'Found just now; it is on the homepage',
  '刚才没找到：装好、用过一次后再查，或选记录目录': 'Not found just now: install it and use it once, then look again, or choose its folder',
  '来源 · Gemini CLI 与 Antigravity': 'Source · Gemini CLI and Antigravity', '含 Antigravity 的 App、agy CLI 与 IDE': "Antigravity's app, agy CLI and IDE included",
  '未发现 Gemini CLI 或 Antigravity 的记录': 'No Gemini CLI or Antigravity records found',
  'API 等值估算按这个汇率换成人民币；订阅月费与 API 实际花费仍是美元': 'API-equivalent estimates use this rate for yuan; subscription prices and actual API spend stay in dollars',
  '每周窗口用了不到 85%，重置前 12 小时提醒': 'A reminder 12 hours before a weekly window resets with under 85% used',
  订阅窗口用到所选比例时通知一次: 'A notification once a subscription window reaches the chosen share',
  '每 5 分钟查一次 Claude、OpenAI、Cursor 的官方状态页，出问题时标在卡片上': 'Checks the Claude, OpenAI and Cursor status pages every 5 minutes and marks problems on the cards',
  '工具删掉旧记录后仍计入“全部”等统计': 'Usage stays in All time and the other ranges after a tool deletes its old logs',
  点卡片就地展开: 'Cards open in place',
  // Module setup
  概览: 'Overview', 用量热力图: 'Usage heatmap', '工具 Token 用量': 'Tool token usage', 订阅用量: 'Plan usage', 其他: 'Other',
  配置目录: 'Config folder', 管理地址: 'Admin address', 管理令牌文件: 'Admin token file', '选择配置目录…': 'Choose config folder…',
  '修改管理地址…': 'Change admin address…', '选择管理令牌文件…': 'Choose admin token file…', 恢复自动读取: 'Back to automatic',
  '在 Finder 中打开': 'Show in Finder', 'Magpie 本机网关记录': 'Magpie\'s local gateway logs', '只读取 usage.jsonl': 'Reads only usage.jsonl',
  读取目录: 'Folder read', '选择记录目录…': 'Choose log folder…', 恢复自动查找: 'Find it automatically again',
  'Claude 桌面 App 与 Claude Code': 'the Claude desktop app and Claude Code', '含 VS Code、JetBrains 与终端里的会话': 'Including sessions in VS Code, JetBrains and the terminal',
  '选择 Claude 数据目录…': 'Choose Claude data folder…', 编程客户端的本机记录: 'the coding clients\' local records',
  '仅编程会话，不含普通聊天': 'Coding sessions only, not regular chats', 自动读取本机客户端: 'local clients, read automatically',
  未发现客户端或用量记录: 'No client or usage records found', '读取 Cursor 用量记录': 'Read Cursor usage records',
  'Cursor 桌面版与 Cursor CLI': 'Cursor desktop and Cursor CLI', '两者同一账户，用量都在账户记录里': 'Both use one account, and its records hold the usage',
  '用本机 Cursor 登录查询 cursor.com，令牌只在内存中使用': 'Asks cursor.com with the local Cursor sign-in; the token stays in memory',
  登录来源: 'Sign-in source', 当前来源: 'Current source', '本机 ChatGPT 登录': 'local ChatGPT sign-in',
  '勾选 OpenCodex 后改用 OpenCodex 账户': 'Uses the OpenCodex account once OpenCodex is on', 本机登录目录: 'Local sign-in folder',
  'Claude Code 登录查询': 'Ask with the Claude Code sign-in', 'Code 登录来源': 'Code sign-in source', '启用本机用量回传…': 'Turn on local usage relay…',
  'App 快照文件': 'App snapshot file', 日志目录: 'Log folder', 面板地址: 'Panel address', '设置面板地址…': 'Set panel address…',
  '清除连接设置…': 'Clear connection…', 在主页显示: 'Show on the homepage', '已启用 {n} 项': '{n} on', 主页模块: 'Home modules',
  'Harness Token 用量': 'Harness token usage', 订阅: 'Subscriptions',
  // Errors the engine writes after 本次读取失败：
  本次读取失败: 'This read failed', '令牌已过期，请在 Claude Code 里运行 /login': 'The token expired; run /login in Claude Code',
  网络连接超时: 'The connection timed out', 保留最近一次成功的数据: 'keeping the last good data',
  // Share page
  风格: 'Style', 标准: 'Standard', 色彩: 'Colours', 工具数量: 'Tools',
  // 新手指引
  帮助: 'Help', 新手指引: 'Quick tour', 界面语言: 'Language', 跳过: 'Skip', 开始: 'Start', 上一步: 'Back', 下一步: 'Next',
  '欢迎使用 Starmeter': 'Welcome to Starmeter',
  '它读取 AI 工具已经留在这台电脑上的用量记录，在一个面板里告诉你用了多少。不需要账号，任何内容都不会上传。花半分钟认识一下。':
    'It reads the usage records your AI tools already keep on this computer and shows, in one panel, how much you use. No account, and nothing is uploaded. Here is a 30-second tour.',
  用量总览: 'Your usage at a glance',
  '最上面是这段时间的 API 等值估算、Token 和请求数，下面是趋势和热力图。估算按官方价格计算，不是你的账单。':
    'At the top: the API-equivalent estimate, tokens and requests for the range; below, the trend and the heatmap. Estimates use official prices; they are not your bill.',
  '从今天到全部共 8 种范围，整页跟着切换。': 'Eight ranges, from today to all time; the whole page follows.',
  每个工具一张卡: 'A card for each tool',
  '卡片列出每个工具用的模型和用量，点开是它的详情页。也可以按模型、项目分组；“会话”里是正在运行的 Claude Code 和 Codex 会话。':
    'Each card lists a tool\'s models and usage; click it for the tool\'s own page. Group by model or project instead, or open Sessions for the Claude Code and Codex sessions at work.',
  '订阅额度和 API 余额': 'Plans and API balances',
  '带箭头的行点开是详情：每个额度窗口用了多少、何时重置，账户余额，以及这一项的设置。':
    'A row with an arrow opens its details: how much of each window is used and when it resets, balances, and the settings for that item.',
  三个按钮: 'Three buttons',
  '图钉：钉住后面板浮在最上层并实时刷新，可以拖动、调大小。齿轮：选择主页显示哪些模块，设置目录、Key、语言和外观。分享：把用量做成一张图。':
    'Pin: keep the panel on top, refreshing live; drag it and resize it. Gear: choose what the homepage shows, and set folders, keys, language and look. Share: turn your usage into a picture.',
  '以后可以在 齿轮 → 通用 → 新手指引 再看一遍。': 'You can take this tour again from the gear → General → Quick tour.',
  '最上面是这段时间的 API 等值估算、Token 和请求数，下面是趋势和热力图。第一次读取要一点时间，读到以后数字会自己出现。':
    'At the top: the API-equivalent estimate, tokens and requests for the range; below, the trend and the heatmap. The first reading takes a moment; the figures appear by themselves.',
  还没有用量: 'No usage yet',
  '这台电脑上还没找到 AI 编程工具的用量记录。Starmeter 读的是 Claude Code、Codex、Cursor、Gemini 等工具自己留在本机的记录：装好其中一个、用过一次，这里就会出现数字。':
    'No AI coding tool has left usage records on this computer yet. Starmeter reads the records Claude Code, Codex, Cursor, Gemini and others keep here: install one and use it once, and the figures appear.',
  选择要读的工具: 'Choose your tools',
  '点齿轮打开“主页模块”，勾选你在用的工具；工具的记录不在默认位置时，点它的名字选文件夹。“通用”里的“自动检测”会把这台电脑上找得到的都勾上。':
    'The gear opens Home modules: tick the tools you use, and click a tool\'s name to pick its folder when its records live elsewhere. Auto-detect in General ticks every tool it finds on this computer.',
  全局快捷键: 'Global shortcut', 不使用: 'Off', '在任何地方按下即可打开或收起面板': 'Opens or closes the panel from anywhere',
  快捷键已被占用: 'Shortcut taken', 当前被占用: 'taken by another app',
  '这个组合键现在被系统或其他程序占用，按了没反应；换一个，或退出占用它的程序后再打开一次面板': 'The system or another app holds this combination now, so it does nothing; pick another, or quit that app and open the panel again', '这个组合键已被系统或其他程序占用，换一个试试。': 'The system or another app already uses this combination; try another.',
  // 多台电脑合并
  // 在手机上看
  // Short enough for its cell in the module grid.
  在手机上看: 'Phone view', 扫码在手机浏览器里看: 'Scan with your phone',
  '外网链接断开了，可以重新生成': 'The public link dropped; you can make a new one',
  '这个网络的 DNS 查不到 Cloudflare 隧道的地址（常见于代理或 VPN），换个网络或关掉代理再试':
    'This network\'s DNS can\'t find Cloudflare\'s tunnel servers (common behind a proxy or VPN); try another network or turn the proxy off',
  'Cloudflare 暂时不给新的临时链接，稍后再试': 'Cloudflare isn\'t giving out new temporary links right now; try again later',
  'Cloudflare 这次没有连上，稍后再试': 'Couldn\'t connect to Cloudflare this time; try again later',
  '连不上电脑：电脑可能睡着了、换了网络，或者已经停止手机查看': 'Can\'t reach the computer: it may be asleep, on another network, or no longer sharing with the phone',
  '没能复制，下面是完整链接，选中后手动复制': 'Couldn\'t copy: here is the whole link to copy by hand', 完整链接: 'Whole link',
  '外网链接连不上电脑：电脑可能睡着了、断了网，或者外网链接已关闭': 'The public link can\'t reach the computer: it may be asleep or offline, or the link was closed',
  '链接已失效：在电脑上重新打开“在手机上看”，再扫一次码': 'This link no longer works: open Phone view on the computer again and scan the new code',
  开着: 'On', 手机在哪: 'Where the phone is', 同一个网络: 'Same network', 任何网络: 'Any network',
  '经 Cloudflare 生成外网链接？': 'Make a public link through Cloudflare?',
  '手机不在同一个网络时用：Cloudflare 给一个临时的 https 地址，转到这台电脑。主页的数字会经过 Cloudflare；链接带一次性口令，停止或一小时后失效。':
    'For a phone on another network: Cloudflare gives a temporary https address that leads to this computer. The homepage\'s figures pass through Cloudflare; the link carries a one-time key and stops working when you stop it, or after an hour.',
  '第一次要从 Cloudflare 的 GitHub 发布页下载 cloudflared（约 20 MB），校验后存在 App 的数据文件夹。':
    'The first time, cloudflared (about 20 MB) is downloaded from Cloudflare\'s GitHub releases, checked, and kept in the app\'s data folder.',
  下载并生成: 'Download and make it', 生成链接: 'Make the link', '正在向 Cloudflare 要链接…': 'Asking Cloudflare for a link…',
  手机扫这个二维码: 'Scan this with your phone', 这台电脑没有连上网络: 'This computer isn\'t on a network', 正在开启: 'Starting',
  已复制链接: 'Link copied', '任何网络都能打开，链接带一次性口令，还能用 {n} 分钟': 'Opens from any network with its one-time key; {n} min left',
  '手机和电脑连同一个网络（Wi-Fi）后扫码；第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许':
    'Put the phone on the same network (Wi-Fi) as this computer, then scan; the first time, the system may ask whether Starmeter may accept incoming connections: allow it.',
  '只读：手机上只能看主页和工具页，不能改设置。点“停止”或退出 App 前一直开着。': 'Read-only: the phone sees the homepage and tool pages and changes nothing. It stays on until you stop it or quit the app.',
  关闭外网链接: 'Close the public link', 停止: 'Stop', 已停止: 'Stopped', 重新开始: 'Start again',
  手机上的页面已断开: "The phone's page is disconnected",
  没能开启: "Couldn't start", '原因在下面，可以再试一次': 'See why below, then try again', 重试: 'Try again',
  '中国大陆部分地区打不开 Cloudflare 的链接。手机和电脑能连同一个 Wi-Fi 时，用“同一个网络”更稳。': 'In parts of mainland China, Cloudflare links don\'t open. When the phone can join the same Wi-Fi, Same network is more reliable.',
  '原来的二维码和链接已失效。点“重新开始”生成新的二维码，手机再扫一次就能看。': 'The old code and link no longer open. Start again for a new code, and scan it once more.',
  先打开手机查看: 'Turn on the phone view first', 手机查看已经关闭: 'The phone view was stopped',
  这台电脑的系统没有对应的: 'No cloudflared for this system', '连不上 GitHub，没能下载 cloudflared': 'Couldn\'t reach GitHub to download cloudflared',
  'GitHub 的发布信息读不懂': 'GitHub\'s release information couldn\'t be read', '最新发布里没有这台电脑用的 cloudflared': 'The latest release has no cloudflared for this computer',
  'cloudflared 的下载信息不完整，没有下载': 'cloudflared\'s download information was incomplete; nothing downloaded', '下载 cloudflared 失败': 'Downloading cloudflared failed',
  '下载的 cloudflared 校验不通过，已丢弃': 'The downloaded cloudflared failed its check and was thrown away', 'cloudflared 解压失败': 'cloudflared couldn\'t be unpacked',
  'Cloudflare 这次没有给出链接，稍后再试': 'Cloudflare gave no link this time; try again later', '这台电脑的系统没有对应的 cloudflared': 'No cloudflared for this system',
  多台电脑合并: 'Combine computers', '含 {n} 台电脑': '{n} computers', 未设置: 'Not set', 选择同步文件夹: 'Choose a synced folder', '选择同步文件夹…': 'Choose a synced folder…', 同步文件夹: 'Synced folder',
  '本机名称…': 'This computer\'s name…', 本机名称: 'This computer\'s name', 停止合并: 'Stop combining', 刚刚同步: 'Synced just now',
  '其他电脑选同一个文件夹后会出现在这里': 'Other computers appear here once they choose the same folder',
  '每台电脑都选同一个同步文件夹（iCloud 云盘、Dropbox、OneDrive…），各自写一份用量，合起来显示；文件里只有工具、模型和用量数字':
    'Choose the same synced folder (iCloud Drive, Dropbox, OneDrive…) on each computer: each writes its usage there and they add up. The files hold only tools, models and counts.',
  '停止合并多台电脑？': 'Stop combining computers?', 其他电脑上显示的名字: 'The name other computers show',
  '这台电脑的用量文件会从同步文件夹里删掉，其他电脑不再计入它；这台电脑也不再计入其他电脑。':
    'This computer\'s usage file leaves the synced folder, so the others stop counting it, and this computer stops counting theirs.',
  打开主页模块: 'Open Home modules', 稍后: 'Later', 尚未勾选工具: 'No tools chosen yet',
  复制图片: 'Copy picture', 已复制: 'Copied', 复制失败: 'Copy failed', '保存…': 'Save…', 已保存: 'Saved',
  '查看更多 {n} 个会话': 'Show {n} more', '查看更多 {n} 个模型': 'Show {n} more', '查看更多 {n} 个项目': 'Show {n} more', '分享…': 'Share…', 少一个: 'One fewer', 多一个: 'One more', 保存图片: 'Save picture', 长按图片也能存到相册: 'or long-press the picture to save it to Photos', 没能画出来: 'Couldn\'t draw it', 正在生成: 'Drawing', 卡片: 'Card', 终端: 'Terminal', 手绘: 'Sketch', 报刊: 'Paper',
  夜空: 'Night sky', 地铁: 'Metro', 织物: 'Knit', 收据: 'Receipt', 蓝图: 'Blueprint', 霓虹: 'Neon',
  // Questions
  取消: 'Cancel', 确定: 'OK', 好: 'OK', 继续: 'Continue',
  保存: 'Save', 设置: 'Settings',
  // Share pictures
  '{style}风格的用量图预览': 'Usage picture preview, {style} style', 时间范围: 'Range', 像素: 'pixels', '其他 {n} 个模型': '{n} more models', '其他 {n} 个模型|1': '1 more model',
  含未定价: 'some unpriced', '占 {n}': '{n} of all', 其他工具: 'Other tools', '{range} AI 用量': 'AI usage · {range}', 今日: 'Today',
  昨日: 'Yesterday', 累计: 'All time', '已知 API 等值估算': 'Known API-equivalent estimate', 'API 等值估算': 'API-equivalent estimate',
  缓存命中: 'Cache hits', '等 {n} 个': 'and {n} more', '图 1 · ': 'Fig. 1 · ', 总量: 'Total',
  // The panel's own labels
  '近 1 分钟 +{n} Token · 输出 ': 'Last min +{n} · output ', '收起 {name} 明细': 'Fold {name}', '展开 {name} 明细': 'Unfold {name}',
  '{name} 详情': '{name} details', 此时段暂无模型调用: 'No model calls in this range', '全部来自 {name}': 'All from {name}',
  正在进行: 'In progress', 卡片分组: 'Card grouping', 'AI 用量面板': 'AI usage panel', '在主页显示 {name}': 'Show {name} on the homepage',
  拖动调整大小: 'Drag to resize', '已知 {n}': 'Known {n}',
};

type Rule = [RegExp, (m: RegExpMatchArray, tr: (text: string) => string) => string];
/** A range after "active": today, yesterday, this week; in the last 7 days. */
const during = (range: string) => (/^Last /.test(range) ? `in the ${range.toLowerCase()}` : range.toLowerCase());

const time = (zh: string) => zh.replace(/^周([一二三四五六日天])/, (_, d: string) => WEEKDAYS[d]);

/** A plan's window and its share inside “Used: …”: a window with a common name (本周, 本周期) is written in lower case
 *  there (“Used: this week 38% · 5 hours 12%”); a name that starts as a model's or an account's own (Fable, Claude 和
 *  GPT) keeps its capitals. */
function windowShare(part: string, tr: (text: string) => string): string {
  const named = part.match(/^(.+) (\d+(?:\.\d+)?%)$/);
  if (!named || !/[\u3400-\u9fff]/.test(named[1]) || /^[A-Za-z]/.test(named[1])) return tr(part);
  const name = tr(named[1]);
  return `${name.charAt(0).toLowerCase()}${name.slice(1)} ${named[2]}`;
}

/** Rules for the whole text, before it is split into parts: a share used across windows or accounts, the time a
 *  chart was read. */
export const whole: Rule[] = [
  [/^已用 (.+ · .+)$/, (m, tr) => `Used: ${m[1].split(' · ').map((part) => windowShare(part, tr)).join(' · ')}`],
  [/^更新时间：(.+)$/, (m) => `Updated ${m[1]}`],
];

/** One part of the engine's text. */
export const rules: Rule[] = [
  // Shares used: one window, or one as of a time.
  [/^已用 (\d+(?:\.\d+)?%)$/, (m) => `${m[1]} used`],
  [/^已用 (\d+(?:\.\d+)?%) 截至 (.+)$/, (m) => `${m[1]} used as of ${m[2]}`],
  [/^已用 (.+)$/, (m, tr) => `Used: ${tr(m[1])}`],
  [/^(\d+) 个账户$/, (m) => `${m[1]} accounts`],
  [/^(\d+) 台电脑$/, (m) => `${m[1]} ${plural(m[1], 'computer', 'computers')}`],
  [/^还剩 (\d+) 分钟$/, (m) => `${m[1]} min left`],
  [/^电脑暂时没有返回数据（HTTP (\d+)），稍后会自动重试$/, (m) => `The computer sent nothing this time (HTTP ${m[1]}); trying again shortly`],
  [/^(\d+) 分钟前同步$/, (m) => `Synced ${m[1]} min ago`],
  [/^(\d+) 小时前同步$/, (m) => `Synced ${m[1]} h ago`],
  [/^(\d+) 天前同步$/, (m) => `Synced ${m[1]} ${plural(m[1], 'day', 'days')} ago`],
  [/^(\d+) 小时$/, (m) => `${m[1]} ${plural(m[1], 'hour', 'hours')}`],
  [/^重置于 (.+)$/, (m) => `Resets ${time(m[1])}`],
  [/^比匀速多用 (.+)$/, (m) => `${m[1]} ahead of even pace`],
  [/^比匀速少用 (.+)$/, (m) => `${m[1]} behind even pace`],
  [/^本月回本 (.+) 倍$/, (m) => `Paid back ${m[1]}× this month`],
  [/^本月 API 等值 (.+)$/, (m) => `API equivalent this month ${m[1]}`],
  [/^月费 (.+)$/, (m) => `Monthly ${m[1]}`],
  [/^剩余 (.+)$/, (m) => `${m[1]} left`],
  [/^余 (.+) Credits$/, (m) => `${m[1]} credits left`],
  [/^可免费重置 (\d+) 次$/, (m) => `${m[1]} free ${plural(m[1], 'reset', 'resets')}`],
  [/^可用 (\d+) 次$/, (m) => `${m[1]} available`],
  [/^最早 (.+) 过期 · 在 Codex 里使用$/, (m) => `First expires ${m[1]} · use it in Codex`],
  [/^额外用量剩 (.+)$/, (m) => `${m[1]} of extra usage left`],
  [/^按需剩 (.+)$/, (m) => `${m[1]} of on-demand left`],
  [/^到期 (.+)$/, (m) => `Expires ${m[1]}`],
  [/^(.+) 订阅$/, (m, tr) => `${tr(m[1])} plan`],
  [/^需勾选 (.+)$/, (m, tr) => `Needs ${tr(m[1])} on`],
  [/^加到主页：(.+)$/, (m, tr) => `Add to the homepage: ${m[1].split('、').map(tr).join(', ')}`],
  [/^Antigravity 没有给出额度（HTTP (\d+)）$/, (m) => `Antigravity gave no quota (HTTP ${m[1]})`],
  // Updated and when
  [/^(\d+) 分钟前更新$/, (m) => `${m[1]} min ago`],
  [/^(\d+) 小时前更新$/, (m) => `${m[1]} h ago`],
  [/^(\d+) 天前更新$/, (m) => `${m[1]} ${plural(m[1], 'day', 'days')} ago`],
  [/^(\d+) 分钟前$/, (m) => `${m[1]} min ago`],
  [/^更新时间：(.+)$/, (m) => `Updated ${m[1]}`],
  [/^实时 (.+)$/, (m) => `Live ${m[1]}`],
  // Money
  [/^余额 (.+)$/, (m) => `Balance ${m[1]}`],
  [/^本月已用 (.+)$/, (m) => `${m[1]} this month`],
  [/^今天已用 (.+)$/, (m) => `${m[1]} today`],
  [/^已知 (.+)$/, (m) => `Known ${m[1]}`],
  // Cards
  [/^(\d+) 次已由 (.+) 计入$/, (m) => `${m[1]} counted by ${m[2]}`],
  [/^(\d+) 次未定价$/, (m) => `${m[1]} unpriced`],
  [/^(\d+) 次请求$/, (m) => `${m[1]} ${plural(m[1], 'request', 'requests')}`],
  [/^其他 (\d+) 个(模型|项目|工具)$/, (m) => `${m[1]} more ${m[2] === '模型' ? 'models' : m[2] === '项目' ? 'projects' : 'tools'}`],
  [/^已定价 (.+)$/, (m) => `${m[1]} priced`],
  [/^每 (\d+) 天$/, (m) => `Every ${m[1]} days`],
  [/^每 (\d+) 秒$/, (m) => (m[1] === '1' ? 'Every second' : `Every ${m[1]} seconds`)],
  [/^图表展示最近 (\d+) 天$/, (m) => `The chart shows the last ${m[1]} days`],
  [/^(.+) 不可读$/, (m) => `${m[1]} can't be read`],
  // Heatmap
  [/^每格 (\d+) 小时$/, (m) => `${m[1]} ${plural(m[1], 'hour', 'hours')} per dot`],
  [/^每格 (\d+) 天$/, (m) => `${m[1]} ${plural(m[1], 'day', 'days')} per dot`],
  [/^(\d+)小时$/, (m) => `${m[1]}h`],
  [/^(\d+)天$/, (m) => `${m[1]}d`],
  [/^(\d+)月$/, (m) => MONTHS[Number(m[1]) - 1] ?? m[0]],
  [/^(\d+)月(\d+)日(.*)$/, (m) => `${MONTHS[Number(m[1]) - 1]} ${m[2]}${m[3]}`],
  [/^连续 (\d+) 天$/, (m) => `${m[1]}-day streak`],
  [/^最长 (\d+) 天$/, (m) => `longest ${m[1]}`],
  [/^最长连续 (\d+) 天$/, (m) => `Longest streak ${m[1]} days`],
  // Sessions
  [/^(\d+) 运行中$/, (m) => `${m[1]} running`],
  [/^(\d+) 空闲$/, (m) => `${m[1]} idle`],
  [/^(\d+) 等待输入$/, (m) => `${m[1]} waiting`],
  [/^最近 (\d+) 轮$/, (m) => (m[1] === '1' ? 'Last turn' : `Last ${m[1]} turns`)],
  // A session's whole cost over its turns, and the range its card's figure counts.
  [/^累计 (.+)$/, (m, tr) => `Total ${tr(m[1]).replace(/^Known /, 'known ')}`],
  [/^金额只算(.+)$/, (m, tr) => `amounts cover ${tr(m[1]).toLowerCase()}`],
  // Which sessions 会话 lists: those active in the range shown (近 30 天 under 全部).
  [/^(.+)活动过的会话$/, (m, tr) => `Sessions active ${during(tr(m[1]))}`],
  [/^(.+)没有会话$/, (m, tr) => `No sessions ${during(tr(m[1]))}`],
  [/^运行中 (.+)$/, (m, tr) => `Running ${tr(m[1])}`],
  [/^空闲 (.+)$/, (m, tr) => `Idle ${tr(m[1])}`],
  [/^(\d+) 分钟$/, (m) => `${m[1]} min`],
  [/^上下文 (.+)$/, (m) => `Context ${m[1]}`],
  // Monitoring and updates
  [/^(\d+\/\d+) 在线$/, (m) => `${m[1]} online`],
  [/^内存 (.+)$/, (m) => `Memory ${m[1]}`],
  [/^最后在线 (.+)$/, (m) => `Last online ${m[1]}`],
  [/^(\d+) 项待更新$/, (m) => `${m[1]} to update`],
  [/^(\d+) 项$/, (m) => m[1]],
  [/^点击运行 (.+)$/, (m) => `Click to run ${m[1]}`],
  [/^更新到 (.+)$/, (m) => `Update to ${m[1]}`],
  [/^重试更新到 (.+)$/, (m) => `Retry the update to ${m[1]}`],
  [/^可更新 (.+)$/, (m) => `${m[1]} available`],
  [/^当前 (.+) → 最新 (.+)$/, (m) => `Current ${m[1]} → latest ${m[2]}`],
  [/^已自动更新至 (.+)$/, (m) => `Updated automatically to ${m[1]}`],
  [/^(.+) 返回 HTTP (\d+)$/, (m) => `${m[1]}: HTTP ${m[2]}`],
  [/^找不到版本 (.+) 的发布$/, (m) => `No release for version ${m[1]}`],
  [/^安装程序退出码 (-?\d+)$/, (m) => `The installer exited with code ${m[1]}`],
  [/^安装包里的版本不是 (.+)$/, (m) => `The package isn't version ${m[1]}`],
  [/^不能写入 (.+)，请从发布页下载安装$/, (m) => `Can't write to ${m[1]}; download it from the release page`],
  [/^当前版本 (.+)。将从 GitHub 下载这一版的安装包，校验 SHA-256 后替换本机的 Starmeter，并自动重新打开；设置、Key 和统计记录都会保留。$/, (m) => `Now ${m[1]}. The new version's package comes from GitHub and, once its SHA-256 checks out, takes the place of this Starmeter, which opens again; settings, keys and usage records stay.`],
  // Rows that open something
  [/^打开 (.+)$/, (m, tr) => `Open ${tr(m[1])}`],
  // A name and its share (a window of a plan, a client): last, after the rules that know their own names.
  [/^(.+) (\d+(?:\.\d+)?%)$/, (m, tr) => `${tr(m[1])} ${m[2]}`],
];
