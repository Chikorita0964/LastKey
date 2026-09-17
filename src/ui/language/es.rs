//! Spanish strings, keyed by the English source string.
//! Anything absent here falls back to English at the call site.

pub fn text(source: &str) -> Option<&'static str> {
    Some(match source {
        "Play preview" => "Reproducir vista previa",
        "Pause preview" => "Pausar vista previa",
        "Unsaved Draft Changes" => "Cambios del borrador sin guardar",
        "Click Apply to commit draft edits." => {
            "Haz clic en Aplicar para guardar los cambios del borrador."
        }
        "Updates live while measuring" => "Se actualiza en directo durante la medición",
        "Based on neutral transitions" => "Basado en transiciones neutras",
        "Based on physical overlaps" => "Basado en solapamientos físicos",
        "Shows how long each key is held and where it overlaps its opposite, live." => {
            "Muestra en directo cuánto se mantiene cada tecla y cuándo se solapa con la opuesta."
        }
        "Keyboard input capture is active" => "La captura del teclado está activa",
        "Each overlap randomly picks one of the two delays below." => {
            "Cada solapamiento elige al azar uno de los dos retardos siguientes."
        }
        "Unclear input order (<1 ms), excluded from timing ranges." => {
            "Orden de entrada incierto (<1 ms), excluido de los intervalos de tiempo."
        }
        "Preview" => "Vista previa",
        "Previous example" => "Ejemplo anterior",
        "Next example" => "Ejemplo siguiente",
        "Game receives A + D" => "El juego recibe A + D",
        "Game receives no direction" => "El juego no recibe ninguna dirección",
        "Game receives A" => "El juego recibe A",
        "Game receives D" => "El juego recibe D",
        "Key mappings" => "Asignación de teclas",
        "Input timings" => "Tiempos de entrada",
        "Key Input Timeline" => "Línea de tiempo de teclas",
        "Input timing measurement" => "Medición de tiempos",
        "Measured Input Transitions" => "Latencias medidas por eje",
        "Suggested delays" => "Ajustes SOCD sugeridos",
        "Hardware scan codes the SOCD filter uses" => "Códigos físicos usados por el filtro SOCD",
        "How opposite-direction overlaps resolve." => "Cómo se resuelven las pulsaciones opuestas.",
        "Restore mapping defaults" => "Restaurar asignación",
        "Restore timing defaults" => "Restaurar tiempos",
        "Restore all defaults" => "Restaurar todo",
        "Revert" => "Revertir",
        "Apply" => "Aplicar",
        "Profile Slots" => "Ranuras de perfil",
        "Profiles" => "Perfiles",
        "Profile" => "Perfil",
        "Language" => "Idioma",
        "Rename" => "Renombrar",
        "Load" => "Cargar",
        "Cancel" => "Cancelar",
        "Close" => "Cerrar",
        "Profile name" => "Nombre del perfil",
        "Changes are saved when you click Apply." => "Los cambios se guardan al pulsar Aplicar.",
        "Load a slot to activate it immediately. Apply saves edits to the active slot." => {
            "Cargar activa el perfil de inmediato. Aplicar guarda los cambios en el perfil activo."
        }
        "Load this slot?" => "¿Cargar este perfil?",
        "Unapplied draft changes will be discarded." => "Los cambios no aplicados se descartarán.",
        "Loading and activating profile…" => "Cargando y activando el perfil…",
        "Saving profile name…" => "Guardando el nombre…",
        "Profile loaded and activated." => "Perfil cargado y activado.",
        "Profile renamed." => "Perfil renombrado.",
        "Active" => "Activo",
        "Updating…" => "Actualizando…",
        "Enable the SOCD filter" => "Activar el filtro SOCD",
        "Disable the SOCD filter" => "Desactivar el filtro SOCD",
        "All keys uniquely assigned." => "Todas las teclas son únicas.",
        "4 directions mapped" => "4 direcciones asignadas",
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
        "Starting…" => "Iniciando…",
        "Stopping…" => "Deteniendo…",
        "Start measurement" => "Iniciar medición",
        "Stop measurement" => "Detener medición",
        "Reset session" => "Reiniciar sesión",
        "Records your mapped key-pair timing for this session." => {
            "Registra los tiempos de los pares de teclas de esta sesión."
        }
        "No measurement results yet." => "Aún no hay resultados.",
        "Physical key edges" => "Cambios de tecla",
        "Valid paired samples" => "Muestras válidas",
        "Physical overlap share" => "Proporción de solapamiento físico",
        "Indistinguishable share" => "Proporción casi simultánea",
        "INPUT PATTERN" => "PATRÓN",
        "SAMPLES" => "MUESTRAS",
        "MIN" => "MÍN.",
        "MAX" => "MÁX.",
        "Neutral transition" => "Transición neutra",
        "Physical overlap" => "Solapamiento físico",
        "Indistinguishable" => "Casi simultáneo",
        "Based on P10-P50 input timings, excluding indistinguishable inputs." => {
            "Rangos P10–P50 de ambos ejes, sin muestras casi simultáneas."
        }
        "Apply suggestions" => "Usar recomendaciones",
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
        "Detect an opposite-direction overlap." => "Detecta un solapamiento de teclas opuestas.",
        "Release the previous key output immediately." => {
            "Suelta la salida de la tecla anterior de inmediato."
        }
        "Send the new key immediately. 0 ms added delay" => {
            "Envía la nueva tecla de inmediato. 0 ms de retardo añadido"
        }
        "Send the new key immediately." => "Envía la nueva tecla de inmediato.",
        "Wait a random time within {range}. This gap sends no input" => {
            "Espera un tiempo aleatorio dentro de {range}. Este intervalo no envía ninguna entrada"
        }
        "Send the new key once the wait ends." => "Envía la nueva tecla al terminar la espera.",
        "Wait a random time within {range}. The overlap stays live" => {
            "Espera un tiempo aleatorio dentro de {range}. El solapamiento sigue activo"
        }
        "Release the previous key output once the wait ends." => {
            "Suelta la salida de la tecla anterior al terminar la espera."
        }
        "Click keycap to rebind" => "Pulsa una tecla para reasignarla",
        "Press a new key on your keyboard..." => "Pulsa una tecla nueva...",
        "ESC Cancel" => "ESC Cancelar",
        _ => return None,
    })
}
