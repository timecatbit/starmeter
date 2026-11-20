# Starmeter 开发交接

Claude 和 Codex 在这里交接 6.x 的开发与审查。产品约定、构建和测试命令见 [AGENTS.md](AGENTS.md)。

## 接手与回报

- 每次开始或继续本项目、准备领修、准备复审时，重读本文件和最新提交。使用独立 Git 工作树时，从 `git worktree list --porcelain` 找到 `branch refs/heads/main` 对应的 `worktree`，在那里的 `DEVELOPMENT.md` 读写交接；代码可在自己的分支修改。
- Codex 把审查意见直接登记在下表，给出复现条件和可观察的预期。Claude 领修时在同一行填「处理中 · Claude」；已有相关提交或未提交改动时先核对进展。两边都可补充新发现。
- 修复者填「待复审」、提交号和验证结果。Codex 用表内场景独立复测，记下「已验证」、所测提交和实际结果；仍能复现就填「重新打开」及实测值。更新前重读该行，只改自己负责的条目。
- 维护机上的 Codex 每 5 分钟检查本表和相关修复提交。有「待复审」就独立验收；有尚未领修的「待处理」或「重新打开」项就交给 Claude Code。已有负责人时等其交回。每轮都按下列清单做全场景审查，优先 macOS 独立版。上一轮仍在执行时接续同一轮，直到每类都有实测结果或具体的验证缺口。复现的新问题登记编号并直接交给 Claude；没有新发现时仅在本轮留下覆盖记录。

## 每轮全量审查

| 用户场景 | 核验范围 |
| --- | --- |
| 安装与更新 | 首装、升级、备份和回滚；独立 App、SwiftBar、各系统及芯片对应安装包、工具更新 |
| 首次打开与数据连接 | 指引、自动检测和覆盖确认；逐个 Harness、订阅、API、Komari 的配置、缺失、读取失败及恢复 |
| 统计与历史 | 各时间范围和时区、费用与汇率、缓存、网关去重、会话、归档及多设备合并 |
| 窗口与偏好 | 鼠标、键盘、失焦与重开、钉住、折叠、提醒、系统外观、中英文及重启后保留设置 |
| 展示与输出 | 图表、分组、模型明细、各分享样式、复制与保存、CSV 和 JSON 导出 |
| 手机与跨平台 | 本地及外网查看、失效链接与断线重连；macOS 两种芯片、Windows、Linux 原生入口 |

按当前源码运行 `scripts/check.py` 对应的全套 WebKit、Rust、Python 与对照测试。核对平台构建及安装冒烟结果所属的版本。缺少自动用例的功能补充隔离验证；区分真机、模拟宿主和代码走查，记录跳过原因。健康轮次的覆盖记录留在会话即可。

## 审查队列

以下案例来自对 `d34af15` 的 macOS 独立版、引擎和 WebKit 审查，数值均为合成测试数据。「待复审」表示找到了对应提交，需在该提交或其后代上验证。除 `4b938de`、`8bd9e4f` 外的修复都已在 6.0.1（`399885a`）里发布。

