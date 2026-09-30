# Windows-Signatur (Ops-Runbook)

[release.yml](workflows/release.yml) signiert das Windows-Release mit Azure Artifact Signing; Zugriff nur über das Environment `release`.

## Läufe

- Push auf `main` mit neuer Version: CI auf demselben Commit, dann `build-windows` (kompiliert ohne Secrets, `tauri build --no-bundle`) und `sign-windows` (Environment `release`, bündelt mit `tauri bundle` und signiert). Vorhandene Releases werden nie ersetzt.
- Tauri signiert über `bundle.windows.signCommand` App-EXE, NSIS-Plugins, Uninstaller und Setup und erzeugt die Updater-Signatur (`.sig`) erst danach über das signierte Setup. Das `signCommand` steht nur in der `--config`-Datei des Release-Laufs, lokale Builds und Testbuilds bleiben unsigniert.
- Signiert wird mit SignTool und Microsofts Artifact-Signing-Client (dlib), beide als NuGet-Paket mit fester Version und SHA256 in `release.yml`. Die Anmeldung läuft per OIDC über `azure/login`; die dlib nutzt nur die Azure-CLI-Anmeldung.
- Der Lauf prüft Setup und die darin enthaltene `tippit.exe` mit `Get-AuthenticodeSignature`: gültig, Zeitstempel vorhanden, Subject gleich `AZURE_ARTIFACT_SIGNING_PUBLISHER_SUBJECT`.
- macOS bleibt ad-hoc signiert und folgt derselben Trennung: `build-macos` kompiliert ohne Secrets, `bundle-macos` (Environment `release`, nur für den Updater-Schlüssel) bündelt die per Prüfsumme geprüfte Binärdatei. In beiden Bündel-Jobs läuft `bun install` ohne Installationsskripte.
- Testbuilds (`build.yml`, Label `build` oder Handstart) nutzen das Environment nicht und sind unsigniert.

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
Client-Secret, OIDC) und `TAURI_SIGNING_PRIVATE_KEY` (Updater, auch für macOS).
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
