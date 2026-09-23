//! English source strings: the canonical inventory every other language
//! file is keyed by. Entries map to themselves, so this file is the one
//! place to read what is translatable and to diff a new language
//! against. A string missing here still renders as written.
pub fn text(source: &str) -> Option<&'static str> {
    Some(match source {
        "Play preview" => "Play preview",
        "Pause preview" => "Pause preview",
        "Unsaved Draft Changes" => "Unsaved Draft Changes",
        "Click Apply to commit draft edits." => "Click Apply to commit draft edits.",
        "Updates live while measuring" => "Updates live while measuring",
        "Based on neutral transitions" => "Based on neutral transitions",
        "Based on physical overlaps" => "Based on physical overlaps",
        "Shows how long each key is held and where it overlaps its opposite, live." => {
            "Shows how long each key is held and where it overlaps its opposite, live."
        }
        "Keyboard input capture is active" => "Keyboard input capture is active",
        "Each overlap randomly picks one of the two delays below." => {
            "Each overlap randomly picks one of the two delays below."
        }
        "Unclear input order (<1 ms), excluded from timing ranges." => {
            "Unclear input order (<1 ms), excluded from timing ranges."
        }
        "PREVIEW" => "PREVIEW",
        "Previous example" => "Previous example",
        "Next example" => "Next example",
        "Opposite-direction overlap" => "Opposite-direction overlap",
        "Neutral gap (neither key active)" => "Neutral gap (neither key active)",
        "A output active" => "A output active",
        "D output active" => "D output active",
        "Key mappings" => "Key mappings",
        "Input timings" => "Input timings",
        "Key Input Timeline" => "Key Input Timeline",
        "Input timing measurement" => "Input timing measurement",
        "Measured Input Transitions" => "Measured Input Transitions",
        "Suggested delays" => "Suggested delays",
        "Hardware scan codes the SOCD filter uses" => "Hardware scan codes the SOCD filter uses",
        "How opposite-direction overlaps resolve." => "How opposite-direction overlaps resolve.",
        "Restore mapping defaults" => "Restore mapping defaults",
        "Restore timing defaults" => "Restore timing defaults",
        "Restore all defaults" => "Restore all defaults",
        "Revert" => "Revert",
        "Apply" => "Apply",
        "Profile Slots" => "Profile Slots",
        "Profiles" => "Profiles",
        "Profile" => "Profile",
        "Language" => "Language",
        "Rename" => "Rename",
        "Load" => "Load",
        "Cancel" => "Cancel",
        "Close" => "Close",
        "Profile name" => "Profile name",
        "Changes are saved when you click Apply." => "Changes are saved when you click Apply.",
        "Load a slot to activate it immediately. Apply saves edits to the active slot." => {
            "Load a slot to activate it immediately. Apply saves edits to the active slot."
        }
        "Load this slot?" => "Load this slot?",
        "Unapplied draft changes will be discarded." => {
            "Unapplied draft changes will be discarded."
        }
        "Loading and activating profile…" => "Loading and activating profile…",
        "Saving profile name…" => "Saving profile name…",
        "Profile loaded and activated." => "Profile loaded and activated.",
        "Profile renamed." => "Profile renamed.",
        "Active" => "Active",
        "Updating…" => "Updating…",
        "Enable the SOCD filter" => "Enable the SOCD filter",
        "Disable the SOCD filter" => "Disable the SOCD filter",
        "All keys uniquely assigned." => "All keys uniquely assigned.",
        "Duplicate key bindings detected." => "Duplicate key bindings detected.",
        "UP" => "UP",
        "DOWN" => "DOWN",
        "LEFT" => "LEFT",
        "RIGHT" => "RIGHT",
        "Immediate" => "Immediate",
        "Press Delay" => "Press Delay",
        "Random Mix" => "Random Mix",
        "Release Delay" => "Release Delay",
        "How it works" => "How it works",
        "Delay Mix Ratio" => "Delay Mix Ratio",
        "Press delay" => "Press delay",
        "Release delay" => "Release delay",
        "New Key Press Delay" => "New Key Press Delay",
        "Previous Key Release Delay" => "Previous Key Release Delay",
        "Starting…" => "Starting…",
        "Stopping…" => "Stopping…",
        "Start measurement" => "Start measurement",
        "Stop measurement" => "Stop measurement",
        "Reset session" => "Reset session",
        "Records your mapped key-pair timing for this session." => {
            "Records your mapped key-pair timing for this session."
        }
        "No measurement results yet." => "No measurement results yet.",
        "Physical key edges" => "Physical key edges",
        "Valid paired samples" => "Valid paired samples",
        "Physical overlap share" => "Physical overlap share",
        "Indistinguishable share" => "Indistinguishable share",
        "INPUT PATTERN" => "INPUT PATTERN",
        "SAMPLES" => "SAMPLES",
        "MIN" => "MIN",
        "MAX" => "MAX",
        "Neutral transition" => "Neutral transition",
        "Physical overlap" => "Physical overlap",
        "Indistinguishable" => "Indistinguishable",
        "Based on P10-P50 input timings, excluding indistinguishable inputs." => {
            "Based on P10-P50 input timings, excluding indistinguishable inputs."
        }
        "Apply suggestions" => "Apply suggestions",
        "Synchronized" => "Synchronized",
        "Settings are synchronized with the runtime." => {
            "Settings are synchronized with the runtime."
        }
        "Settings applied." => "Settings applied.",
        "Connecting to the LastKey runtime..." => "Connecting to the LastKey runtime...",
        "Connected to the LastKey runtime." => "Connected to the LastKey runtime.",
        "The LastKey runtime is disconnected." => "The LastKey runtime is disconnected.",
        "The settings UI is waiting for LastKey.exe." => {
            "The settings UI is waiting for LastKey.exe."
        }
        "Request snapshot" => "Request snapshot",
        "Applying settings..." => "Applying settings...",
        "Mapping changed. Select Apply when ready." => "Mapping changed. Select Apply when ready.",
        "Recommendations written to the draft. Select Apply when ready." => {
            "Recommendations written to the draft. Select Apply when ready."
        }
        "Detect an opposite-direction overlap." => "Detect an opposite-direction overlap.",
        "Release the previous key output immediately." => {
            "Release the previous key output immediately."
        }
        "Send the new key immediately. 0 ms added delay" => {
            "Send the new key immediately. 0 ms added delay"
        }
        "Send the new key immediately." => "Send the new key immediately.",
        "Wait a random time within {range}. This gap sends no input" => {
            "Wait a random time within {range}. This gap sends no input"
        }
        "Send the new key once the wait ends." => "Send the new key once the wait ends.",
        "Wait a random time within {range}. The overlap stays live" => {
            "Wait a random time within {range}. The overlap stays live"
        }
        "Release the previous key output once the wait ends." => {
            "Release the previous key output once the wait ends."
        }
        "Click keycap to rebind" => "Click keycap to rebind",
        "Press a new key on your keyboard..." => "Press a new key on your keyboard...",
        "ESC Cancel" => "ESC Cancel",
        _ => return None,
    })
}
