# Signatur (Ops-Runbook)

[release.yml](workflows/release.yml) signiert das Windows-Release mit Azure Artifact Signing und das macOS-Release mit der Apple Developer ID samt Notarisierung; Zugriff nur über das Environment `release`.

## Läufe

- Push auf `main` mit neuer Version: CI auf demselben Commit, dann `build-windows` (kompiliert ohne Secrets, `tauri build --no-bundle`) und `sign-windows` (Environment `release`, bündelt mit `tauri bundle` und signiert). Vorhandene Releases werden nie ersetzt.
- Tauri signiert über `bundle.windows.signCommand` App-EXE, NSIS-Plugins, Uninstaller und Setup und erzeugt die Updater-Signatur (`.sig`) erst danach über das signierte Setup. Das `signCommand` steht nur in der `--config`-Datei des Release-Laufs, lokale Builds und Testbuilds bleiben unsigniert.
- Signiert wird mit SignTool und Microsofts Artifact-Signing-Client (dlib), beide als NuGet-Paket mit fester Version und SHA256 in `release.yml`. Die Anmeldung läuft per OIDC über `azure/login`; die dlib nutzt nur die Azure-CLI-Anmeldung.
- Der Lauf prüft Setup und die darin enthaltene `tippit.exe` mit `Get-AuthenticodeSignature`: gültig, Zeitstempel vorhanden, Subject gleich `AZURE_ARTIFACT_SIGNING_PUBLISHER_SUBJECT`.
- macOS folgt derselben Trennung: `build-macos` kompiliert ohne Secrets, `bundle-macos` (Environment `release`) bündelt die per Prüfsumme geprüfte Binärdatei, signiert und notarisiert, siehe [macOS](#macos). In beiden Bündel-Jobs läuft `bun install` ohne Installationsskripte.
- Testbuilds (`build.yml`, Label `build` oder Handstart) nutzen das Environment nicht und sind unsigniert (macOS: ad hoc).

## Azure-Einrichtung

| Baustein | Wert |
| --- | --- |
| Artefaktsignatur-Konto | `innov8itcodesigning` (Ressourcengruppe `rg-artifact-signing`, North Europe) |
| Konto-URI / Endpoint | `https://neu.codesigning.azure.net` |
| Zertifikatprofil | `monitoring` (Öffentliches Vertrauen) |
| Zertifikat-Subject | `CN=innov8-IT, O=innov8-IT, L=Mönchengladbach, S=North Rhine-Westphalia, C=DE` |
| Rollenzuweisung | **Artifact Signing Certificate Profile Signer** für die App-Registrierung aus `AZURE_CLIENT_ID` |
| Federated Credential | GitHub Actions, Entitätstyp **Umgebung**, Umgebung `release` |

Subject des Federated Credential, beide Formate dürfen parallel bestehen:

- klassisch: `repo:Xuoon/TippIT:environment:release`
- immutable: `repo:Xuoon@54535650/TippIT@1305693040:environment:release` (Owner-ID `54535650`, Repo-ID `1305693040`; der Vergleich ist exakt, großes X)

Ein eigenes Profil statt `monitoring` braucht nur ein neues Zertifikatprofil im Konto, die Rolle darauf und die Variable `AZURE_ARTIFACT_SIGNING_PROFILE`.

## Environment `release`

Deployment branches: nur `main`. Das Repo ist öffentlich, „Required reviewers“ ist damit verfügbar; weil `bundle-macos` und `sign-windows` zu verschiedenen Zeiten starten, sind das ein bis zwei Freigaben je Release.

**Variables:** `AZURE_ARTIFACT_SIGNING_ENDPOINT`, `AZURE_ARTIFACT_SIGNING_ACCOUNT`,
`AZURE_ARTIFACT_SIGNING_PROFILE`, `AZURE_ARTIFACT_SIGNING_PUBLISHER_SUBJECT`.

**Secrets:** `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID` (kein
Client-Secret, OIDC), `TAURI_SIGNING_PRIVATE_KEY` (Updater, auch für macOS) und
die sechs Apple-Secrets aus [macOS](#macos).
Fehlt ein Wert, bricht `sign-windows` bzw. `bundle-macos` im ersten Schritt ab.

## Störungen

**Azure-Login scheitert** („no matching federated identity record“): Das Subject
passt nicht. Das jeweils andere Format aus der Liste oben als zweites Credential anlegen.

**403 beim Signieren:** Endpoint passt nicht zur Region des Kontos, oder die
Rolle fehlt auf Konto bzw. Profil.

**Signaturprüfung fehlgeschlagen:** Subject in `AZURE_ARTIFACT_SIGNING_PUBLISHER_SUBJECT`
weicht vom Zertifikat ab, oder der Zeitstempel fehlt.

**Release nachholen:** Im gescheiterten Lauf „Re-run failed jobs“. Auch jeder
weitere Push auf `main` baut das Release, solange es die Version noch nicht gibt.

## Fristen

Die **3-Tage-Zertifikate** von Artifact Signing sind Normalbetrieb: Jeder Lauf
holt ein frisches, und der RFC-3161-Zeitstempel (`http://timestamp.acs.microsoft.com`)
hält ausgelieferte Setups auch nach Ablauf gültig.

Echter Termin ist die **Identitätsüberprüfung der Organisation** (`innov8-IT`),
gültig bis **19.11.2028**. Läuft sie aus, gibt es keine neuen Zertifikate und
`sign-windows` scheitert. Verlängert wird sie im Signing-Konto unter „Identitätsüberprüfungen“.

## macOS

Signiert wird mit **Developer ID Application: Sven Labitzki (955U4N2536)**,
dem Einzelperson-Account von Sven. Das Zertifikat gilt teamweit und bis
**07.12.2027**; es ist dasselbe wie in `Xuoon/boats`. Apple erlaubt höchstens
fünf Developer-ID-Zertifikate, deshalb kein eigenes je App anlegen.
Notarisiert wird mit einem App-Store-Connect-Teamschlüssel nur für dieses Repo
(`tippit-notarize`, Rolle Entwickler), damit er sich ohne Folgen für andere
Apps widerrufen lässt. Zertifikat (.p12 mit Passwort) und Schlüssel (.p8) liegen
nur in Bitwarden.

**Secrets im Environment `release`** (Namen wie in `Xuoon/boats`, die
Zuordnung auf Tauris Variablen macht `bundle-macos`):

| Secret | Inhalt | Tauri-Variable |
| --- | --- | --- |
| `CSC_LINK` | .p12 als Base64 | `APPLE_CERTIFICATE` |
| `CSC_KEY_PASSWORD` | Passwort der .p12 | `APPLE_CERTIFICATE_PASSWORD` |
| `APPLE_API_KEY` | Inhalt der .p8 | Datei, Pfad in `APPLE_API_KEY_PATH` |
| `APPLE_API_KEY_ID` | Schlüssel-ID | `APPLE_API_KEY` |
| `APPLE_API_ISSUER` | Issuer-ID | `APPLE_API_ISSUER` |
| `APPLE_TEAM_ID` | `955U4N2536` | `APPLE_SIGNING_IDENTITY` (Tauri prüft, dass das Zertifikat dazu passt) |

**Ablauf in `bundle-macos`:** `tauri bundle` importiert das Zertifikat in einen
temporären Schlüsselbund, signiert App und DMG mit Hardened Runtime, notarisiert
die App per `notarytool`, heftet das Ticket an und packt erst danach
`*.app.tar.gz` samt Updater-Signatur. Danach notarisiert der Job das DMG und
heftet auch dort das Ticket an. Der letzte Schritt prüft App, App aus dem
Update-Tarball und DMG: Developer-ID-Authority, Team-ID, Hardened Runtime,
sicherer Zeitstempel, `stapler validate` und, wenn Gatekeeper auf dem Runner an
ist, `spctl` mit „Notarized Developer ID“.

**Entitlements:** keine. TippIT braucht unter Hardened Runtime keine Ausnahme:
kein JIT (WebKit rechnet im eigenen Prozess), keine nachgeladenen Bibliotheken,
Bedienungshilfen und CGEventTap laufen über TCC. Die ad-hoc-Releases bis 2.3.0
liefen bereits mit Hardened Runtime ohne Entitlements.

**Lokal und in Testbuilds** bleibt `signingIdentity` aus `tauri.macos.conf.json`
(`-`, ad hoc); die Developer ID kommt nur über `APPLE_SIGNING_IDENTITY` im
Release-Lauf.

### Übergang von ad hoc

- Updates: Der Tauri-Updater prüft nur die minisign-Signatur aus `latest.json`
  und tauscht das Bundle per Rename, eine Code-Signatur vergleicht er nicht.
  Ad-hoc-Installationen bekommen das erste signierte Update deshalb normal.
- Bedienungshilfen: Der alte Eintrag gilt für den ad-hoc-Hash und greift nach
  dem Update nicht mehr. TippIT öffnet beim Start die Berechtigungen; „Freigeben“
  entfernt den alten Eintrag und fragt neu. Ab dann übersteht die Freigabe Updates.
- Zwischenablage-Freigabe (ab macOS 15.4) kann ebenso einmal neu gefragt werden.
- Keychain: TippIT legt keine Einträge an (`key.bin` ist eine Datei), es gibt
  nichts, das nach dem Wechsel unlesbar würde. Historie und Einstellungen bleiben.

### Prüfen nach einem Release

```bash
codesign -dv --verbose=4 /Applications/TippIT.app
spctl -a -vvv /Applications/TippIT.app
xcrun stapler validate /Applications/TippIT.app
```

Erwartet: `Authority=Developer ID Application: Sven Labitzki (955U4N2536)`,
`TeamIdentifier=955U4N2536`, `flags=0x10000(runtime)`, eine `Timestamp=`-Zeile;
`spctl` meldet `accepted` und `source=Notarized Developer ID`; `stapler` meldet
`The validate action worked!`.

### Störungen (macOS)

**„certificate … does not match provided identity“:** `CSC_LINK` gehört nicht
zur Team-ID in `APPLE_TEAM_ID`.

**`security import` scheitert:** `CSC_KEY_PASSWORD` passt nicht zur .p12, oder
`CSC_LINK` ist nicht die Base64-Form der Datei.

**Notarisierung „Invalid“:** Der Job gibt das `notarytool`-Log aus; meist fehlt
Hardened Runtime oder der Zeitstempel an einer Binärdatei.

**401 von `notarytool`:** Schlüssel widerrufen, oder ID, Issuer und .p8 gehören
nicht zusammen.

### Fristen (macOS)

Das Developer-ID-Zertifikat läuft am **07.12.2027** ab. Ausgelieferte Builds
bleiben dank Zeitstempel und Notarisierung gültig, neue Releases scheitern.
Ein neues Developer-ID-Zertifikat desselben Accounts braucht nur neue Werte in `CSC_LINK` und `CSC_KEY_PASSWORD`, hier
und in `Xuoon/boats`; die Bedienungshilfen-Freigabe bleibt, weil sie an Team und
Bundle-ID hängt. Dazu die Apple-Developer-Mitgliedschaft jährlich verlängern.
