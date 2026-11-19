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
  未定价: 'Unpriced', 刷新失败: 'Refresh failed', 保留原数据: 'keeping earlier data', 部分记录未读取: 'Some records weren\'t read',
  读取失败: 'Read failed', 数据暂不可用: 'Data unavailable for now', '未计入 Summary': 'Not in Summary', 'Token 未记录': 'Tokens not logged',
  '已由 Magpie 计入': 'Counted by Magpie', 'ChatGPT 对话': 'ChatGPT chats', 'ChatGPT 项目': 'ChatGPT project', 未标注项目: 'No project',
  未标注: 'Unlabelled', 服务部分降级: 'Partial outage', 服务大范围故障: 'Major outage', 服务严重故障: 'Critical outage', 服务维护中: 'Maintenance',
  客户端: 'Clients', '{n} 请求': '{n} requests', '{n} 请求|1': '{n} request', '{n} 会话': '{n} sessions', '{n} 会话|1': '{n} session', '输出 {n}': 'Output {n}', '缓存 —': 'Cache —',
  '缓存 {n}%': 'Cache {n}%', '已定价 {n}%': '{n}% priced', '{n} 次未定价': '{n} unpriced', 缓存: 'Cache', 输出: 'Output', 费用: 'Cost', 会话数: 'Sessions', 输入: 'Input', 缓存读: 'Cache read', 缓存写: 'Cache write',
  // Heatmap
  暂无用量: 'No usage yet', 无用量: 'No usage', 部分小时数据不可用: 'Some hourly data is unavailable', 每格: 'per square',
  // Subscriptions
  来源: 'Source', 本机登录: 'local sign-in', 编程工具额度: 'coding limits', '5 小时': '5 hours', 更新时间: 'Updated',
  '订阅月费…': 'Monthly price…', 'Code 登录': 'Code sign-in', 额外用量: 'Extra usage', 立即刷新用量: 'Refresh usage now',
  打开订阅用量: 'Open subscription usage', 'Cursor 账户（cursor.com）': 'Cursor account (cursor.com)', 本周期: 'This cycle',
  套餐额度: 'Plan allowance', 按需用量: 'On-demand usage', 打开用量与账单: 'Open usage and billing',
  'Qoder 桌面 App 的本机日志': 'the Qoder desktop app\'s local logs', '套餐 Credits': 'Plan Credits', '加购 Credits': 'Add-on Credits',
  '在 Qoder 窗口右下角打开 Credits 用量后更新': 'Updates once you open Credits usage at the bottom right of the Qoder window',
  '手动标记，未验证套餐': 'Marked by hand; the plan isn\'t verified', 本机标记文件: 'Local mark file', 清除配置标记: 'Clear the mark',
  打开订阅管理: 'Open subscription settings', 刚刚更新: 'Just updated', 未开启: 'Off', 未填写: 'Not set', 已标记: 'Marked',
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
  没有会话: 'No sessions', '最近半小时没有 Claude Code 或 Codex 会话': 'No Claude Code or Codex sessions in the last half hour',
  运行中: 'Running', 空闲: 'Idle', 本轮: 'This turn', 上一轮: 'Last turn', 等待输入: 'Waiting',
  // Monitoring and updates
  'Komari 监控': 'Komari status', '打开 Komari 面板': 'Open the Komari panel', 刷新监控数据: 'Refresh monitoring data', 在线: 'Online',
  离线: 'Offline', '到期 长期': 'No expiry', 工具更新: 'Tool updates', 立即检查更新: 'Check for updates now',
  '查看最近 CLI 更新日志': 'Show the latest CLI update log', '选择 Harness…': 'Choose Harnesses…', 'Starmeter · 可更新': 'Starmeter · Update available',
  更新记录: 'Changelog', '点击打开 App 更新': 'Click to update in the app', 'Claude 官方': 'Official', 'Homebrew Cask': 'Homebrew Cask',
  已是最新: 'Up to date', '在 App 内检查更新': 'Checked in the app', '检查失败：npm 无响应，稍后重试': 'Check failed: npm didn\'t answer; trying again later',
  // 外观与设置
  允许折叠: 'Allow folding', 外观与设置: 'Settings', 跟随系统: 'System', 浅色: 'Light', 深色: 'Dark', 实色: 'Solid',
  磨砂: 'Frosted', 玻璃: 'Glass', 显示: 'Display', '语言 Language': 'Language', 金额显示: 'Currency', '美元（$）': 'US dollar ($)',
  '人民币（¥）': 'Chinese yuan (¥)', '汇率…': 'Exchange rate…', 美元: 'US dollar', 人民币: 'Chinese yuan', 每天自动更新: 'Updated daily',
  手动: 'set by hand', 菜单栏显示: 'Menu bar', 只显示星星: 'Star only', '今天的 API 估算': 'Today\'s API estimate',
  最紧张的订阅额度: 'Tightest subscription limit', 钉住与提醒: 'Pinning & alerts', 钉住时刷新: 'Refresh when pinned', 用量提醒: 'Usage alerts',
  重置前提醒: 'Reset reminder', 服务状态: 'Service status', 数据: 'Data', 保留已清理记录的用量: 'Keep usage from cleaned-up logs', 导出用量: 'Export usage',
  'CSV（每天每个工具一行）…': 'CSV (a row per tool per day)…', 'JSON（每个工具的合计与每天）…': 'JSON (each tool\'s totals and days)…',
  模块与配置: 'Modules & setup', 自动检测: 'Auto-detect', '开始检测…': 'Start detecting…', '高级配置文件…': 'Advanced config file…',
  恢复默认模块: 'Restore default modules', 关闭: 'Off',
  '确认后覆盖当前模块选择；不读取对话正文，不显示 Key': 'Replaces the current modules once you confirm; reads no conversations and shows no keys',
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
  风格: 'Style', 色彩: 'Colours', 复制图片: 'Copy picture', 已复制: 'Copied', 复制失败: 'Copy failed', '保存…': 'Save…', 已保存: 'Saved',
  '分享…': 'Share…', 没能画出来: 'Couldn\'t draw it', 正在生成: 'Drawing', 卡片: 'Card', 终端: 'Terminal', 手绘: 'Sketch', 报刊: 'Paper',
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
  '近 1 分钟 +{n} Token · 输出 ': 'Last minute +{n} Token · output ', '收起 {name} 明细': 'Fold {name}', '展开 {name} 明细': 'Unfold {name}',
  '{name} 详情': '{name} details', 此时段暂无模型调用: 'No model calls in this range', '全部来自 {name}': 'All from {name}',
  正在进行: 'In progress', 卡片分组: 'Card grouping', 'AI 用量面板': 'AI usage panel', '在主页显示 {name}': 'Show {name} on the homepage',
  拖动调整大小: 'Drag to resize', '已知 {n}': 'Known {n}',
};

