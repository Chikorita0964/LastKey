//! English source strings: the canonical inventory every other language
//! file is keyed by. Entries map to themselves, so this file is the one
//! place to read what is translatable and to diff a new language
//! against. A string missing here still renders as written.
pub fn text(source: &str) -> Option<&'static str> {
    Some(match source {
        "Key mappings" => "Key mappings",
        "Input timings" => "Input timings",
        "Key Input Timeline" => "Key Input Timeline",
        "Input timing measurement" => "Input timing measurement",
        "Measured Input Transitions" => "Measured Input Transitions",
        "Suggested delays" => "Suggested delays",
        "Hardware scan codes the SOCD filter uses." => "Hardware scan codes the SOCD filter uses.",
        "How opposite-direction overlaps resolve." => "How opposite-direction overlaps resolve.",
        "Restore defaults" => "Restore defaults",
        "Restore all defaults" => "Restore all defaults",
        "Revert" => "Revert",
        "Apply" => "Apply",
        "Profiles" => "Profiles",
        "Profile" => "Profile",
        "Language" => "Language",
        "Rename" => "Rename",
        "Load" => "Load",
        "Close" => "Close",
        "Save name" => "Save name",
        "Discard edits and load" => "Discard edits and load",
        "Profile name · 1–64 characters" => "Profile name · 1–64 characters",
        "Profile name" => "Profile name",
        "Load a slot to activate it immediately. Apply saves edits to the active slot." => {
            "Load a slot to activate it immediately. Apply saves edits to the active slot."
        }
        "The saved slot will become active immediately." => {
            "The saved slot will become active immediately."
        }
        "Loading and activating profile…" => "Loading and activating profile…",
        "Saving profile name…" => "Saving profile name…",
        "Profile loaded and activated." => "Profile loaded and activated.",
        "Profile renamed." => "Profile renamed.",
        "Active" => "Active",
        "Updating…" => "Updating…",
        "ON" => "ON",
        "OFF" => "OFF",
        "Click a keycap to rebind; click again to cancel." => {
            "Click a keycap to rebind; click again to cancel."
        }
        "Modifiers like Shift, Ctrl, and Alt are not captured." => {
            "Modifiers like Shift, Ctrl, and Alt are not captured."
        }
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
        "Start timeline" => "Start timeline",
        "Stop timeline" => "Stop timeline",
        "Starting…" => "Starting…",
        "Stopping…" => "Stopping…",
        "No input yet" => "No input yet",
        "Filter output" => "Filter output",
        "Physical input" => "Physical input",
        "Last 1 second · mapped keys only · memory cleared when stopped" => {
            "Last 1 second · mapped keys only · memory cleared when stopped"
        }
        "Now" => "Now",
        "Start measurement" => "Start measurement",
        "Stop measurement" => "Stop measurement",
        "Reset session" => "Reset session",
        "Records your mapped key-pair timing for this session." => {
            "Records your mapped key-pair timing for this session."
        }
        "No measurement results yet." => "No measurement results yet.",
        "Physical key edges" => "Physical key edges",
        "Valid paired samples" => "Valid paired samples",
        "Indistinguishable share" => "Indistinguishable share",
        "Live counts from this session, values freeze when measurement stops." => {
            "Live counts from this session, values freeze when measurement stops."
        }
        "INPUT PATTERN" => "INPUT PATTERN",
        "SAMPLES" => "SAMPLES",
        "MEDIAN" => "MEDIAN",
        "MIN" => "MIN",
        "MAX" => "MAX",
        "Neutral transition" => "Neutral transition",
        "Physical overlap" => "Physical overlap",
        "Indistinguishable" => "Indistinguishable",
        "Based on P10-P50 input timings, excluding indistinguishable inputs." => {
            "Based on P10-P50 input timings, excluding indistinguishable inputs."
        }
        "Apply suggestions" => "Apply suggestions",
        "SOCD Transition Delay" => "SOCD Transition Delay",
        "Preserved Overlap Duration" => "Preserved Overlap Duration",
        "Unsaved draft changes" => "Unsaved draft changes",
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
        "On each opposing-key overlap, drop the previous direction and send the new one with no added delay." => {
            "On each opposing-key overlap, drop the previous direction and send the new one with no added delay."
        }
        "On each opposing-key overlap, release the previous direction immediately and press the new one after the configured delay." => {
            "On each opposing-key overlap, release the previous direction immediately and press the new one after the configured delay."
        }
        "On each opposing-key overlap, randomly select press delay or release delay using the configured ratio." => {
            "On each opposing-key overlap, randomly select press delay or release delay using the configured ratio."
        }
        "On each opposing-key overlap, press the new direction immediately and release the previous one after the configured delay." => {
            "On each opposing-key overlap, press the new direction immediately and release the previous one after the configured delay."
        }
        "Click a keycap to rebind" => "Click a keycap to rebind",
        "Press a key to assign it" => "Press a key to assign it",
        _ => return None,
    })
}
