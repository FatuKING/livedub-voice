## Download

**[LiveDub-Voice\_{{VERSION}}\_x64-setup.exe](https://github.com/{{REPOSITORY}}/releases/download/{{TAG}}/LiveDub-Voice_{{VERSION}}_x64-setup.exe)**: the Windows installer. It installs for your Windows user only, so it does not ask for administrator rights, and it keeps your settings when it updates an older version. `LiveDub-Voice_x64-setup.exe` is the same file under a name that does not change between releases.

### Requirements

- Windows 10 version 2004 or later, or Windows 11 (64-bit).
- Your own [OpenAI API key](https://platform.openai.com/api-keys). OpenAI bills you for the translated audio (about $0.034 per minute).
- To translate your own voice for others: a virtual audio cable such as [VB-CABLE](https://vb-audio.com/Cable/). Translating what you hear needs nothing else.

### Windows SmartScreen

The installer is not code-signed yet, so Windows may say **"Windows protected your PC"**. Click **More info**, check that the app is _LiveDub Voice_, then click **Run anyway**.

### Checksums

`SHA256SUMS.txt` has the SHA-256 of each file. To check a download in PowerShell, compare the output of `Get-FileHash .\LiveDub-Voice_{{VERSION}}_x64-setup.exe` with:
