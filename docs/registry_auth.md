# Registry-Anmeldedaten / Registry Credentials

## TL;DR (Deutsch)
- Standard: `:registry add <host>` → `:registry set <host> auth basic` → `:registry set <host> user <name>` → Secret in OS-Keyring speichern → `:registry set <host> secret-keyring "<host>/<label>"`.
- Keyring hat Vorrang vor allen anderen Quellen; fällt bei Fehler auf gleichnamige ENV-Variable zurück; erst danach auf das (optionale) AGE-verschlüsselte `secret` im `registries.json`.
- Test: `:registry test <host>`; Warnungen erscheinen in `:messages`.

## Ablauf (Deutsch)
1. Registry anlegen oder auswählen  
   - `:registry add docker.io` (legt anonym an)  
   - Auswahl im Registries-View mit Enter.
2. Authentifizierung festlegen  
   - `:registry set docker.io auth basic` (oder `bearer-token`, `github-pat`, `anonymous`)
3. Benutzername setzen (falls benötigt)  
   - `:registry set docker.io user myuser`
4. Secret im OS-Keyring hinterlegen  
   - Außerhalb von containr: `keyring set containr "docker.io/basic"` → Passwort eingeben.  
   - Im UI referenzieren: `:registry set docker.io secret-keyring "docker.io/basic"`
5. Optionales Fallback per ENV (nur falls Keyring scheitert)  
   - `export docker.io/basic='mein-passwort'`
6. Letztes Mittel: verschlüsseltes Secret im File  
   - `secret` bleibt unterstützt (AGE, `age_identity`), aber wird erst genutzt, wenn Keyring+ENV fehlen.
7. Prüfen  
   - `:registry test docker.io` zeigt Erfolg/Misserfolg und meldet Warnungen.

### Hinweise (Deutsch)
- `secret-keyring` leer lassen, wenn nur anonym genutzt wird.  
- `secret` weglassen, wenn Keyring verwendet wird.  
- Bei Warnung „keyring read failed …“ wird automatisch der ENV-Fallback mit demselben Namen probiert.

---

## TL;DR (English)
- Workflow: `:registry add <host>` → `:registry set <host> auth basic` → `:registry set <host> user <name>` → store secret in OS keyring → `:registry set <host> secret-keyring "<host>/<label>"`.
- Keyring has highest priority; on failure it falls back to an environment variable of the same name, then to the optional AGE-encrypted `secret` in `registries.json`.
- Test via `:registry test <host>`; warnings appear in `:messages`.

## Steps (English)
1. Add/select registry  
   - `:registry add docker.io` (anonymous)  
   - Select in registries view with Enter.
2. Set auth type  
   - `:registry set docker.io auth basic` (or `bearer-token`, `github-pat`, `anonymous`)
3. Set username (if needed)  
   - `:registry set docker.io user myuser`
4. Store secret in OS keyring  
   - Outside containr: `keyring set containr "docker.io/basic"` → enter password.  
   - Reference in UI: `:registry set docker.io secret-keyring "docker.io/basic"`
5. Optional ENV fallback  
   - `export docker.io/basic='my-password'` (only used if keyring read fails)
6. Optional file-based secret (AGE)  
   - `secret` remains supported but is only used if keyring and ENV are absent.
7. Verify  
   - `:registry test docker.io` to check connectivity/auth; warnings in `:messages`.

### Notes (English)
- Leave `secret-keyring` empty for anonymous.  
- Omit `secret` when using keyring.  
- On “keyring read failed …” the ENV fallback with the same name is attempted automatically.
