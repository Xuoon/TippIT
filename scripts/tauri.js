// Tauri lädt .env.local nicht selbst. Über diesen Bun-Launcher lädt Bun die
// Datei automatisch und reicht die Variablen (u. a. TAURI_SIGNING_PRIVATE_KEY
// für die Updater-Signatur) explizit an den Tauri-Prozess weiter.
//
// Ohne gesetztes Signatur-Passwort fragt Tauri beim Bundeln danach und bleibt
// ohne Terminal ewig stehen ("Decrypting updater signing key, expect a prompt
// for password"). Der Schlüssel dieses Projekts hat keins — deshalb wird ein
// leeres vorgegeben, sofern der Aufrufer nichts anderes setzt.
const env = { ...process.env };
env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= "";

const tauri = globalThis.Bun.spawn(
  ["bun", "x", "tauri", ...process.argv.slice(2)],
  {
    env,
    stderr: "inherit",
    stdin: "inherit",
    stdout: "inherit",
  }
);

process.exit(await tauri.exited);
