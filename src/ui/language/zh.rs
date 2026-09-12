//! Simplified Chinese strings, keyed by the English source string.
//! Anything absent here falls back to English at the call site.

pub fn text(source: &str) -> Option<&'static str> {
    Some(match source {
        "Key mappings" => "按键映射",
        "Input timings" => "输入时序",
        "Key Input Timeline" => "按键时间线",
        "Input timing measurement" => "输入时序测量",
        "Measured Input Transitions" => "轴向延迟测量",
        "Suggested delays" => "建议的 SOCD 设置",
        "Hardware scan codes the SOCD filter uses." => "SOCD 过滤器使用的硬件扫描码。",
        "How opposite-direction overlaps resolve." => "设置相反方向按键重叠时的处理方式。",
        "Restore defaults" => "恢复默认值",
        "Restore all defaults" => "恢复所有默认值",
        "Revert" => "撤销修改",
        "Apply" => "应用",
        "Profiles" => "配置档案",
        "Profile" => "档案",
        "Language" => "语言",
        "Rename" => "重命名",
        "Load" => "加载",
        "Close" => "关闭",
        "Save name" => "保存名称",
        "Discard edits and load" => "放弃修改并加载",
        "Profile name · 1–64 characters" => "档案名称 · 1–64 个字符",
        "Profile name" => "档案名称",
        "Load a slot to activate it immediately. Apply saves edits to the active slot." => {
            "加载后立即启用档案。点击“应用”将修改保存到当前档案。"
        }
        "The saved slot will become active immediately." => "已保存的档案将立即启用。",
        "Loading and activating profile…" => "正在加载并启用档案…",
        "Saving profile name…" => "正在保存档案名称…",
        "Profile loaded and activated." => "档案已加载并启用。",
        "Profile renamed." => "档案名称已更改。",
        "Active" => "当前",
        "Updating…" => "更新中…",
        "ON" => "开启",
        "OFF" => "关闭",
        "Click a keycap to rebind; click again to cancel." => "点击键帽重新绑定；再次点击可取消。",
        "Modifiers like Shift, Ctrl, and Alt are not captured." => {
            "不会捕获 Shift、Ctrl 和 Alt 等修饰键。"
        }
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
        "Start timeline" => "开始时间线",
        "Stop timeline" => "停止时间线",
        "Starting…" => "启动中…",
        "Stopping…" => "停止中…",
        "No input yet" => "尚无输入",
        "Filter output" => "过滤器输出",
        "Physical input" => "物理输入",
        "Last 1 second · mapped keys only · memory cleared when stopped" => {
            "最近 1 秒 · 仅映射按键 · 停止后清空内存"
        }
        "Now" => "现在",
        "Start measurement" => "开始测量",
        "Stop measurement" => "停止测量",
        "Reset session" => "重置会话",
        "Records your mapped key-pair timing for this session." => {
            "记录本次会话中映射按键对的输入时序。"
        }
        "No measurement results yet." => "尚无测量结果。",
        "Physical key edges" => "物理按键变化",
        "Valid paired samples" => "有效配对样本",
        "Indistinguishable share" => "近同时输入比例",
        "Live counts from this session, values freeze when measurement stops." => {
            "显示本次会话的实时数据；停止测量后保留结果。"
        }
        "INPUT PATTERN" => "输入模式",
        "SAMPLES" => "样本数",
        "MEDIAN" => "中位数",
        "MIN" => "最小值",
        "MAX" => "最大值",
        "Neutral transition" => "中立转换",
        "Physical overlap" => "物理重叠",
        "Indistinguishable" => "近同时输入",
        "Based on P10-P50 input timings, excluding indistinguishable inputs." => {
            "两个轴的 P10–P50 范围，不含近同时样本。"
        }
        "Apply suggestions" => "将建议写入设置",
        "SOCD Transition Delay" => "SOCD 转换延迟",
        "Preserved Overlap Duration" => "保留重叠时长",
        "Unsaved draft changes" => "有未保存的修改",
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
        "On each opposing-key overlap, drop the previous direction and send the new one with no added delay." => {
            "每次相反方向按键重叠时，立即松开旧方向并输出新方向，不增加延迟。"
        }
        "On each opposing-key overlap, release the previous direction immediately and press the new one after the configured delay." => {
            "相反方向按键重叠时，立即松开旧方向，在设定的延迟后按下新方向。"
        }
        "On each opposing-key overlap, randomly select press delay or release delay using the configured ratio." => {
            "相反方向按键重叠时，根据设定比例随机选择按下延迟或松开延迟。"
        }
        "On each opposing-key overlap, press the new direction immediately and release the previous one after the configured delay." => {
            "相反方向按键重叠时，立即按下新方向，在设定的延迟后松开旧方向。"
        }
        "Click a keycap to rebind" => "点击键帽重新绑定",
        "Press a key to assign it" => "按下要分配的按键",
        _ => return None,
    })
}