type Rule = [RegExp, (m: RegExpMatchArray, tr: (text: string) => string) => string];

const time = (zh: string) => zh.replace(/^周([一二三四五六日天])/, (_, d: string) => WEEKDAYS[d]);

/** Rules for the whole text, before it is split into parts: a share used across windows or accounts, the time a
 *  chart was read. */
export const whole: Rule[] = [
  [/^已用 (.+ · .+)$/, (m, tr) => `Used: ${m[1].split(' · ').map(tr).join(' · ')}`],
  [/^更新时间：(.+)$/, (m) => `Updated ${m[1]}`],
];

/** One part of the engine's text. */
export const rules: Rule[] = [
  // Shares used: one window, or one as of a time.
  [/^已用 (\d+(?:\.\d+)?%)$/, (m) => `${m[1]} used`],
  [/^已用 (\d+(?:\.\d+)?%) 截至 (.+)$/, (m) => `${m[1]} used as of ${m[2]}`],
  [/^已用 (.+)$/, (m, tr) => `Used: ${tr(m[1])}`],
  [/^(\d+) 个账户$/, (m) => `${m[1]} accounts`],
  [/^(\d+) 小时$/, (m) => `${m[1]} hours`],
  [/^重置于 (.+)$/, (m) => `Resets ${time(m[1])}`],
  [/^比匀速多用 (.+)$/, (m) => `${m[1]} ahead of even pace`],
  [/^比匀速少用 (.+)$/, (m) => `${m[1]} behind even pace`],
  [/^本月回本 (.+) 倍$/, (m) => `Paid back ${m[1]}× this month`],
  [/^本月 API 等值 (.+)$/, (m) => `API equivalent this month ${m[1]}`],
  [/^月费 (.+)$/, (m) => `Monthly ${m[1]}`],
  [/^剩余 (.+)$/, (m) => `${m[1]} left`],
  [/^到期 (.+)$/, (m) => `Expires ${m[1]}`],
  [/^(.+) 订阅$/, (m, tr) => `${tr(m[1])} plan`],
  [/^需勾选 (.+)$/, (m, tr) => `Needs ${tr(m[1])} on`],
  // Updated and when
  [/^(\d+) 分钟前更新$/, (m) => `Updated ${m[1]} min ago`],
  [/^(\d+) 小时前更新$/, (m) => `Updated ${m[1]} h ago`],
  [/^(\d+) 天前更新$/, (m) => `Updated ${m[1]} ${plural(m[1], 'day', 'days')} ago`],
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
  [/^每格 (\d+) 小时$/, (m) => `${m[1]} ${plural(m[1], 'hour', 'hours')} a square`],
  [/^每格 (\d+) 天$/, (m) => `${m[1]} ${plural(m[1], 'day', 'days')} a square`],
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
  // Rows that open something
  [/^打开 (.+)$/, (m, tr) => `Open ${tr(m[1])}`],
  // A name and its share (a window of a plan, a client): last, after the rules that know their own names.
  [/^(.+) (\d+(?:\.\d+)?%)$/, (m, tr) => `${tr(m[1])} ${m[2]}`],
];
