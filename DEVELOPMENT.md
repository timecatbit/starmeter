# Starmeter 开发交接

Claude 和 Codex 在这里交接 6.x 的开发与审查。产品约定、构建和测试命令见 [AGENTS.md](AGENTS.md)。

## 接手与回报

- 每次开始或继续本项目、准备领修、准备复审时，重读本文件和最新提交。使用独立 Git 工作树时，从 `git worktree list --porcelain` 找到 `branch refs/heads/main` 对应的 `worktree`，在那里的 `DEVELOPMENT.md` 读写交接；代码可在自己的分支修改。
- Codex 把审查意见直接登记在下表，给出复现条件和可观察的预期。Claude 领修时在同一行填「处理中 · Claude」；已有相关提交或未提交改动时先核对进展。两边都可补充新发现。
- 修复者填「待复审」、提交号和验证结果。Codex 用表内场景独立复测，记下「已验证」、所测提交和实际结果；仍能复现就填「重新打开」及实测值。更新前重读该行，只改自己负责的条目。
- 用户说「继续审查」时，Codex 先看待复审项，再登记新问题；用户让 Claude「继续开发」时，Claude 从尚未有人处理的高优先级条目接手。日常交接只需写编号和结果，免去转贴聊天记录。

## 审查队列

以下案例来自对 `d34af15` 的 macOS 独立版、引擎和 WebKit 审查，数值均为合成测试数据。「待复审」表示找到了对应提交，需在该提交或其后代上验证。除 `4b938de`、`8bd9e4f` 外的修复都已在 6.0.1（`399885a`）里发布。

| 编号 | 状态与提交 | 问题与验收条件 |
| --- | --- | --- |
| SYNC-1 | 待复审 · `db0f7f3` `12f62aa` · Claude 自测：单测 `another_computers_chatgpt_counts_beside_this_ones_opencodex` 得 40 + 100；单读一个范围（手机、切范围）走同一套合并（`12f62aa`） | **跨设备去重。** A 上 OpenCodex 40 Token，B 上直连 ChatGPT 100 Token。独立 App 首页和手机切分组后都应合计 140；同一台设备内的代理流量仍只计一次。[统计逻辑](crates/core/src/dashboard.rs) |
| LIVE-1 | 待复审 · `159daba` · Claude 自测：Playwright「a request a gateway counts adds to the live line…」新增 9,000，速率 10/60；改动前为 0 | **网关实时增量。** Magpie 与 ChatGPT 同时记录一笔请求，去重后新增 9,000 Token、其中输出 10。钉住后这一采样应新增 9,000，输出速率按实际输出计算。[实时采样](app/ui/src/lib/live.ts) |
| SHARE-1 | 待复审 · `c8e3c58` `ca76b84` `8bd9e4f` · Claude 自测：切回今天后的图与重新打开分享页的图 SHA-256 相同。另修：新范围的数字分来源陆续到达时，分享页曾冻结第一份（少 Magpie），现在继续接收，有来源待同步时导出先等（`8bd9e4f`） | **同范围的旧图。** 分享页从「今天」切到「近 7 天」，今天数据从 61.96M 增到 123.91M，再切回今天并保存。下载的 PNG 应与重新打开分享页生成的新图一致。[分享页](app/ui/src/components/SharePage.svelte) |
| SHARE-2 | 待复审 · `6b99f2a` · Claude 自测：Playwright「…cache share…」得 50.0%；卡片带 `cacheInputs`，分享图按可缓存输入加权 | **分享图缓存命中率。** 两个工具各有 100 个可缓存输入 Token，一个命中 100、另一个命中 0。给第一个工具增加 9,900 个输出 Token，分享图仍应显示 50.0%。[分享图统计](app/ui/src/lib/sharePoster.ts) |
| SYNC-2 | 待复审 · `ea73009` · Claude 自测：单测里记录结束于 23:59:58、文件 00:00:02 重写的旧格式“今天”不再计入 | **旧版同步文件跨午夜。** 旧格式「今天」记录截至昨天 23:59:58，同步文件在今天 00:00:02 重写。本机今天 40，远端昨天 100，今天仍应显示 40。[同步](crates/core/src/devices.rs) |
| SYNC-3 | 待复审 · `ea73009` `4b938de` · Claude 自测：单测 `a_ranges_day_is_in_the_counting_time_zone`，台北午夜按台北日期 2026-11-20 比对并合并（UTC 下是 11-19） | **自定义统计时区。** 系统 UTC，统计时区 Asia/Taipei，两台电脑取同一个台北午夜。远端 100 和本机 40 应合计 140。[同步](crates/core/src/devices.rs) |
| SESSION-1 | 待复审 · `c0d86ea` · Claude 自测：单测 `a_segment_put_back_is_read_again…` 补回旧段后 2 轮 2 请求（改动前 1 轮 2 请求） | **补回旧分段。** 先读第二段的 1 轮、1 请求，再补回较早的第一段，保持最新段不变。刷新后应显示 2 轮、2 请求。[会话缓存](crates/core/src/sessions.rs) |
| SESSION-2 | 待复审 · `c0d86ea` · Claude 自测：同一单测里旧段修改时间改到最新后状态仍为 waiting（改动前 running）；分段按文件名排序 | **旧分段的修改时间。** 最新编号段已经完成，恢复早期段使其文件修改时间更新。状态仍应取最新编号段，显示等待。[分段排序](crates/core/src/sessions.rs) |
| SESSION-3 | 待复审 · `c0d86ea` · Claude 自测：引擎对照用例 2.3 MB 输出、尚无新 token_count 时列出两轮，最新一轮 0 请求运行中 | **首个用量事件尚未来到。** 第一轮已完成，第二轮开始并产生约 2.25 MiB 工具输出，尚无新的 token_count。此时应能看到两轮，最新一轮运行中。[轮次读取](crates/core/src/sessions.rs) |
| PHONE-1 | 待复审 · `c6117ec` · 按代码路径核对：SwiftBar 版手机只保留电脑当前的分组（与卡片一致）和电脑当前的范围；SwiftBar 手机页未实机测 | **SwiftBar 手机分组。** 电脑选「模型」时，核对手机上所选标签和卡片。显示的分组与数据应一致，每个可选按钮都应能兑现对应操作。[手机接口](app/src-tauri/src/phone.rs) |
| SHARE-3 | 待复审 · `c8e3c58` `ca76b84` · Claude 自测：Playwright 慢编码下「报刊 → 卡片 → 报刊」，渲染结束后退出，Blob URL 计数回到原值；`ca76b84` 修了 `c8e3c58` 里当前图被代理后误判为未使用、提前释放的问题 | **分享图内存。** 编码完成前依次点「卡片 → 报刊 → 卡片」，三次渲染结束后退出。新建的 PNG Blob URL 应全部释放；原复现残留 1,434,687 字节。[图片缓存](app/ui/src/components/SharePage.svelte) |
| KEY-1 | 待复审 · `c49f78b` · 按代码路径核对：每次面板打开（panel-opened）重试注册，文案改为“退出占用它的程序后再打开一次面板”；未在真实冲突下实测 | **快捷键冲突恢复。** 先让其他程序占用已保存的组合键，再释放并按设置提示重新打开面板。快捷键应成功注册，或由文案明确给出能生效的恢复步骤。[面板打开](app/ui/src/App.svelte) · [快捷键](app/ui/src/lib/shortcut.svelte.ts) |
