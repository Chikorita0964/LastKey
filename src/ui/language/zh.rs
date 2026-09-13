//! Simplified Chinese strings, keyed by the English source string.
//! Anything absent here falls back to English at the call site.

pub fn text(source: &str) -> Option<&'static str> {
    Some(match source {
        "Play preview" => "播放预览",
        "Pause preview" => "暂停预览",
        "Unsaved Draft Changes" => "草稿更改尚未保存",
        "Click Apply to commit draft edits." => "点击应用以保存草稿更改。",
        "Updates live while measuring" => "测量期间实时更新",
        "Based on neutral transitions" => "基于中立过渡",
        "Based on physical overlaps" => "基于物理按键重叠",
        "Shows how long each key is held and where it overlaps its opposite, live." => {
            "实时显示每个按键的按住时长及其与反方向按键的重叠。"
        }
        "Keyboard input capture is active" => "键盘输入捕获已开启",
        "Each overlap randomly picks one of the two delays below." => {
            "每次重叠随机选择以下两种延迟之一。"
        }
        "Unclear input order (<1 ms), excluded from timing ranges." => {
            "输入顺序不明确（小于 1 ms），不计入时序范围。"
        }
        "Preview" => "预览",
        "Previous example" => "上一个示例",
        "Next example" => "下一个示例",
        "Game receives A + D" => "游戏收到 A + D",
        "Game receives no direction" => "游戏未收到方向输入",
        "Game receives A" => "游戏收到 A",
        "Game receives D" => "游戏收到 D",
        "Key mappings" => "按键映射",
        "Input timings" => "输入时序",
        "Key Input Timeline" => "按键时间线",
        "Input timing measurement" => "输入时序测量",
        "Measured Input Transitions" => "轴向延迟测量",
        "Suggested delays" => "建议的 SOCD 设置",
        "Hardware scan codes the SOCD filter uses" => "SOCD 过滤器使用的硬件扫描码",
        "How opposite-direction overlaps resolve." => "设置相反方向按键重叠时的处理方式。",
        "Restore mapping defaults" => "恢复映射默认值",
        "Restore timing defaults" => "恢复时序默认值",
        "Restore all defaults" => "恢复所有默认值",
        "Revert" => "撤销修改",
        "Apply" => "应用",
        "Profile Slots" => "档案槽位",
        "Profiles" => "配置档案",
        "Profile" => "档案",
        "Language" => "语言",
        "Rename" => "重命名",
        "Load" => "加载",
        "Cancel" => "取消",
        "Close" => "关闭",
        "Profile name" => "档案名称",
        "Changes are saved when you click Apply." => "点击“应用”后保存更改。",
        "Load a slot to activate it immediately. Apply saves edits to the active slot." => {
            "加载后立即启用档案。点击“应用”将修改保存到当前档案。"
        }
        "Load this slot?" => "加载此档案？",
        "Unapplied draft changes will be discarded." => "未应用的修改将被丢弃。",
        "Loading and activating profile…" => "正在加载并启用档案…",
        "Saving profile name…" => "正在保存档案名称…",
        "Profile loaded and activated." => "档案已加载并启用。",
        "Profile renamed." => "档案名称已更改。",
        "Active" => "当前",
        "Updating…" => "更新中…",
        "Enable the SOCD filter" => "启用 SOCD 过滤器",
        "Disable the SOCD filter" => "禁用 SOCD 过滤器",
        "All keys uniquely assigned." => "所有按键均无重复。",
        "Duplicate key bindings detected." => "检测到重复按键。",
        "UP" => "上",
        "DOWN" => "下",
        "LEFT" => "左",
        "RIGHT" => "右",
        "Immediate" => "即时",
        "Press Delay" => "按下延迟",
        "Random Mix" => "随机混合",
        "Release Delay" => "松开延迟",
        "How it works" => "工作原理",
        "Delay Mix Ratio" => "延迟混合比例",
        "Press delay" => "按下延迟",
        "Release delay" => "松开延迟",
        "New Key Press Delay" => "新按键按下延迟",
        "Previous Key Release Delay" => "前一按键松开延迟",
        "Starting…" => "启动中…",
        "Stopping…" => "停止中…",
        "Start measurement" => "开始测量",
        "Stop measurement" => "停止测量",
        "Reset session" => "重置会话",
        "Records your mapped key-pair timing for this session." => {
            "记录本次会话中映射按键对的输入时序。"
        }
        "No measurement results yet." => "尚无测量结果。",
        "Physical key edges" => "物理按键变化",
        "Valid paired samples" => "有效配对样本",
        "Physical overlap share" => "物理重叠比例",
        "Indistinguishable share" => "近同时输入比例",
        "INPUT PATTERN" => "输入模式",
        "SAMPLES" => "样本数",
        "MIN" => "最小值",
        "MAX" => "最大值",
        "Neutral transition" => "中立转换",
        "Physical overlap" => "物理重叠",
        "Indistinguishable" => "近同时输入",
        "Based on P10-P50 input timings, excluding indistinguishable inputs." => {
            "两个轴的 P10–P50 范围，不含近同时样本。"
        }
        "Apply suggestions" => "将建议写入设置",
        "Synchronized" => "已同步",
        "Settings are synchronized with the runtime." => "设置已与运行时同步。",
        "Settings applied." => "设置已应用。",
        "Connecting to the LastKey runtime..." => "正在连接 LastKey 运行时…",
        "Connected to the LastKey runtime." => "已连接 LastKey 运行时。",
        "The LastKey runtime is disconnected." => "LastKey 运行时已断开。",
        "The settings UI is waiting for LastKey.exe." => "设置窗口正在等待 LastKey.exe。",
        "Request snapshot" => "重新同步",
        "Applying settings..." => "正在应用设置…",
        "Mapping changed. Select Apply when ready." => "按键映射已更改。准备好后点击“应用”。",
        "Recommendations written to the draft. Select Apply when ready." => {
            "建议已写入草稿。准备好后点击“应用”。"
        }
        "Detect an opposite-direction overlap." => "检测到相反方向按键重叠。",
        "Release the previous key output immediately." => "立即松开旧方向的输出。",
        "Send the new key immediately. 0 ms added delay" => "立即输出新方向。增加的延迟为 0 ms",
        "Send the new key immediately." => "立即输出新方向。",
        "Wait a random time within {range}. This gap sends no input" => {
            "在 {range} 内随机等待。此间隔不输出任何按键"
        }
        "Send the new key once the wait ends." => "等待结束后输出新方向。",
        "Wait a random time within {range}. The overlap stays live" => {
            "在 {range} 内随机等待。期间重叠保持生效"
        }
        "Release the previous key output once the wait ends." => "等待结束后松开旧方向的输出。",
        "Click keycap to rebind" => "点击键帽重新绑定",
        "Press a new key on your keyboard..." => "请按下新的按键…",
        "ESC Cancel" => "ESC 取消",
        _ => return None,
    })
}
