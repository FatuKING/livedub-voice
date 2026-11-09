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

  languages: {
    /** Language names inside a sentence. */
    name: {
      en: "English",
      es: "Spanish",
    },
    /** Language names as a standalone option. */
    option: {
      en: "English",
      es: "Spanish",
    },
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
    },
    outgoing: {
      eyebrow: "Outgoing · you → others",
      title: "Others hear you in {language}",
      start: "Start outgoing",
    },
    comingSoon: "Coming soon",
    comingSoonNote:
      "Live translation is not available yet. It is coming in a next version.",
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
      sourceApp: "Source app",
      sourceAppHint:
        "Leave empty to translate the whole system (except LiveDub Voice).",
      sourceAppPlaceholder: "e.g. Discord.exe",
      headphones: "Headphones",
      headphonesHint: "Where you hear the translation.",
      originalVolume: "Original audio volume",
      originalVolumeHint:
        "How loud the original voices stay under the translation.",
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
    emptyTitle: "No usage yet",
    emptyText:
      "Once you start translating, the minutes for each direction and their estimated cost show up here. They are counted on this computer at {price} per audio minute; LiveDub Voice does not read your OpenAI billing.",
    minutes: {
      one: "{count} minute",
      other: "{count} minutes",
    },
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
    openai_rate_limited: "OpenAI rate limit or quota reached. Try again later.",
    network: "Could not reach OpenAI ({detail}).",
    openai_protocol: "Unexpected answer from OpenAI ({detail}).",
    openai_api: "OpenAI returned an error ({detail}).",
    translator_closed: "The translation session is closed.",
  },
} as const;