| 编号 | 状态与提交 | 问题与验收条件 |
| --- | --- | --- |
| SYNC-1 | 已验证 · Codex 复测 `3fcc5cb`：独立 App 的手机接口在首次读取和切换到模型分组后均合计 140（OpenCodex 40、另一台 ChatGPT 100）。 修复提交 `db0f7f3` `12f62aa` · Claude 自测：单测 `another_computers_chatgpt_counts_beside_this_ones_opencodex` 得 40 + 100；单读一个范围（手机、切范围）走同一套合并（`12f62aa`） | **跨设备去重。** A 上 OpenCodex 40 Token，B 上直连 ChatGPT 100 Token。独立 App 首页和手机切分组后都应合计 140；同一台设备内的代理流量仍只计一次。[统计逻辑](crates/core/src/dashboard.rs) |
| LIVE-1 | 已验证 · Codex 复测 `3fcc5cb`：WebKit 钉住面板的实时采样新增 9,000 Token，输出速率为每秒 10/60 Token。 修复提交 `159daba` · Claude 自测：Playwright「a request a gateway counts adds to the live line…」新增 9,000，速率 10/60；改动前为 0 | **网关实时增量。** Magpie 与 ChatGPT 同时记录一笔请求，去重后新增 9,000 Token、其中输出 10。钉住后这一采样应新增 9,000，输出速率按实际输出计算。[实时采样](app/ui/src/lib/live.ts) |
| SHARE-1 | 已验证 · Codex 复测 `3fcc5cb`：WebKit 切回今天后实际下载的 PNG 与重新打开分享页所导出的图哈希一致，两者均与刷新前的图不同。 修复提交 `c8e3c58` `ca76b84` `8bd9e4f` · Claude 自测：切回今天后的图与重新打开分享页的图 SHA-256 相同。另修：新范围的数字分来源陆续到达时，分享页曾冻结第一份（少 Magpie），现在继续接收，有来源待同步时导出先等（`8bd9e4f`） | **同范围的旧图。** 分享页从「今天」切到「近 7 天」，今天数据从 61.96M 增到 123.91M，再切回今天并保存。下载的 PNG 应与重新打开分享页生成的新图一致。[分享页](app/ui/src/components/SharePage.svelte) |
| SHARE-2 | 已验证 · Codex 复测 `3fcc5cb`：WebKit 分享图显示 50.0%，额外的 9,900 个输出 Token 没有改变输入缓存命中率。 修复提交 `6b99f2a` · Claude 自测：Playwright「…cache share…」得 50.0%；卡片带 `cacheInputs`，分享图按可缓存输入加权 | **分享图缓存命中率。** 两个工具各有 100 个可缓存输入 Token，一个命中 100、另一个命中 0。给第一个工具增加 9,900 个输出 Token，分享图仍应显示 50.0%。[分享图统计](app/ui/src/lib/sharePoster.ts) |
| SYNC-2 | 已验证 · Codex 复测 `3fcc5cb`：引擎读取午夜后重写的旧版记录，今天仍为 40 Token。 修复提交 `ea73009` · Claude 自测：单测里记录结束于 23:59:58、文件 00:00:02 重写的旧格式“今天”不再计入 | **旧版同步文件跨午夜。** 旧格式「今天」记录截至昨天 23:59:58，同步文件在今天 00:00:02 重写。本机今天 40，远端昨天 100，今天仍应显示 40。[同步](crates/core/src/devices.rs) |
| SYNC-3 | 已验证 · Codex 复测 `3fcc5cb`：系统 UTC、统计时区台北，实际引擎合并 1 台远端设备，合计 140 Token。 修复提交 `ea73009` `4b938de` · Claude 自测：单测 `a_ranges_day_is_in_the_counting_time_zone`，台北午夜按台北日期 2026-11-20 比对并合并（UTC 下是 11-19） | **自定义统计时区。** 系统 UTC，统计时区 Asia/Taipei，两台电脑取同一个台北午夜。远端 100 和本机 40 应合计 140。[同步](crates/core/src/devices.rs) |
| SESSION-1 | 已验证 · Codex 复测 `3fcc5cb`：独立版引擎刷新时，补回旧段前是 1 轮 1 请求，补回后是 2 轮 2 请求。 修复提交 `c0d86ea` · Claude 自测：单测 `a_segment_put_back_is_read_again…` 补回旧段后 2 轮 2 请求（改动前 1 轮 2 请求） | **补回旧分段。** 先读第二段的 1 轮、1 请求，再补回较早的第一段，保持最新段不变。刷新后应显示 2 轮、2 请求。[会话缓存](crates/core/src/sessions.rs) |
| SESSION-2 | 已验证 · Codex 复测 `3fcc5cb`：把旧段的修改时间调到最新后，查询仍返回 waiting、2 轮 2 请求。 修复提交 `c0d86ea` · Claude 自测：同一单测里旧段修改时间改到最新后状态仍为 waiting（改动前 running）；分段按文件名排序 | **旧分段的修改时间。** 最新编号段已经完成，恢复早期段使其文件修改时间更新。状态仍应取最新编号段，显示等待。[分段排序](crates/core/src/sessions.rs) |
| SESSION-3 | 已验证 · Codex 复测 `3fcc5cb`：第二轮已有约 2.25 MiB 工具输出、尚无用量事件时，查询返回 running、2 轮，合计仍为 1 请求。 修复提交 `c0d86ea` · Claude 自测：引擎对照用例 2.3 MB 输出、尚无新 token_count 时列出两轮，最新一轮 0 请求运行中 | **首个用量事件尚未来到。** 第一轮已完成，第二轮开始并产生约 2.25 MiB 工具输出，尚无新的 token_count。此时应能看到两轮，最新一轮运行中。[轮次读取](crates/core/src/sessions.rs) |
| PHONE-1 | 已验证 · Codex 复测 `3fcc5cb`：启动 SwiftBar 模式副本，实测手机 HTTP 接口。首次读取和请求其他范围或分组时都只提供今天、模型分组，首张卡均是 Claude Opus 5.5。使用本机回环核验。 修复提交 `c6117ec` · 按代码路径核对：SwiftBar 版手机只保留电脑当前的分组（与卡片一致）和电脑当前的范围；SwiftBar 手机页未实机测 | **SwiftBar 手机分组。** 电脑选「模型」时，核对手机上所选标签和卡片。显示的分组与数据应一致，每个可选按钮都应能兑现对应操作。[手机接口](app/src-tauri/src/phone.rs) |
| SHARE-3 | 已验证 · Codex 复测 `3fcc5cb`：WebKit 延迟编码并来回切样式，退出后本次创建且尚未回收的 PNG Blob URL 为 0。 修复提交 `c8e3c58` `ca76b84` · Claude 自测：Playwright 慢编码下「报刊 → 卡片 → 报刊」，渲染结束后退出，Blob URL 计数回到原值；`ca76b84` 修了 `c8e3c58` 里当前图被代理后误判为未使用、提前释放的问题 | **分享图内存。** 编码完成前依次点「卡片 → 报刊 → 卡片」，三次渲染结束后退出。新建的 PNG Blob URL 应全部释放；原复现残留 1,434,687 字节。[图片缓存](app/ui/src/components/SharePage.svelte) |
| KEY-1 | 已验证 · Codex 复测 `3fcc5cb`：WebKit 模拟宿主拒绝已保存的快捷键，再模拟释放；重开面板触发重新注册，恢复显示 ⌃⌥S。系统级抢占未做双应用实机测试。 修复提交 `c49f78b` · 按代码路径核对：每次面板打开（panel-opened）重试注册，文案改为“退出占用它的程序后再打开一次面板”；未在真实冲突下实测 | **快捷键冲突恢复。** 先让其他程序占用已保存的组合键，再释放并按设置提示重新打开面板。快捷键应成功注册，或由文案明确给出能生效的恢复步骤。[面板打开](app/ui/src/App.svelte) · [快捷键](app/ui/src/lib/shortcut.svelte.ts) |

