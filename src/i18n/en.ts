/**
 * English panel strings: the source of truth for every user-visible string.
 *
 * Keys are English and nested by screen or component. `{name}` is replaced by a parameter; an
 * object with `one`/`other` is a plural picked by the `count` parameter. Every key here must
 * also exist in `es.ts` (the type check and a test enforce it).
 */
export const en = {
  app: {
    navLabel: "Main",
    nav: {
      home: "Home",
      settings: "Settings",
      usage: "Usage",
    },
    version: "Version {version}",
    infoError: "Could not read app info: {error}",
  },

  // Translator language names come from `Intl.DisplayNames` (see `lib/languages.ts`).
  languages: {
    /** Source chip of a route: the translator detects the spoken language on its own. */
    auto: "Auto",
    /** The auto-detected source inside a sentence ("Any language to Spanish"). */
    anyName: "Any language",
    route: "{from} to {to}",
  },

  home: {
    title: "Home",
    description:
      "Two routes, one for each direction. Choose languages and devices in Settings.",
    settingsError: "Could not load settings: {error}",
    incoming: {
      eyebrow: "Incoming · others → you",
      title: "Hear others in {language}",
      start: "Start incoming",
      stop: "Stop incoming",
      level: "App audio",
      captions: "What you hear",
      captionsEmpty: "The translation of what others say shows up here.",
      help: "While it runs, the original audio plays at {volume} under the translation. It goes back to its volume when you stop.",
      blocked: {
        noApiKey: "Add your OpenAI API key in Settings to start.",
        outputMissing:
          "The headphones chosen in Settings are not connected. Plug them in, or choose another output.",
      },
      stopped: "The incoming translation stopped: {error}",
      reconnecting:
        "Lost the connection to OpenAI. Reconnecting… Until then the original audio plays at its lowered volume, without translation.",
    },
    outgoing: {
      eyebrow: "Outgoing · you → others",
      title: "Others hear you in {language}",
      start: "Start outgoing",
      stop: "Stop outgoing",
      level: "Microphone",
      captions: "What others hear",
      captionsEmpty: "The translation of what you say shows up here.",
      help: 'In Discord / WhatsApp Web choose "CABLE Output" as your microphone.',
      notCable:
        "The output in Settings is not a known virtual cable. Others only hear you if their app records from it.",
      blocked: {
        noApiKey: "Add your OpenAI API key in Settings to start.",
        noOutput:
          "Choose the virtual cable (for example CABLE Input) as the output in Settings to start.",
        outputMissing:
          "The output chosen in Settings is not connected. Install or plug in the virtual cable, or choose another output.",
      },
      stopped: "The outgoing translation stopped: {error}",
      reconnecting:
        "Lost the connection to OpenAI. Reconnecting… Until then others hear nothing from you.",
    },
    goToSettings: "Go to Settings",
    status: {
      idle: "Off",
      connecting: "Connecting…",
      live: "Live",
      reconnecting: "Reconnecting…",
      reconnectingAttempt: "Reconnecting {attempt}/{max}…",
      stopping: "Finishing translation…",
      error: "Error",
    },
    listensTo: "Listens to",
    playsOn: "Plays on",
    sendsTo: "Sends to",
    wholeSystem: "Whole system",
    defaultDevice: "Default device",
    defaultMicrophone: "Default microphone",
    notSet: "Not set",
    openSettings: "Open settings",
  },

  settings: {
    title: "Settings",
    description:
      "Your OpenAI API key, and the languages, devices and app to translate for each direction.",
    uiLanguage: {
      eyebrow: "Panel",
      label: "App language",
      hint: "Language of this panel. System follows the Windows display language.",
      system: "System ({language})",
      saveError: "Could not save the app language: {error}",
    },
    loading: "Loading settings…",
    loadError: "Could not load settings: {error}",
    saveError: "Could not save settings: {error}",
    devicesError: "Could not list audio devices: {error}",
    saved: "Settings saved.",
    save: "Save",
    saving: "Saving…",
    refreshDevices: "Refresh devices",
    refreshing: "Refreshing…",
    incoming: {
      eyebrow: "Incoming",
      title: "Others → you",
      ariaLabel: "Incoming (others → you)",
      enabled: "Translate incoming audio",
      targetLanguage: "Translate others into",
      source: "Translate audio from",
      sourceSystem: "Whole system",
      sourceApp: "One app",
      sourceSystemHint: "Everything playing on this PC except LiveDub Voice.",
      sourceAppLabel: "App",
      sourceAppHint:
        "Includes its helper processes (browser tabs, call windows). Missing? Play something in it and refresh.",
      sourceAppChoose: "Choose an app…",
      sourceAppPlaying: "{name} · playing",
      sourceAppNotRunning: "{name} (not running)",
      sourceAppNone:
        "No app is playing audio right now. Play something in the app you want, then refresh.",
      sourceAppPending:
        "Until you choose an app, the whole system is translated.",
      refreshApps: "Refresh apps",
      refreshingApps: "Refreshing…",
      appsError: "Could not list the apps playing audio: {error}",
      headphones: "Headphones",
      headphonesHint: "Where you hear the translation.",
      originalVolume: "Original audio volume",
      originalVolumeHint:
        "How loud the original audio stays under the translation (with Whole system, every app is lowered). It goes back to its volume when the translation stops.",
      appliesNextStart:
        "The incoming translation is running. Changes here apply the next time you start it, except the original audio volume, which applies when you save.",
    },
    outgoing: {
      eyebrow: "Outgoing",
      title: "You → others",
      ariaLabel: "Outgoing (you → others)",
      enabled: "Translate your microphone",
      targetLanguage: "Translate you into",
      microphone: "Microphone",
      virtualCable: "Virtual cable",
      virtualCableHint:
        "Playback side of a virtual cable (e.g. CABLE Input). Pick its recording side as the mic in your call or game.",
      virtualCableMissing:
        "No virtual cable found. Install a virtual cable such as VB-CABLE, then refresh the devices.",
      appliesNextStart:
        "The outgoing translation is running. Changes here apply the next time you start it.",
    },
  },

  components: {
    secretField: {
      show: "Show key",
      hide: "Hide key",
    },
  },

  devices: {
    systemDefault: "System default",
    isDefault: "{name} (default)",
    saved: "Saved device",
    unavailable: "Unavailable device",
  },

  micTest: {
    groupLabel: "Microphone test",
    title: "Test microphone",
    start: "Start test",
    stop: "Stop test",
    level: "Microphone level",
    listen: "Listen to myself",
    listenHint:
      "Plays your microphone on your headphones (Incoming → Headphones). Wear headphones to avoid feedback.",
    stopped: "The microphone test stopped: {detail}",
  },

  apiKey: {
    eyebrow: "OpenAI",
    title: "API key",
    ariaLabel: "OpenAI API key",
    intro:
      "Create a key at platform.openai.com → API keys. LiveDub Voice stores it in Windows Credential Manager and only sends it to OpenAI. Translation costs about {price} per audio minute for each active direction (≈ {hourly}/hour with both on), billed to your OpenAI account.",
    savedKey: "Saved key: {key}",
    label: "API key",
    replaceLabel: "Replace API key",
    save: "Save and validate",
    checkAgain: "Check again",
    remove: "Remove",
    confirmLabel: "Confirm",
    confirmText:
      "Remove the saved key from this PC? Translation stops working until you add a key again.",
    confirmRemove: "Yes, remove key",
    cancel: "Cancel",
    readError: "Could not read the API key: {error}",
    saveError: "Could not save the key: {error}",
    checkError: "Could not check the key: {error}",
    removeError: "Could not remove the key: {error}",
    pill: {
      loading: "Loading",
      notSet: "Not set",
      checking: "Checking",
      valid: "Valid",
      invalidKey: "Invalid",
      noModelAccess: "No model access",
      rateLimited: "Rate limited",
      offline: "Offline",
      unexpected: "Could not check",
    },
    result: {
      valid:
        "OpenAI accepted the key. This check does not spend credits, so a key with no credits also passes: if translation fails later, check billing on platform.openai.com.",
      invalidKey:
        "OpenAI rejected this key. Check that it was copied completely, or create a new one.",
      noModelAccess:
        "The key works, but this account cannot use gpt-realtime-translate yet. Make sure billing is set up on platform.openai.com and that the key's project has access to the model.",
      rateLimited:
        "OpenAI is rate limiting requests. The key is saved; check again in a minute.",
      offline:
        "Could not reach OpenAI. The key is saved; check your connection and try again.",
      unexpected:
        "OpenAI answered unexpectedly. The key is saved; try again later.",
      unexpectedStatus:
        "OpenAI answered unexpectedly (HTTP {status}). The key is saved; try again later.",
    },
  },

  usage: {
    title: "Usage",
    description:
      "Minutes translated and estimated cost, per session and over time.",
    estimate:
      "Estimated on this computer at {price} per audio minute of each direction. This is not your OpenAI bill: the real one is on platform.openai.com.",
    loadError: "Could not load usage: {error}",
    emptyTitle: "No usage yet",
    emptyText:
      "Once you start translating, the minutes for each direction and their estimated cost show up here. They are counted on this computer at {price} per audio minute; LiveDub Voice does not read your OpenAI billing.",
    minutes: {
      one: "{count} minute",
      other: "{count} minutes",
    },
    minutesShort: "{minutes} min",
    session: {
      eyebrow: "This session",
      incoming: "Incoming · others → you",
      outgoing: "Outgoing · you → others",
      live: "Live",
      last: "Last session",
      none: "Not started since LiveDub Voice opened.",
      minutes: "Minutes",
      cost: "Estimated cost",
    },
    totals: {
      eyebrow: "Totals",
      title: "Today and this month",
      today: "Today",
      month: "This month",
      incoming: "Incoming",
      outgoing: "Outgoing",
      cost: "Estimated cost",
    },
    history: {
      eyebrow: "History",
      title: "By day",
      caption: "Minutes and estimated cost per day, newest first",
      day: "Day",
      incoming: "Incoming",
      outgoing: "Outgoing",
      cost: "Cost",
    },
    cap: {
      eyebrow: "Budget",
      title: "Spending cap",
      ariaLabel: "Spending cap",
      intro:
        "Stops the translations when the estimated cost reaches a limit, and warns at 80 %. Leave a field empty for no limit.",
      daily: "Daily cap (USD)",
      dailyHint: "Starts over at midnight, your local time.",
      monthly: "Monthly cap (USD)",
      monthlyHint: "Starts over on the 1st of each month.",
      placeholder: "No limit",
      save: "Save cap",
      saving: "Saving…",
      saved: "Spending cap saved.",
      invalid:
        "Enter an amount between 0.01 and 10,000 US dollars, or leave it empty for no limit.",
      loadError: "Could not load the spending cap: {error}",
      saveError: "Could not save the spending cap: {error}",
      openaiBudget: "Set a budget in your OpenAI account",
      openaiHint:
        "This cap only covers LiveDub Voice on this computer, and the cost is estimated. A budget in the OpenAI dashboard is the real safety net.",
      openError: "Could not open the OpenAI dashboard: {error}",
    },
  },

  /** The spending cap's notices, on Home, Usage and next to Start. */
  cap: {
    period: {
      daily: "daily",
      monthly: "monthly",
    },
    warning:
      "You have used {percent} of your {period} spending cap ({spent} of {limit}).",
    reached:
      "You reached your {period} spending cap ({spent} of {limit}). Translations stay off until you raise the cap or keep going for today.",
    stopped:
      "Translation stopped: you reached your {period} spending cap ({spent} of {limit}).",
    overridden:
      "Over your {period} spending cap ({spent} of {limit}): translating anyway until midnight.",
    startBlocked:
      "Your spending cap is reached. Keep translating for the rest of today?",
    startBlockedLabel: "Spending cap reached",
    continueToday: "Keep going today",
    raise: "Raise the cap",
    cancel: "Cancel",
    continueError: "Could not keep going over the cap: {error}",
  },

  /** Messages for the `code` of an `AppError` from the core. `{detail}` is the core's detail. */
  errors: {
    io: "A file could not be read or written ({detail}).",
    invalid_settings: "The settings are not valid ({detail}).",
    invalid_api_key: "This does not look like an OpenAI API key ({detail}).",
    no_api_key: "No OpenAI API key is set.",
    credential_store:
      "Windows Credential Manager could not handle the key ({detail}).",
    audio: "Audio device error ({detail}).",
    internal: "Something went wrong inside LiveDub Voice ({detail}).",
    openai_auth: "OpenAI rejected the API key.",
    openai_no_model_access:
      "This OpenAI account cannot use gpt-realtime-translate.",
    openai_rate_limited: "OpenAI rate limit reached. Try again in a minute.",
    openai_quota:
      "Your OpenAI account has no credits or quota left. Check billing on platform.openai.com.",
    openai_unsupported_language:
      "OpenAI cannot translate into the chosen language. Choose another one in Settings.",
    openai_server: "OpenAI is having trouble ({detail}). Try again later.",
    reconnect_failed:
      "Lost the connection to OpenAI and could not reconnect ({detail}). Check your connection and start again.",
    network: "Could not reach OpenAI ({detail}).",
    openai_protocol: "Unexpected answer from OpenAI ({detail}).",
    openai_api: "OpenAI returned an error ({detail}).",
    translator_closed: "The translation session is closed.",
    no_output_device:
      "No output is chosen for the outgoing translation. Choose the virtual cable in Settings.",
    device_missing: "A device chosen in Settings is not connected ({detail}).",
    source_app_not_running:
      "The app chosen in Settings is not running ({detail}). Open it, or translate the whole system.",
    spending_cap_reached:
      "Your spending cap is reached ({detail}). Raise it on the Usage screen, or keep going for today.",
  },
} as const;
