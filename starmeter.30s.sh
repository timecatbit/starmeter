#!/bin/bash -p
# <xbar.title>Starmeter</xbar.title>
# <xbar.version>6.0.16</xbar.version>
# <xbar.author>timecatbit</xbar.author>
# <xbar.author.github>timecatbit/starmeter</xbar.author.github>
# <xbar.desc>可选 AI 客户端用量、账户资源与 Komari 监控，使用原生 macOS 面板。</xbar.desc>
# 星星打开原生数据面板；右键保留 SwiftBar 原菜单。
# 直接执行脚本，登录 Shell 的启动提示会被 SwiftBar 当成菜单标题。
# <swiftbar.runInBash>false</swiftbar.runInBash>
# <swiftbar.hideRunInTerminal>true</swiftbar.hideRunInTerminal>
# <swiftbar.hideAbout>true</swiftbar.hideAbout>
# <swiftbar.hideDisablePlugin>true</swiftbar.hideDisablePlugin>
# <swiftbar.hideLastUpdated>true</swiftbar.hideLastUpdated>
# SwiftBar 按 schedule 每分钟运行，忽略文件名里的 30s（保留它只为兼容 xbar 和已安装的路径）：
# 按文件名间隔刷新的插件，锁屏或睡眠期间错过两次刷新，SwiftBar 就在星星旁画 ⚠️；按 schedule 运行的不做这项检查。
# <swiftbar.schedule>* * * * *</swiftbar.schedule>
#
# 可调环境变量（取消注释即可覆盖）：
#   OCX_BASE_URL     管理地址，默认 http://127.0.0.1:10100
#   KOMARI_BASE_URL   Komari 面板地址；在本机 config.local.sh 中配置
#   OCX_ENGINE       指定另一份 starmeter-engine

# 未指定地址时由引擎读取 OpenCodex 当前运行端口。
export OCX_BASE_URL
# 记录插件自身的绝对路径：SwiftBar 的 bash= 参数要绝对路径才能可靠回调本插件。
case "$0" in
    /*) OCX_PLUGIN_PATH="$0" ;;
    *)  OCX_PLUGIN_PATH="$(cd "$(dirname "$0")" 2>/dev/null && pwd)/$(basename "$0")" ;;
esac
export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.local/bin:/usr/bin:/bin:$PATH"
export OCX_PLUGIN_PATH
OCX_USER_CONFIG="${OCX_STATE_DIR:-$HOME/Library/Application Support/SwiftBarToken}/config.local.sh"
# 配置保留在当前 Shell 生效；配置打印的普通文字不属于 SwiftBar 菜单。
if [ -f "$OCX_USER_CONFIG" ]; then source "$OCX_USER_CONFIG" >/dev/null; fi

# The engine is a compiled program installed in the hidden folder; a source checkout runs its own build.
# OCX_ENGINE in config.local.sh can point at another build.
if [ -z "$OCX_ENGINE" ]; then
    OCX_ENGINE="$(dirname "$OCX_PLUGIN_PATH")/.opencodex-panel/bin/starmeter-engine"
    [ -x "$OCX_ENGINE" ] || OCX_ENGINE="$(dirname "$OCX_PLUGIN_PATH")/build/starmeter-engine"
fi
if [ ! -x "$OCX_ENGINE" ]; then
    printf '%s\n' 'Starmeter' '---' '插件运行文件缺失，请重新运行安装脚本。'
    exit 0
fi
exec "$OCX_ENGINE" "$@"
