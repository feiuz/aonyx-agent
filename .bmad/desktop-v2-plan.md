# Plan — Desktop v2 : refonte UI + i18n + auth via aonyx-account

**Projet** : Aonyx Agent (`desktop/`) + aonyx-account (repo séparé)
**Date** : 2026-06-04 · **Statut** : décisions OQ1-OQ4 actées — prêt pour Phase 0
**ADR liés (à créer)** : ADR-010 (frontend), ADR-011 (auth), ADR-012 (i18n), ADR-013 (enregistrement app)
**Choix actés (2026-06-04)** : front = **vanilla + golden-layout** (vrai drag-dock, zéro réécriture framework) · auth = **optionnelle/offline-first** · licence = **FREE/PREMIUM** (mirror imvu-toolkit).

---

## 1. Problème

La dispo actuelle du desktop ne plaît pas. Concrètement :
- Layout **figé** : topbar + sidebar gauche (Sessions + Memory) + colonne chat. Rien n'est déplaçable.
- **Mono-langue** (anglais en dur dans `index.html`/`app.js`), pas de détection.
- Pas d'**historique de conversations** lisible, pas de **session utilisateur**, pas de zone **mises à jour** visible (l'updater est planqué dans Settings).
- Aucune **authentification** : or le desktop doit dépendre d'aonyx-account (compte, licence, sync multi-device, préférences).

## 2. État vérifié (faits, pas hypothèses)

**Desktop** — Tauri 2, **frontend vanilla JS** (`desktop/src/{index.html,app.js,styles.css}`, `frontendDist:"../src"`, `withGlobalTauri`). Backend `desktop/src-tauri/src/lib.rs` : commandes `api_*` (proxy vers `aonyx serve api`), `check_for_update`/`install_update`, `list_models` (fetch live). Layout = `header.topbar` + `#settings` + `#wizard` + `.workspace(.sidebar, .chatcol)`.

**aonyx-account** — cloné en local : **`H:\Web\Htdocs\aonyx-account`** (v0.1.0). Next.js 14 + Express + Prisma + MariaDB (LXC 105), prod `account.aonyx.fr`. = **Universal API Gateway JWT**.
- **Registre d'apps** : `server/config/apps.config.ts` — interface `AonyxApp { id, name, type:'web'|'electron'|'mobile', origins[], routePrefix?, licensingEnabled, defaultTier, freeMaxDevices, description, plans?, features? }`. Ajouter une app = ajouter une entrée. Précédent **`imvu-toolkit`** (type `electron`, `licensingEnabled:true`, FREE/PREMIUM). CORS : `app://`/`file://`/no-origin **toujours acceptés** (le middleware auth protège).
- **Auth standard** : `POST /api/v1/auth/{register,login,refresh,logout}`, `GET /api/v1/auth/profile`, `POST /api/v1/auth/mfa/verify`, forgot/reset/verify-email. `authMiddleware` (JWT jose) + rate-limiters.
- **🎯 Device-code grant** (déjà implémenté) : `POST /api/v1/auth/device/code` (demande), `POST /api/v1/auth/device/token` (poll), `GET /lookup`, `POST /approve`+`/deny` (authentifiés côté web). = le grant OAuth standard pour clients desktop/CLI (cf. `gh auth login`).
- **Modèles Prisma utiles** : `User`, `UserProfile(language, subscription)`, `Session(deviceFingerprint, deviceInfo, ipAddress, expiresAt)`, `License(licenseKey, tier)`, MFA/WebAuthn.

## 3. Tension structurante à acter — offline-first

L'agent desktop est **local-first / offline** (agent embarqué `aonyx serve api`, palais local, RAG local). **L'auth NE DOIT PAS bloquer l'usage hors-ligne.**
→ **Décision proposée : auth optionnelle.** L'app marche pleinement sans compte (agent local). Se connecter **débloque** : licence/tier, sync préférences, langue depuis `UserProfile.language`, multi-device, features cloud futures. (ADR-011)

## 4. Objectifs / Non-objectifs

