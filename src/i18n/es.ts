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
      stop: "Detener entrante",
      level: "Audio de la app",
      captions: "Lo que llega a tus auriculares",
      captionsEmpty: "Aquí aparece la traducción de lo que dicen los demás.",
      help: "Mientras está activa, el audio original suena al {volume} debajo de la traducción. Vuelve a su volumen al detenerla.",
      blocked: {
        noApiKey: "Para empezar, falta la API key de OpenAI en Configuración.",
        outputMissing:
          "Los auriculares elegidos en Configuración no están conectados. Hay que conectarlos o elegir otra salida.",
      },
      stopped: "La traducción entrante se detuvo: {error}",
      autoOff: {
        one: "La traducción entrante se apagó sola tras {count} minuto sin voz.",
        other:
          "La traducción entrante se apagó sola tras {count} minutos sin voz.",
      },
      reconnecting:
        "Se perdió la conexión con OpenAI. Reconectando… Mientras tanto, el audio original suena con el volumen bajado y sin traducción.",
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
      autoOff: {
        one: "La traducción saliente se apagó sola tras {count} minuto sin voz.",
        other:
          "La traducción saliente se apagó sola tras {count} minutos sin voz.",
      },
      reconnecting:
        "Se perdió la conexión con OpenAI. Reconectando… Mientras tanto, los demás no escuchan nada de tu voz.",
      pushToTalk: {
        hold: "Mantener {key} para hablar",
        talking: "Hablando: los demás te escuchan",
        startFirst:
          "Para hablar con {key}, primero hay que iniciar la traducción saliente.",
      },
    },
    goToSettings: "Ir a Configuración",
    status: {
      idle: "Apagado",
      connecting: "Conectando…",
      live: "En vivo",
      reconnecting: "Reconectando…",
      reconnectingAttempt: "Reconectando {attempt}/{max}…",
      stopping: "Terminando la traducción…",
      error: "Error",
    },
    listensTo: "Escucha",
    playsOn: "Suena en",
    sendsTo: "Envía a",
    wholeSystem: "Todo el sistema",
    defaultDevice: "Dispositivo predeterminado",
    defaultMicrophone: "Micrófono predeterminado",
    shortcut: "Atajo",
    notSet: "Sin elegir",
    openSettings: "Abrir configuración",
    howToUse: "Cómo usar",
    hideGuide: "Ocultar guía",
    voice: {
      label: "Voz",
      speaking: "Hablando",
      listening: "Escuchando",
      pausedHint: "Nadie habla: no se envía audio a OpenAI.",
      keyUpHint:
        "La tecla para hablar está suelta: no se envía audio a OpenAI.",
      sendingHint: "Se está enviando audio a OpenAI.",
    },
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
      source: "Traducir el audio de",
      sourceSystem: "Todo el sistema",
      sourceApp: "Una app",
      sourceSystemHint: "Todo lo que suena en esta PC, excepto LiveDub Voice.",
      sourceAppLabel: "App",
      sourceAppHint:
        "Incluye sus procesos auxiliares (pestañas del navegador, ventanas de llamada). Si no aparece, reproducir algo en ella y actualizar.",
      sourceAppChoose: "Elegir una app…",
      sourceAppPlaying: "{name} · sonando",
      sourceAppNotRunning: "{name} (no está abierta)",
      sourceAppNone:
        "Ninguna app está reproduciendo audio ahora. Reproducir algo en la app deseada y actualizar.",
      sourceAppPending: "Hasta elegir una app, se traduce todo el sistema.",
      refreshApps: "Actualizar apps",
      refreshingApps: "Actualizando…",
      appsError: "No se pudieron listar las apps con audio: {error}",
      headphones: "Auriculares",
      headphonesHint: "Donde suena la traducción.",
      originalVolume: "Volumen del audio original",
      originalVolumeHint:
        "Cuánto se sigue oyendo el audio original debajo de la traducción (con Todo el sistema, se bajan todas las apps). Vuelve a su volumen al detener la traducción.",
      appliesNextStart:
        "La traducción entrante está activa. Los cambios se aplican la próxima vez que se inicie, salvo el volumen del audio original, que se aplica al guardar.",
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
      appliesNextStart:
        "La traducción saliente está activa. Los cambios se aplican la próxima vez que se inicie.",
      talkMode: "Enviar tu voz",
      talkModes: {
        voiceDetection: "Al hablar",
        pushToTalk: "Pulsar para hablar",
      },
      talkModeHints: {
        voiceDetection:
          "La detección de voz decide cuándo se habla (ver Detección de voz).",
        pushToTalk:
          "Solo mientras se mantiene la tecla, más un segundo después de soltarla para que la frase se termine. La traducción saliente tiene que estar encendida: apagada, la tecla no hace nada.",
      },
      pushToTalkKeyHint:
        "Se mantiene mientras se habla. Con pulsar para hablar activo, Windows le quita esta tecla a las demás apps: conviene una que tu juego no use.",
      pushToTalkKeyMissing:
        "Falta elegir una tecla para usar pulsar para hablar.",
    },
    voice: {
      eyebrow: "Ambas direcciones",
      title: "Detección de voz",
      ariaLabel: "Detección de voz",
      pauseWhenSilent: "No enviar el silencio",
      pauseWhenSilentHint:
        "Solo la voz va a OpenAI, más un segundo después de cada frase para que se termine de traducir. El silencio que no se envía no cuenta en Uso. Conviene desactivarlo si se pierden las primeras palabras de las frases.",
      autoOff: "Apagar cuando nadie habla",
      autoOffHint:
        "Detiene una dirección después de este tiempo sin voz, para que una traducción olvidada no siga activa.",
      autoOffNever: "Nunca",
      autoOffMinutes: {
        one: "Tras {count} minuto",
        other: "Tras {count} minutos",
      },
      appliesNextStart:
        "Hay una traducción activa. Los cambios se aplican la próxima vez que se inicie.",
    },
    hotkeys: {
      eyebrow: "Teclado",
      title: "Atajos de teclado",
      ariaLabel: "Atajos de teclado",
      intro:
        "Funcionan aunque un juego o una llamada tenga el foco y LiveDub Voice esté oculto en la bandeja. Los cambios se aplican al guardar.",
      actions: {
        toggleIncoming: "Encender o apagar la entrante",
        toggleOutgoing: "Encender o apagar la saliente",
        toggleBoth: "Encender o apagar ambas",
        pushToTalk: "Tecla para hablar",
      },
      none: "Ninguno",
      recording: "Presionar las teclas… (Esc para cancelar)",
      change: "Cambiar",
      clear: "Quitar el atajo",
      resetDefaults: "Volver a los predeterminados",
      keysHint:
        "Mientras LiveDub Voice está abierto, Windows le quita estas teclas a todas las apps. Letras, números y otras teclas de escritura necesitan Ctrl, Alt o Win; las teclas de función (F1–F24), Pausa, Bloq Despl e Insert también sirven solas. No se puede usar un modificador solo ni un botón del mouse: en su lugar, asignar el botón a F13–F24 desde el software del mouse.",
      soundCues: "Sonidos de aviso",
      soundCuesHint:
        "Un sonido corto y suave en tus auriculares cuando un atajo enciende (sube) o apaga (baja) una traducción.",
      problems: {
        needsModifier:
          "{shortcut} necesita Ctrl, Alt o Win: sola, esa tecla dejaría de escribirse en todas las apps.",
        unsupportedKey:
          "{shortcut} no puede ser un atajo. Sirve una letra, un número o una tecla de función con Ctrl, Alt o Win.",
        conflict: "{shortcut} ya se usa para “{action}”.",
        unavailable:
          "{shortcut} no funciona: otra app lo usa o Windows lo reserva. Hay que elegir otro.",
        typesCharacter:
          "{shortcut} escribe un carácter con tu distribución de teclado (AltGr), así que no se usa. Hay que elegir otro.",
      },
    },
    window: {
      eyebrow: "Ventana",
      title: "Al cerrar la ventana",
      ariaLabel: "Al cerrar la ventana",
      closeQuitsApp: "Cerrar la ventana sale de la app",
      closeQuitsAppHint:
        "Desactivado, cerrar la ventana oculta LiveDub Voice en la bandeja del sistema (junto al reloj) y las traducciones siguen activas.",
      quit: "Salir de LiveDub Voice",
      quitHint:
        "Detiene ambas traducciones, devuelve el volumen a las apps y cierra la app.",
      quitError: "No se pudo salir: {error}",
    },
    setup: {
      eyebrow: "Configuración inicial",
      title: "Asistente de inicio",
      ariaLabel: "Asistente de inicio",
      text: "Repetir los pasos de bienvenida: idioma de la app, API key, auriculares, cable virtual, privacidad y la guía rápida.",
      runAgain: "Repetir la configuración inicial",
    },
  },

  onboarding: {
    stepsLabel: "Pasos de la configuración",
    done: "hecho",
    stepOf: "Paso {step} de {total}",
    back: "Atrás",
    next: "Siguiente",
    finish: "Empezar a usar LiveDub Voice",
    skip: "Omitir configuración",
    skipHint: "Cada paso se puede completar más tarde en Configuración.",
    steps: {
      welcome: "Bienvenida",
      apiKey: "API key de OpenAI",
      headphones: "Auriculares",
      cable: "Cable virtual",
      privacy: "Privacidad",
      guide: "Guía rápida",
    },
    welcome: {
      title: "Bienvenida a LiveDub Voice",
      description:
        "Un intérprete en vivo para llamadas y juegos. Unos pocos pasos cortos y queda listo; cualquiera se puede omitir y retomar en Configuración.",
      incomingText:
        "Escuchar a las personas de la llamada o del juego en tu idioma, en tus auriculares.",
      outgoingText:
        "Los demás te escuchan en el suyo, a través de un micrófono virtual. No necesitan instalar nada.",
    },
    apiKey: {
      title: "Conectar la cuenta de OpenAI",
      description:
        "LiveDub Voice traduce con el modelo gpt-realtime-translate de OpenAI, con tu propia API key. OpenAI cobra directamente a tu cuenta.",
      howEyebrow: "Obtener una key",
      howTitle: "Crear una API key",
      howStep1:
        "Iniciar sesión en platform.openai.com y agregar un medio de pago en Billing.",
      howStep2: "Crear una key en API keys y copiarla.",
      howStep3:
        "Pegarla aquí abajo. Se guarda en el Administrador de credenciales de Windows y solo se envía a OpenAI.",
      openKeys: "Abrir platform.openai.com/api-keys",
      costEyebrow: "Costo",
      costTitle: "Cuánto cuesta",
      cost: "{price} por minuto de audio por cada dirección activa: unos {hourly} por hora con una dirección, el doble con las dos.",
      budget:
        "Conviene fijar un presupuesto mensual en el panel de OpenAI: es la verdadera red de seguridad. También se puede fijar el límite de gasto propio de LiveDub Voice en la pantalla Uso.",
      openBudget: "Fijar un presupuesto en OpenAI",
      openError: "No se pudo abrir el navegador: {error}",
    },
    headphones: {
      title: "Usar auriculares",
      description:
        "Con parlantes, el micrófono capta la traducción de los demás y la devuelve a la llamada: escuchan un eco de su propia voz.",
      cardEyebrow: "Recomendado",
      cardTitle: "Auriculares o headset",
      point1:
        "La traducción entrante suena en tus auriculares. Se eligen en Configuración → Entrante → Auriculares (hasta entonces se usa la salida predeterminada).",
      point2:
        "Sirven auriculares con cable o inalámbricos. Lo ideal es un headset con micrófono.",
    },
    cable: {
      title: "Preparar un micrófono virtual",
      description:
        "Windows no trae un micrófono virtual propio. LiveDub Voice reproduce tu voz traducida en un cable de audio virtual, y la llamada o el juego usa ese cable como micrófono.",
      eyebrow: "Saliente",
      searching: "Buscando un cable virtual…",
      listError: "No se pudieron listar los dispositivos de audio: {error}",
      foundTitle: "Cable virtual encontrado",
      ready: "Listo",
      foundText:
        "LiveDub Voice envía tu voz traducida a {name}. En la llamada o el juego, elegir como micrófono el lado de grabación del cable (CABLE Output en VB-CABLE).",
      chooseLabel: "Enviar tu traducción a",
      chosen:
        "Elegido como salida de la traducción saliente. Se puede cambiar en Configuración.",
      saveError: "No se pudo elegir el cable: {error}",
      missingTitle: "No se encontró ningún cable virtual",
      missing: "Sin instalar",
      missingText:
        "Instalar VB-CABLE, un cable virtual gratuito: descargarlo, descomprimirlo, ejecutar VBCABLE_Setup_x64.exe como administrador y reiniciar Windows si lo pide. Después, volver a buscar. VoiceMeeter y Virtual-Audio-Driver también sirven.",
      download: "Descargar VB-CABLE",
      checkAgain: "Volver a buscar",
      checking: "Buscando…",
      incomingOnly:
        "¿Solo interesa escuchar traducidos a los demás? Este paso se puede omitir: la traducción entrante no necesita cable.",
      openError: "No se pudo abrir el navegador: {error}",
    },
    privacy: {
      title: "Privacidad y consentimiento",
      description: "Qué envía LiveDub Voice, y a quién.",
      lightIntro:
        "La configuración ya está hecha. Antes de seguir, dos cosas importantes: qué envía LiveDub Voice y cómo usarlo en tus apps.",
      othersTitle: "Avisar que se usa un traductor",
      others:
        "La traducción entrante envía a OpenAI las voces de las personas con quienes se habla. Conviene avisarles que se usa un traductor en vivo y respetar las reglas de la llamada, el servidor o el juego.",
      openaiTitle: "El audio solo va a OpenAI",
      openai:
        "El audio se transmite a OpenAI con tu API key y bajo los términos de OpenAI, y a nadie más. LiveDub Voice no guarda grabaciones.",
      telemetryTitle: "Sin telemetría",
      telemetry:
        "LiveDub Voice no recopila datos de uso ni informes de errores. Los minutos y costos se cuentan solo en esta computadora.",
    },
  },

  guide: {
    title: "Cómo usar LiveDub Voice",
    description:
      "Cada dirección se inicia desde Inicio, y en la llamada o el juego se elige el cable virtual como micrófono.",
    incoming: {
      eyebrow: "Entrante · voces de los demás",
      title: "Escuchar a los demás en tu idioma",
      step1:
        "En Configuración → Entrante, elegir la app a traducir (Discord, el navegador, Teams, un juego) o todo el sistema, y tus auriculares.",
      step2: "En Inicio, pulsar Iniciar entrante.",
      step3:
        "El audio original baja de volumen y la traducción suena encima, un par de segundos después.",
    },
    outgoing: {
      eyebrow: "Saliente · tu voz",
      title: "Los demás te escuchan en su idioma",
      step1:
        "En la llamada o el juego, elegir CABLE Output como micrófono (ver la tabla de abajo).",
      step2: "En Inicio, pulsar Iniciar saliente y hablar con normalidad.",
      step3:
        "Los demás escuchan la traducción un par de segundos después de hablar.",
    },
    mic: {
      eyebrow: "Micrófono",
      title: "Qué micrófono elegir",
      intro:
        "Elegir CABLE Output (VB-Audio Virtual Cable), o el lado de grabación de tu cable, y no el micrófono real: LiveDub Voice ya lo escucha.",
      caption: "Dónde se elige el micrófono en cada app",
      app: "App",
      where: "Dónde",
      discord: "Discord",
      discordWhere: "Ajustes de usuario → Voz y vídeo → Dispositivo de entrada",
      meet: "Google Meet",
      meetWhere: "Configuración (ícono de engranaje) → Audio → Micrófono",
      teams: "Microsoft Teams",
      teamsWhere: "Configuración → Dispositivos → Micrófono",
      whatsapp: "WhatsApp Web",
      whatsappWhere:
        "Usa el micrófono del navegador: se elige desde el ícono de cámara/micrófono de la barra de direcciones, o en la configuración de sitios del navegador",
      games: "Juegos",
      gamesWhere:
        "Ajustes del chat de voz → Dispositivo de entrada. Si no existe, poner CABLE Output como dispositivo de grabación predeterminado en la configuración de Sonido de Windows",
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
    estimate:
      "Estimado en esta computadora a {price} por minuto de audio de cada dirección. No es tu factura de OpenAI: la real está en platform.openai.com.",
    loadError: "No se pudo cargar el uso: {error}",
    emptyTitle: "Todavía no hay uso",
    emptyText:
      "Al empezar a traducir, aquí aparecen los minutos de cada dirección y su costo estimado. Se cuentan en esta computadora a {price} por minuto de audio; LiveDub Voice no lee tu facturación de OpenAI.",
    minutes: {
      one: "{count} minuto",
      other: "{count} minutos",
    },
    minutesShort: "{minutes} min",
    session: {
      eyebrow: "Esta sesión",
      incoming: "Entrante · voces de los demás",
      outgoing: "Saliente · tu voz",
      live: "En vivo",
      last: "Última sesión",
      none: "Sin iniciar desde que se abrió LiveDub Voice.",
      minutes: "Minutos",
      cost: "Costo estimado",
    },
    totals: {
      eyebrow: "Totales",
      title: "Hoy y este mes",
      today: "Hoy",
      month: "Este mes",
      incoming: "Entrante",
      outgoing: "Saliente",
      cost: "Costo estimado",
    },
    history: {
      eyebrow: "Historial",
      title: "Por día",
      caption:
        "Minutos y costo estimado por día, del más reciente al más antiguo",
      day: "Día",
      incoming: "Entrante",
      outgoing: "Saliente",
      cost: "Costo",
    },
    cap: {
      eyebrow: "Presupuesto",
      title: "Límite de gasto",
      ariaLabel: "Límite de gasto",
      intro:
        "Detiene las traducciones cuando el costo estimado llega a un límite, y avisa al 80 %. Un campo vacío significa sin límite.",
      daily: "Límite diario (USD)",
      dailyHint: "Vuelve a cero a la medianoche, en tu hora local.",
      monthly: "Límite mensual (USD)",
      monthlyHint: "Vuelve a cero el día 1 de cada mes.",
      placeholder: "Sin límite",
      save: "Guardar límite",
      saving: "Guardando…",
      saved: "Límite de gasto guardado.",
      invalid:
        "Hace falta un monto entre 0,01 y 10.000 dólares, o dejar el campo vacío para no tener límite.",
      loadError: "No se pudo cargar el límite de gasto: {error}",
      saveError: "No se pudo guardar el límite de gasto: {error}",
      openaiBudget: "Definir un presupuesto en tu cuenta de OpenAI",
      openaiHint:
        "Este límite solo cubre LiveDub Voice en esta computadora, y el costo es estimado. Un presupuesto en el panel de OpenAI es la protección real.",
      openError: "No se pudo abrir el panel de OpenAI: {error}",
    },
  },

  tray: {
    open: "Abrir LiveDub Voice",
    quit: "Salir de LiveDub Voice",
    incoming: "Entrante (los demás → tu idioma)",
    outgoing: "Saliente (tu voz → los demás)",
    states: {
      idle: "Apagada",
      connecting: "Conectando…",
      live: "En vivo",
      reconnecting: "Reconectando…",
      stopping: "Terminando…",
      error: "Error",
    },
  },

  closeToTray: {
    title: "LiveDub Voice sigue activo",
    text: "Al cerrar la ventana, LiveDub Voice se oculta en la bandeja del sistema, junto al reloj, y las traducciones siguen activas. Un clic en el icono de la bandeja vuelve a abrirlo; con clic derecho se puede iniciar o detener una traducción, o salir.",
    settingHint:
      "Para salir al cerrar la ventana, activar “Cerrar la ventana sale de la app” en Configuración.",
    hide: "Entendido, ocultar en la bandeja",
    quit: "Salir de LiveDub Voice",
    error: "No se pudo cerrar la ventana: {error}",
  },

  cap: {
    period: {
      daily: "diario",
      monthly: "mensual",
    },
    warning:
      "Ya se usó el {percent} de tu límite de gasto {period} ({spent} de {limit}).",
    reached:
      "Se alcanzó tu límite de gasto {period} ({spent} de {limit}). Las traducciones quedan apagadas hasta subir el límite o seguir por hoy.",
    stopped:
      "La traducción se detuvo: se alcanzó tu límite de gasto {period} ({spent} de {limit}).",
    overridden:
      "Por encima de tu límite de gasto {period} ({spent} de {limit}): se sigue traduciendo hasta la medianoche.",
    startBlocked:
      "Se alcanzó tu límite de gasto. ¿Seguir traduciendo por el resto del día?",
    startBlockedLabel: "Límite de gasto alcanzado",
    continueToday: "Seguir por hoy",
    raise: "Subir el límite",
    cancel: "Cancelar",
    continueError: "No se pudo seguir por encima del límite: {error}",
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
      "Se alcanzó el límite de uso de OpenAI. Conviene volver a intentar en un minuto.",
    openai_quota:
      "La cuenta de OpenAI no tiene créditos ni cuota disponibles. Revisar la facturación en platform.openai.com.",
    openai_unsupported_language:
      "OpenAI no puede traducir al idioma elegido. Elegir otro en Configuración.",
    openai_server:
      "OpenAI tiene problemas ({detail}). Conviene volver a intentar más tarde.",
    reconnect_failed:
      "Se perdió la conexión con OpenAI y no se pudo reconectar ({detail}). Revisar la conexión y volver a iniciar.",
    network: "No se pudo conectar con OpenAI ({detail}).",
    openai_protocol: "Respuesta inesperada de OpenAI ({detail}).",
    openai_api: "OpenAI devolvió un error ({detail}).",
    translator_closed: "La sesión de traducción está cerrada.",
    no_output_device:
      "No hay ninguna salida elegida para la traducción saliente. El cable virtual se elige en Configuración.",
    device_missing:
      "Un dispositivo elegido en Configuración no está conectado ({detail}).",
    source_app_not_running:
      "La app elegida en Configuración no está abierta ({detail}). Abrirla o traducir todo el sistema.",
    spending_cap_reached:
      "Se alcanzó tu límite de gasto ({detail}). Se puede subir en la pantalla Uso, o seguir por hoy.",
    hotkey_unavailable:
      "Windows rechazó el atajo {detail}: puede que otra app lo use. Hay que elegir otro.",
    hotkey_types_character:
      "El atajo {detail} escribe un carácter con tu distribución de teclado (AltGr). Hay que elegir otro.",
  },
};