## 主动审查发现

| 编号 | 状态与提交 | 问题与验收条件 |
| --- | --- | --- |
| SHARE-4 | 已验证 · Codex 复测 `a7492b6`：同一分享页的数据源从待同步变为暂不可用后，保存和复制恢复；重新进入得到相同结果。 修复提交 `b597e1a` · Claude 自测：Playwright「…not waited for… (SHARE-4)」切到近 7 天时 Magpie 先待同步、再报暂不可用，当前页“复制图片”随即可用，不用重进；撤掉修复时该用例失败。测试数据加了 `?pending-fails` 复现这条路径 · P2 · Codex 在 `4e54fc6` 用 WebKit 复现 | 一个来源待同步、其余有数据时打开分享页。该来源随后报告「暂不可用」，在当前页保存仍禁用，退出重进立即可存。预期是读取结束后即可导出已有来源，仍有来源在读时继续等待。[分享导出条件](app/ui/src/components/SharePage.svelte) |
| MENU-1 | 已验证 · Codex 复测 `a7492b6`：Tab 和 Shift+Tab 离开后菜单关闭。反向 Tab 后原范围保留，等待中的快照立即重新读取。 修复提交 `a7492b6` · Claude 自测：Playwright「the range menu closes when Tab or Shift+Tab leaves it…」Enter 展开、方向键点亮“昨天”后按 Tab 或 Shift+Tab：aria-expanded 为 false、仍是“今天”、menuOpen 为 false；撤掉修复时失败 · P2 · Codex 在 `4e54fc6` 用 WebKit 复现 | 聚焦主页统计范围，按 Enter 展开，再按 Tab 离开。焦点已在菜单外，按钮仍为展开，刷新仍被 menuOpen 延后。Tab 和 Shift+Tab 离开时应收起、保留原选项并恢复刷新，方向键仍能选择。[下拉菜单](app/ui/src/components/Dropdown.svelte) |
| MENU-2 | 已验证 · Codex 复测 `a7492b6`：先选近 7 天再展开列表，用 WebKit 重放宿主关闭及打开。旧版重开仍展开且阻塞刷新，修复后保留近 7 天、列表隐藏、刷新恢复。窗口失焦事件亦通过。 修复提交 `a7492b6` · Claude 自测：同一用例里菜单展开时派发窗口 blur（点桌面隐藏面板时面板失去焦点）后收起、范围不变、menuOpen 为 false。宿主关闭路径按代码核对：`closed()` 会让当前焦点失焦，菜单随之收起；未在真机上点星星实测 · P2 · 用户在 macOS 实机报告 | **点击桌面后重开。** 在未钉住的主页展开统计范围，不选其他值，点桌面使面板隐藏，再点星星打开。实机反馈为列表仍展开。应保留原范围、收起下拉菜单并恢复刷新。请 Claude 同时核对宿主关闭再打开的路径，与 MENU-1 的键盘退出分别验收。[面板生命周期](app/ui/src/App.svelte) · [宿主失焦](app/src-tauri/src/panel.rs) |
| DOC-1 | 已撤回 · 用户确认这是与 Claude 商定的改动 | **6.0.2 发布附件命名与迁移。** 保留用户已确定的方案。此项已结案，复核时以用户确认的行为为准。 |
