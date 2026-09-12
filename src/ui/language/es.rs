//! Spanish strings, keyed by the English source string.
//! Anything absent here falls back to English at the call site.

pub fn text(source: &str) -> Option<&'static str> {
    Some(match source {
        "Key mappings" => "Asignación de teclas",
        "Input timings" => "Tiempos de entrada",
        "Key Input Timeline" => "Línea de tiempo de teclas",
        "Input timing measurement" => "Medición de tiempos",
        "Measured Input Transitions" => "Latencias medidas por eje",
        "Suggested delays" => "Ajustes SOCD sugeridos",
        "Hardware scan codes the SOCD filter uses." => "Códigos físicos usados por el filtro SOCD.",
        "How opposite-direction overlaps resolve." => "Cómo se resuelven las pulsaciones opuestas.",
        "Restore defaults" => "Restaurar valores",
        "Restore all defaults" => "Restaurar todo",
        "Revert" => "Revertir",
        "Apply" => "Aplicar",
        "Profiles" => "Perfiles",
        "Profile" => "Perfil",
        "Language" => "Idioma",
        "Rename" => "Renombrar",
        "Load" => "Cargar",
        "Close" => "Cerrar",
        "Save name" => "Guardar nombre",
        "Discard edits and load" => "Descartar y cargar",
        "Profile name · 1–64 characters" => "Nombre del perfil · 1–64 caracteres",
        "Profile name" => "Nombre del perfil",
        "Load a slot to activate it immediately. Apply saves edits to the active slot." => {
            "Cargar activa el perfil de inmediato. Aplicar guarda los cambios en el perfil activo."
        }
        "The saved slot will become active immediately." => {
            "El perfil guardado se activará de inmediato."
        }
        "Loading and activating profile…" => "Cargando y activando el perfil…",
        "Saving profile name…" => "Guardando el nombre…",
        "Profile loaded and activated." => "Perfil cargado y activado.",
        "Profile renamed." => "Perfil renombrado.",
        "Active" => "Activo",
        "Updating…" => "Actualizando…",
        "ON" => "ON",
        "OFF" => "OFF",
        "Click a keycap to rebind; click again to cancel." => {
            "Pulsa una tecla para reasignarla; vuelve a pulsar para cancelar."
        }
        "Modifiers like Shift, Ctrl, and Alt are not captured." => {
            "No se capturan modificadores como Mayús, Ctrl ni Alt."
        }
        "All keys uniquely assigned." => "Todas las teclas son únicas.",
        "Duplicate key bindings detected." => "Hay teclas duplicadas.",
        "UP" => "ARRIBA",
        "DOWN" => "ABAJO",
        "LEFT" => "IZQ.",
        "RIGHT" => "DER.",
        "Immediate" => "Inmediato",
        "Press Delay" => "Retardo al pulsar",
        "Random Mix" => "Mezcla aleatoria",
        "Release Delay" => "Retardo al soltar",
        "How it works" => "Cómo funciona",
        "Delay Mix Ratio" => "Proporción de retardos",
        "Press delay" => "Retardo al pulsar",
        "Release delay" => "Retardo al soltar",
        "New Key Press Delay" => "Retardo de la nueva tecla",
        "Previous Key Release Delay" => "Retardo al soltar la anterior",
        "Start timeline" => "Iniciar línea de tiempo",
        "Stop timeline" => "Detener línea de tiempo",
        "Starting…" => "Iniciando…",
        "Stopping…" => "Deteniendo…",
        "No input yet" => "Sin entradas",
        "Filter output" => "Salida del filtro",
        "Physical input" => "Entrada física",
        "Last 1 second · mapped keys only · memory cleared when stopped" => {
            "Último segundo · solo teclas asignadas · se borra al detener"
        }
        "Now" => "Ahora",
        "Start measurement" => "Iniciar medición",
        "Stop measurement" => "Detener medición",
        "Reset session" => "Reiniciar sesión",
        "Records your mapped key-pair timing for this session." => {
            "Registra los tiempos de los pares de teclas de esta sesión."
        }
        "No measurement results yet." => "Aún no hay resultados.",
        "Physical key edges" => "Cambios de tecla",
        "Valid paired samples" => "Muestras válidas",
        "Indistinguishable share" => "Proporción casi simultánea",
        "Live counts from this session, values freeze when measurement stops." => {
            "Datos en vivo de la sesión; los valores se fijan al detenerla."
        }
        "INPUT PATTERN" => "PATRÓN",
        "SAMPLES" => "MUESTRAS",
        "MEDIAN" => "MEDIANA",
        "MIN" => "MÍN.",
        "MAX" => "MÁX.",
        "Neutral transition" => "Transición neutra",
        "Physical overlap" => "Solapamiento físico",
        "Indistinguishable" => "Casi simultáneo",
        "Based on P10-P50 input timings, excluding indistinguishable inputs." => {
            "Rangos P10–P50 de ambos ejes, sin muestras casi simultáneas."
        }
        "Apply suggestions" => "Usar recomendaciones",
        "SOCD Transition Delay" => "Retardo de transición SOCD",
        "Preserved Overlap Duration" => "Duración del solapamiento",
        "Unsaved draft changes" => "Cambios sin guardar",
        "Synchronized" => "Sincronizado",
        "Settings are synchronized with the runtime." => "Ajustes sincronizados con el motor.",
        "Settings applied." => "Ajustes aplicados.",
        "Connecting to the LastKey runtime..." => "Conectando con el motor de LastKey…",
        "Connected to the LastKey runtime." => "Conectado al motor de LastKey.",
        "The LastKey runtime is disconnected." => "El motor de LastKey está desconectado.",
        "The settings UI is waiting for LastKey.exe." => "Esperando a LastKey.exe.",
        "Request snapshot" => "Sincronizar",
        "Applying settings..." => "Aplicando ajustes…",
        "Mapping changed. Select Apply when ready." => {
            "Asignación modificada. Pulsa Aplicar cuando estés listo."
        }
        "Recommendations written to the draft. Select Apply when ready." => {
            "Recomendaciones copiadas al borrador. Pulsa Aplicar cuando estés listo."
        }
        "On each opposing-key overlap, drop the previous direction and send the new one with no added delay." => {
            "Al solaparse teclas opuestas, suelta la anterior y envía la nueva sin añadir retardo."
        }
        "On each opposing-key overlap, release the previous direction immediately and press the new one after the configured delay." => {
            "Al solaparse teclas opuestas, suelta la anterior de inmediato y pulsa la nueva tras el retardo configurado."
        }
        "On each opposing-key overlap, randomly select press delay or release delay using the configured ratio." => {
            "En cada solapamiento, elige al azar un retardo al pulsar o al soltar según la proporción configurada."
        }
        "On each opposing-key overlap, press the new direction immediately and release the previous one after the configured delay." => {
            "Al solaparse teclas opuestas, pulsa la nueva de inmediato y suelta la anterior tras el retardo configurado."
        }
        "Click a keycap to rebind" => "Pulsa una tecla para reasignarla",
        "Press a key to assign it" => "Pulsa una tecla para asignarla",
        _ => return None,
    })
}
