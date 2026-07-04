FlowOSS — Local Voice Dictation for Windows
===========================================

Press a shortcut, speak, and your words are typed into whatever app you're in.
All speech-to-text runs locally on your PC. No cloud, no account, no telemetry.


GETTING STARTED
---------------
1. Unzip this folder anywhere (e.g. Documents or Desktop). Keep the .exe and
   the .dll files together in the same folder.

2. Double-click  flowoss-desktop.exe

3. FIRST RUN ONLY: FlowOSS downloads its speech models (about 800 MB) once.
   A small pill near the bottom of the screen shows the progress. This can take
   a few minutes on the first launch depending on your connection. After that,
   startup is fast and works fully offline.

4. Look for the FlowOSS icon in the system tray (bottom-right, near the clock).
   Right-click it for Settings and Quit.


USING IT
--------
  Ctrl + Shift + Space   Start / stop dictation. Focus any text field, press it,
                         speak, press again — your text is pasted in.

  Ctrl + Shift + A       Assist: highlight some text, press it, ask a spoken
                         question about it (optional; needs an AI provider set
                         up in Settings).

You can change these shortcuts, the microphone, and paste behavior from the
Settings window (via the tray icon).


REQUIREMENTS
------------
- Windows 10 (version 1803+) or Windows 11, 64-bit.
- Microsoft Edge WebView2 Runtime. It's already on virtually all Windows 10/11
  machines. If FlowOSS won't open its window, install it (free) from:
      https://developer.microsoft.com/microsoft-edge/webview2/
- A microphone.


"WINDOWS PROTECTED YOUR PC" WARNING
-----------------------------------
Because this build isn't code-signed, SmartScreen may warn on first launch.
Click "More info", then "Run anyway". (This is expected for indie apps.)


PRIVACY
-------
Transcription happens entirely on your device. Nothing is sent anywhere unless
you explicitly configure and use Assist mode with an AI provider.


TROUBLESHOOTING
---------------
- Models won't download: check your internet connection and that the folder is
  writable, then relaunch. Models are stored under
  %APPDATA%\flowoss\models
- No text pasted: make sure a text field is focused before you stop dictation.
- Change or free up a conflicting hotkey in Settings.

Licensed under MIT OR Apache-2.0. Speech models: NVIDIA Parakeet (CC-BY-4.0),
Silero VAD. Built on sherpa-onnx.
