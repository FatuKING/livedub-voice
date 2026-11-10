import type { Messages } from "./types";

/**
 * Spanish panel strings. Neutral Spanish: no "tú"/"vos" verb forms (infinitives, impersonal
 * phrasing, and "tu"/"te", which both share). Product terms stay as they are.
 */
export const es: Messages = {
  app: {
    navLabel: "Principal",
    nav: {
      home: "Inicio",
      settings: "Configuración",
      usage: "Uso",
    },
    version: "Versión {version}",
    infoError: "No se pudo leer la información de la app: {error}",
  },

  languages: {
    auto: "Cualquiera",
    anyName: "cualquier idioma",
    route: "De {from} a {to}",
  },

  home: {
    title: "Inicio",
    description:
      "Dos rutas, una por dirección. Los idiomas y dispositivos se eligen en Configuración.",
    settingsError: "No se pudo cargar la configuración: {error}",
    incoming: {
      eyebrow: "Entrante · voces de los demás",
      title: "Escuchar a los demás en {language}",
      start: "Iniciar entrante",
    },
    outgoing: {
      eyebrow: "Saliente · tu voz",
      title: "Que los demás te escuchen en {language}",
      start: "Iniciar saliente",
      stop: "Detener saliente",
      level: "Micrófono",
      captions: "Lo que escuchan los demás",
      captionsEmpty: "Aquí aparece la traducción de tu voz.",
      help: 'En Discord / WhatsApp Web, elegir "CABLE Output" como micrófono.',
      notCable:
        "La salida elegida en Configuración no es un cable virtual conocido. Los demás solo oyen la traducción si su app graba desde ella.",
      blocked: {
        noApiKey: "Para empezar, falta la API key de OpenAI en Configuración.",
        noOutput:
          "Para empezar, falta elegir el cable virtual (por ejemplo CABLE Input) como salida en Configuración.",
        outputMissing:
          "La salida elegida en Configuración no está conectada. Hay que instalar o conectar el cable virtual, o elegir otra salida.",
      },
      stopped: "La traducción saliente se detuvo: {error}",
      goToSettings: "Ir a Configuración",
    },
    status: {
      idle: "Apagado",
      connecting: "Conectando…",
      live: "En vivo",
      error: "Error",
    },
    comingSoon: "Próximamente",
    comingSoonNote:
      "La traducción en vivo todavía no está disponible. Llega en una próxima versión.",
    listensTo: "Escucha",
    playsOn: "Suena en",
    sendsTo: "Envía a",
    wholeSystem: "Todo el sistema",
    defaultDevice: "Dispositivo predeterminado",
    defaultMicrophone: "Micrófono predeterminado",
    notSet: "Sin elegir",
    openSettings: "Abrir configuración",
  },

  settings: {
    title: "Configuración",
    description:
      "Tu API key de OpenAI y, para cada dirección, los idiomas, los dispositivos y la app que se traduce.",
    uiLanguage: {
      eyebrow: "Panel",
      label: "Idioma de la app",
      hint: "Idioma de este panel. «Sistema» sigue el idioma de Windows.",
      system: "Sistema ({language})",
      saveError: "No se pudo guardar el idioma de la app: {error}",
    },
    loading: "Cargando la configuración…",
    loadError: "No se pudo cargar la configuración: {error}",
    saveError: "No se pudo guardar la configuración: {error}",
    devicesError: "No se pudieron listar los dispositivos de audio: {error}",
    saved: "Configuración guardada.",
    save: "Guardar",
    saving: "Guardando…",
    refreshDevices: "Actualizar dispositivos",
    refreshing: "Actualizando…",
    incoming: {
      eyebrow: "Entrante",
      title: "Voces de los demás",
      ariaLabel: "Entrante (voces de los demás)",
      enabled: "Traducir el audio entrante",
      targetLanguage: "Idioma para escuchar a los demás",
      sourceApp: "App de origen",
      sourceAppHint:
        "Vacío = traducir todo el sistema (excepto LiveDub Voice).",
      sourceAppPlaceholder: "p. ej. Discord.exe",
      headphones: "Auriculares",
      headphonesHint: "Donde suena la traducción.",
      originalVolume: "Volumen del audio original",
      originalVolumeHint:
        "Cuánto se siguen oyendo las voces originales debajo de la traducción.",
    },
    outgoing: {
      eyebrow: "Saliente",
      title: "Tu voz",
      ariaLabel: "Saliente (tu voz)",
      enabled: "Traducir tu micrófono",
      targetLanguage: "Idioma en que te escuchan los demás",
      microphone: "Micrófono",
      virtualCable: "Cable virtual",
      virtualCableHint:
        "Lado de reproducción de un cable virtual (p. ej. CABLE Input). Su lado de grabación es el micrófono a elegir en la llamada o el juego.",
      virtualCableMissing:
        "No se encontró ningún cable virtual. Hace falta instalar uno, como VB-CABLE, y después actualizar los dispositivos.",
    },
  },

  components: {
    secretField: {
      show: "Mostrar la key",
      hide: "Ocultar la key",
    },
  },

  devices: {
    systemDefault: "Predeterminado del sistema",
    isDefault: "{name} (predeterminado)",
    saved: "Dispositivo guardado",
    unavailable: "Dispositivo no disponible",
  },

  micTest: {
    groupLabel: "Prueba de micrófono",
    title: "Probar el micrófono",
    start: "Iniciar prueba",
    stop: "Detener prueba",
    level: "Nivel del micrófono",
    listen: "Escucharme",
    listenHint:
      "Reproduce tu micrófono en tus auriculares (Entrante → Auriculares). Usar auriculares evita el acople.",
    stopped: "La prueba de micrófono se detuvo: {detail}",
  },

  apiKey: {
    eyebrow: "OpenAI",
    title: "API key",
    ariaLabel: "API key de OpenAI",
    intro:
      "La key se crea en platform.openai.com → API keys. LiveDub Voice la guarda en el Administrador de credenciales de Windows y solo la envía a OpenAI. Traducir cuesta alrededor de {price} por minuto de audio en cada dirección activa (≈ {hourly}/hora con las dos), y se cobra en tu cuenta de OpenAI.",
    savedKey: "Key guardada: {key}",
    label: "API key",
    replaceLabel: "Reemplazar la API key",
    save: "Guardar y validar",
    checkAgain: "Volver a comprobar",
    remove: "Quitar",
    confirmLabel: "Confirmar",
    confirmText:
      "¿Quitar la key guardada de esta PC? La traducción deja de funcionar hasta que se agregue una key de nuevo.",
    confirmRemove: "Sí, quitar la key",
    cancel: "Cancelar",
    readError: "No se pudo leer la API key: {error}",
    saveError: "No se pudo guardar la key: {error}",
    checkError: "No se pudo comprobar la key: {error}",
    removeError: "No se pudo quitar la key: {error}",
    pill: {
      loading: "Cargando",
      notSet: "Sin key",
      checking: "Comprobando",
      valid: "Válida",
      invalidKey: "No válida",
      noModelAccess: "Sin acceso al modelo",
      rateLimited: "Límite alcanzado",
      offline: "Sin conexión",
      unexpected: "No se pudo comprobar",
    },
    result: {
      valid:
        "OpenAI aceptó la key. Esta comprobación no gasta créditos, así que una key sin créditos también pasa: si la traducción falla después, conviene revisar la facturación en platform.openai.com.",
      invalidKey:
        "OpenAI rechazó esta key. Conviene revisar que se haya copiado completa, o crear una nueva.",
      noModelAccess:
        "La key funciona, pero esta cuenta todavía no puede usar gpt-realtime-translate. Hace falta tener la facturación configurada en platform.openai.com y que el proyecto de la key tenga acceso al modelo.",
      rateLimited:
        "OpenAI está limitando las solicitudes. La key quedó guardada; se puede volver a comprobar en un minuto.",
      offline:
        "No se pudo conectar con OpenAI. La key quedó guardada; conviene revisar la conexión y volver a intentar.",
      unexpected:
        "OpenAI respondió algo inesperado. La key quedó guardada; se puede volver a intentar más tarde.",
      unexpectedStatus:
        "OpenAI respondió algo inesperado (HTTP {status}). La key quedó guardada; se puede volver a intentar más tarde.",
    },
  },

  usage: {
    title: "Uso",
    description:
      "Minutos traducidos y costo estimado, por sesión y a lo largo del tiempo.",
    emptyTitle: "Todavía no hay uso",
    emptyText:
      "Al empezar a traducir, aquí aparecen los minutos de cada dirección y su costo estimado. Se cuentan en esta computadora a {price} por minuto de audio; LiveDub Voice no lee tu facturación de OpenAI.",
    minutes: {
      one: "{count} minuto",
      other: "{count} minutos",
    },
  },

  errors: {
    io: "No se pudo leer o escribir un archivo ({detail}).",
    invalid_settings: "La configuración no es válida ({detail}).",
    invalid_api_key: "Esto no parece una API key de OpenAI ({detail}).",
    no_api_key: "No hay ninguna API key de OpenAI configurada.",
    credential_store:
      "El Administrador de credenciales de Windows no pudo manejar la key ({detail}).",
    audio: "Error del dispositivo de audio ({detail}).",
    internal: "Algo falló dentro de LiveDub Voice ({detail}).",
    openai_auth: "OpenAI rechazó la API key.",
    openai_no_model_access:
      "Esta cuenta de OpenAI no puede usar gpt-realtime-translate.",
    openai_rate_limited:
      "Se alcanzó el límite de uso o la cuota de OpenAI. Conviene volver a intentar más tarde.",
    network: "No se pudo conectar con OpenAI ({detail}).",
    openai_protocol: "Respuesta inesperada de OpenAI ({detail}).",
    openai_api: "OpenAI devolvió un error ({detail}).",
    translator_closed: "La sesión de traducción está cerrada.",
    no_output_device:
      "No hay ninguna salida elegida para la traducción saliente. El cable virtual se elige en Configuración.",
    device_missing:
      "Un dispositivo elegido en Configuración no está conectado ({detail}).",
  },
};