**Objectifs**
1. Layout réarrangeable façon Claude Code (sidebar/historique déplaçables + redimensionnables, état persistant).
2. Sidebar gauche = historique des conversations + **widget session utilisateur en bas** + **zone mises à jour au-dessus du widget**.
3. i18n **FR/EN** avec **détection automatique** de la langue.
4. Auth optionnelle via aonyx-account (device-code grant), token en keyring OS.
5. aonyx-account : `aonyx-agent` enregistré comme app de 1ʳᵉ classe.

**Non-objectifs (v2)**
- MFA/WebAuthn *dans* le client desktop (géré côté web pendant l'approbation device-code — gratuit).
- Sync temps réel des conversations vers le cloud (juste préférences/profil au départ).
- Billing/Stripe dans le desktop (renvoi web si licence payante un jour).
- Refonte du backend agent Rust (hors périmètre — c'est de l'UI desktop + auth).

## 5. Décisions (ADR à acter)

### ADR-010 — Stack frontend desktop — **ACTÉ : vanilla + golden-layout**
On garde le **frontend vanilla JS** (pas de migration React/Svelte) et on ajoute **golden-layout** (lib de docking agnostique) pour le vrai drag-dock : panneaux qu'on glisse, empile, redimensionne, ferme — disposition persistée. Rationale : golden-layout fournit le « déplaçable comme Claude Code » sans réécrire l'app ; churn moyen, pas de perte de l'existant. Conséquence à surveiller : l'état applicatif (auth, historique, i18n, widgets) reste en vanilla → discipline d'un petit module d'état maison (store léger) pour ne pas se noyer. Alternatives écartées : React+dockview (réécriture trop lourde pour le gain), vanilla+split.js (pas de vrai drag-dock).

### ADR-011 — Auth desktop via **device-code grant**, optionnelle
Flux : desktop `POST /auth/device/code` → ouvre le navigateur sur `account.aonyx.fr` (user code) → user approuve (web, MFA/WebAuthn possibles) → desktop `POST /auth/device/token` (poll) → JWT (access+refresh). **Tokens en keyring OS** (`keyring` crate côté Rust), refresh auto via `/auth/refresh`. Implémenté en **commandes Tauri Rust** (`account_*`) — server-to-server, pas de souci CORS. Auth **non bloquante** (offline-first, §3).

### ADR-012 — i18n FR/EN + détection
Au 1ᵉʳ run : détecter la langue OS (`navigator.language` côté webview, ou locale via Tauri). Si connecté : `UserProfile.language` **fait foi** (et un changement dans le desktop pousse vers le compte). Fichiers de messages `fr.json`/`en.json`, fallback EN. Choix manuel dans Settings.

### ADR-013 — `aonyx-agent` enregistré dans aonyx-account — **licence FREE/PREMIUM**
Entrée dans `apps.config.ts` (mirror `imvu-toolkit`) : `id:'aonyx-agent'`, `type:'electron'`, `origins:[]`, `routePrefix:'agent'`, `licensingEnabled:true`, `defaultTier:'FREE'`, `freeMaxDevices` + `plans`/`features` (FREE vs PREMIUM). Le code agent reste OSS/MIT ; la **licence gate des features cloud/premium** (sync, multi-device, quotas…), pas le binaire local. → workstream D3 **in-scope** (routes `/api/v1/agent/*` + service licence). Reste à définir le découpage exact des features FREE/PREMIUM (OQ4-bis).

## 6. Workstreams → Épics → Stories (effort S/M/L)

### A. Refonte UI — layout déplaçable
- **A1** Intégrer **golden-layout** + petit store d'état vanilla (pas de migration framework) — **M**
- **A2** Layout dockable : panneaux sidebar / historique / chat déplaçables + redimensionnables ; **persistance** de la disposition (localStorage/Tauri store) — **L**
- **A3** Sidebar : liste **historique conversations** (clic = ouvre, rename/suppr) — **M**
- **A4** Bas de sidebar : **widget session utilisateur** (avatar/email/tier ou « Se connecter ») — **S**
- **A5** Au-dessus du widget : **zone Mises à jour** (réutilise `check_for_update`/`install_update`, badge si dispo) — **S**
- **A6** Thème/polish (cohérence Saira, dark, responsive) — **S**

### B. i18n FR/EN
- **B1** Infra i18n (loader `fr/en.json`, helper `t()`, détection langue, switch Settings) — **M**
- **B2** Extraction de toutes les strings UI → FR/EN — **M**

### C. Auth desktop (device-code)
- **C1** Commandes Rust `account_device_start` / `account_device_poll` / `account_refresh` / `account_logout` / `account_me` + stockage **keyring** — **M**
- **C2** UI flux device-code (modale : « ouvre le navigateur, code XXXX », polling, succès/erreur) — **M**
- **C3** État auth global non bloquant + injection `Authorization: Bearer` + `X-App-Id: aonyx-agent` + `X-Device-Id` — **S**
- **C4** Sync `UserProfile.language` ↔ i18n (dépend B1) — **S**

### D. aonyx-account (`H:\Web\Htdocs\aonyx-account`)
- **D1** Entrée `aonyx-agent` dans `apps.config.ts` (+ origins/routePrefix) — **S**
- **D2** Vérifier device-code grant E2E pour ce client (contrat req/resp, expiration, scopes) ; ajuster `auth.service` si besoin — **M**
- **D3** Routes `/api/v1/agent/*` + service licence FREE/PREMIUM (mirror toolkit) — **M**
- **D4** Page web d'approbation device-code stylée Aonyx Agent (si pas générique) — **S**

## 7. Séquencement (phases)

- **Phase 0 — Débloquer** : ADR-010 tranché · D1 (register app) · D2 (valider device-code).
- **Phase 1 — La refonte visible** : A1→A6 + B1 (la dispo nouvelle + i18n infra). *Livrable : nouvelle UI déplaçable, FR/EN détecté, widgets en place (auth en « Se connecter » inactif).*
- **Phase 2 — Auth + licence branchées** : C1→C4 + D3 (licence FREE/PREMIUM) + B2 (strings complètes). *Livrable : login device-code, widget user vivant (email/tier), gating premium, langue synchronisée au compte.*
- **Phase 3 — Plus tard** : D4 (page d'approbation stylée), MFA-aware, sync conversations cloud, quotas premium fins.

## 8. Tests / Acceptation (extraits)
- Layout : je déplace l'historique à droite, je redémarre → la dispo est conservée.
- i18n : OS en FR → l'app s'ouvre en FR sans config ; switch EN → tout bascule ; reconnexion compte FR → reste FR.
- Auth : « Se connecter » → navigateur s'ouvre, code affiché ; j'approuve sur le web → le widget montre mon email/tier ; je relance l'app → toujours connecté (refresh) ; **avion mode/offline → l'agent local marche, widget = hors-ligne, zéro blocage**.
- Updates : version mockée plus récente → badge au-dessus du widget ; « Installer » relance l'app.

## 9. Open questions (bloquantes ⛔ / non ⚪)

- **OQ1 ✅ ACTÉ** Stack frontend = **vanilla + golden-layout** (drag-dock sans réécriture framework).
- **OQ2 ✅ ACTÉ** « Déplaçable » = **vrai drag-dock** (panneaux qu'on glisse/empile), fourni par golden-layout.
- **OQ3 ✅ ACTÉ** Auth = **optionnelle / offline-first**.
- **OQ4 ✅ ACTÉ** Licence = **tiers FREE/PREMIUM** (mirror imvu-toolkit).
- **OQ4-bis ⚪** Découpage précis des features FREE vs PREMIUM de l'agent (à définir avant D3).
- **OQ5 ⚪** Cible auth : prod `account.aonyx.fr` direct, ou instance dev/staging d'abord ?

## 10. Dépendances / risques
- aonyx-account local OK (`H:\Web\Htdocs\aonyx-account`) — mais c'est un **autre repo/CI/déploiement** (commits + push GitLab séparés).
- `keyring` (Rust) : back-ends OS (Windows Credential Manager OK ; Linux → Secret Service requis).
- Migration framework (si ADR-010 A/B) = on perd l'app vanilla qui marche → faire en branche, garder un fallback.
- Le device-code grant existe mais **non encore validé avec un vrai client** → D2 est un vrai jalon, pas une formalité.
