# Plan complet — Desktop v2 : refonte sur l'archi React d'aonyx-rag (Electron → Tauri)

**Projet** : Aonyx Agent (`desktop/`)
**Date** : 2026-06-04 · **Statut** : proposé (plan-first, à valider avant exécution)
**Remplace** : ADR-010 (vanilla + golden-layout) → **ADR-014** (adopter l'archi React d'aonyx-rag)
**Référence étudiée** : `H:\Web\RAG` (`electron/` + `frontend/`) — **clean-room** : on copie l'**organisation / les patterns / les conventions**, JAMAIS les données ni la logique RAG métier.

---

## 0. Pourquoi ce pivot

L'app desktop d'aonyx-rag (`aonyx-rag-desktop` v4.2.72) est une archi **mûre et éprouvée** : React 18 + Vite + Tailwind + react-router + react-query + lucide, organisée proprement (shell, vues = sections nav, design system, contexte auth, couche services, hooks). La capture envoyée (Dashboard/Chat/Projets/…/Paramètres) **est sa Sidebar**. Le desktop Aonyx Agent (vanilla JS + golden-layout) n'a pas cette structure → on calque l'organisation d'aonyx-rag sur Tauri. Le vanilla + golden-layout est **abandonné** : le rail de nav + react-router est la vraie réponse à « une sidebar comme ça ».

## 1. L'archi vérifiée d'aonyx-rag (ce qu'on reproduit)

**Stack renderer** (`frontend/`, Vite) : React 18 · react-router-dom (**HashRouter**) · @tanstack/react-query · axios · lucide-react · react-hot-toast · react-markdown+remark-gfm+syntax-highlighter · cytoscape (Knowledge Graph) · @headlessui/react · tailwindcss · vitest+playwright.

**Organisation `frontend/src/`** :
- `main.jsx` → `App.jsx` : `QueryClientProvider` → `AuthProvider` → `HashRouter` → `TitleBar` + `UpdateChecker` + `Toaster` + `Routes`. Routes = `/login` (public) + `/*` (`PrivateRoute` → `Dashboard` qui porte Sidebar + sous-routes + CommandPalette).
- `components/` : une vue par section nav (Dashboard, ChatInterface, ProjectsList/ProjectDetails, Stats, MemoryHealth, KGExplorer/KGGraphView, Users, Permissions, MCP, Settings/) + cross-cutting (Sidebar, TitleBar, Header, ThemeToggle, UpdateChecker, ConnectionLost, Login, OAuthConsent, Profile).
- `components/ui/` : design system (Badge, Card, Chip, Collapsible, CommandPalette, EmptyState, ErrorCard, FormField, Modal, PageHeader, StatCard…).
- `components/agent/` : AgentMessage, ToolCallBlock, SlashCommandPalette (rendu du chat agent).
- `components/Settings/{,sections,parts}` : Paramètres en sous-app (Overview + LLM/RAG/Embeddings/Interface/System + sections/ + parts/).
- `context/AuthContext.jsx` : JWT (login/logout/verify/refresh horaire), **mode dégradé** (user caché + timestamp, tolère backend down N h), decode exp JWT local.
- `hooks/` : react-query (useProjects, useProjectStats, useProjectFiles…).
- `services/` : services domaine (projectService, queryService, settingsService, userService).
- `config/` : `apiClient.js` (wrap axios global + interceptor Bearer), `api.js` (`apiUrl`, `isElectronMode`, base `/api` proxy Vite en dev), `version.js`.
- `utils/`, `assets/`.

**Sidebar** : tableau `navigation` `{name, href, icon(lucide), superAdminOnly?}` ; collapsible (w-64 ↔ w-16) ; barre active coral à gauche (« style Claude ») ; items role-gated ; footer = bouton update + toggle collapse + **LLM Health (3 ProfileBadge) + version**.

**Pont natif** (`electron/preload.js`) : `contextBridge.exposeInMainWorld('electronAPI', {...})` — window controls (minimize/maximize/close, frameless), getConfig/setConfig (electron-store), updates (check/download/install + progress), deep-link, platform, isElectron ; factory listener avec unsubscribe.

**Main** (`electron/main.js`) : fenêtre **frameless** (`frame:false`) + splash + tray + single-instance + deep-links `aonyx://` + electron-store + AutoUpdater ; dev charge `http://localhost:3000` (Vite), prod charge le build.

**Build** (`electron/package.json`) : dev = `concurrently(vite + electron wait-on)` ; build = `vite build` → `electron-builder` (nsis + portable) → update-version-json ; electron-updater + publish GitHub.

## 2. Mapping Electron → Tauri (ce qui diffère)

| aonyx-rag (Electron) | Aonyx Agent (Tauri) |
|---|---|
| `window.electronAPI` (preload contextBridge) | `window.__TAURI__.core.invoke` + plugins, encapsulé dans `services/bridge.js` |
| axios + `apiUrl('/api'…)` (HTTP direct renderer) | **commandes Rust** `api_*` (proxy vers `aonyx serve api`) + `account_*` (aonyx-account). Le renderer ne fait **jamais** d'HTTP direct |
| electron-store getConfig/setConfig | commandes Rust `read/save_provider_config` (+ tauri-plugin-store si besoin) |
| electron-updater + UpdateChecker | **tauri-plugin-updater** (`check_for_update`/`install_update` déjà livrés) |
| `frame:false` + TitleBar custom + window-min/max/close | `tauri.conf` `decorations:false` + window API (ou décorations natives en MVP) |
| deep-link `aonyx://` (argv/open-url) | tauri-plugin-deep-link (callback OAuth, Phase auth) |
| `main.js` BrowserWindow/tray/splash | `tauri.conf` windows + `lib.rs` (déjà là) ; tray/splash optionnels |
| token localStorage | **keyring OS** via commande Rust (desktop = plus sûr, ADR-011) |
| dev: vite :3000 + electron ; build electron-builder | `tauri.conf` `beforeDevCommand`/`devUrl`/`beforeBuildCommand`/`frontendDist` ; build = `cargo tauri build` |

## 3. Nouvelle arborescence `desktop/` (layout Tauri+Vite idiomatique)

```
desktop/
  package.json            # scripts vite: dev / build ; deps React/router/query/lucide/tailwind
  vite.config.js          # base './', server.port 1420, build.outDir dist
  tailwind.config.js  postcss.config.js
  index.html              # point d'entrée Vite (remonté à la racine)
  src/                     # renderer React (≈ frontend/src d'aonyx-rag)
    main.jsx  App.jsx  index.css
    components/  ui/  agent/  Settings/{,sections,parts}
    layout/{Sidebar.jsx, TitleBar.jsx, AppShell.jsx}
    context/{AuthContext.jsx, ThemeContext.jsx}
    hooks/  services/  config/  utils/  assets/
  src-tauri/               # backend Rust (≈ electron/) — les commandes SONT l'API
    src/lib.rs             # api_*, list_models, check/install_update (+ account_* Phase auth)
    tauri.conf.json        # beforeDevCommand "npm run dev", devUrl :1420,
                           # beforeBuildCommand "npm run build", frontendDist "../dist"
```

> Le `desktop/src` vanilla actuel (index.html/app.js/styles.css + vendor/golden-layout) est **remplacé**. Panneaux déplaçables = option future (`react-resizable-panels`) dans la vue Chat si désiré — non prioritaire.

## 4. Sections (routes) — calquées, adaptées à l'agent

`/` Dashboard · `/chat` Chat (stream agent, sessions, slash-commands, tool-blocks) · `/projects` Projets (mémoire/RAG par projet) · `/stats` Statistiques · `/memory-health` Memory Health · `/kg` Knowledge Graph (cytoscape) · `/users` Utilisateurs (admin) · `/permissions` Permissions (admin) · `/mcp` MCP (serveurs) · `/settings` Paramètres (provider/LLM/RAG/embeddings/interface/système).
**MVP** = Chat + Paramètres réels ; le reste = vues placeholder (`PageHeader` + `EmptyState`) remplies par phase.

## 5. Couche services (le cœur du mapping invoke)

- `config/bridge.js` : `const invoke = window.__TAURI__?.core?.invoke` + `isTauri()` + `safeInvoke()` (≈ `apiClient.js`).
- `services/agentService.js` : `info()`, `stream(session, content, onEvent)` (Channel Tauri), sessions CRUD, `memorySearch()`, `ragSearch()`, `listModels()`.
- `services/accountService.js` (Phase auth) : `deviceStart/poll`, `me`, `refresh`, `logout`.
- `services/configService.js` : `readProviderConfig`, `saveProviderConfig`.
- `services/updateService.js` : `check`, `install`.
- `hooks/` : react-query autour des services (`useInfo`, `useSessions`, `useModels`…).

## 6. Auth (mirror AuthContext, backend aonyx-account)

Même **forme** que `AuthContext.jsx` (useAuth, login/logout/verify/refresh, mode dégradé) MAIS :
- login = **device-code grant** (ADR-011) via `account_device_start/poll` (pas de username/password).
- token en **keyring OS** (commande Rust) ; refresh auto via `account_refresh`.
- **offline-first** (ADR-011) : `PrivateRoute` ne bloque PAS les sections locales (Chat, Memory, Settings marchent sans compte) ; il garde seulement les sections **compte** (Profil, licence, multi-device). Se connecter débloque cloud/licence/sync.
- widget user = `ProfileBadge` en bas du rail (email/tier ou « Se connecter ») ; **zone updates au-dessus** (mirror footer Sidebar).

## 7. `tauri.conf.json` + Rust (changements)

- `build.beforeDevCommand`: `"npm run dev"` (cwd desktop) · `build.devUrl`: `"http://localhost:1420"` · `build.beforeBuildCommand`: `"npm run build"` · `build.frontendDist`: `"../dist"`.
- `withGlobalTauri: true` (déjà). CSP reste `null` (OK pour Vite/HMR ; resserrer plus tard).
- (option) `app.windows[0].decorations: false` + TitleBar custom (sinon natif en MVP).
- Rust : commandes **inchangées** (le dur est fait) ; ajouter `account_*` (Phase auth) + (option) tauri-plugin-store / keyring / deep-link.

## 8. Phasage

- **P0 — Scaffold** : créer le projet Vite+React+Tailwind+router+react-query+lucide ; wirer `tauri.conf` (dev/build) ; shell minimal (`App` + `AuthProvider` stub + `HashRouter` + `AppShell`/`Sidebar`/`TitleBar`) ; rail nav + routes vides. **Critère** : `cargo tauri dev` ouvre la fenêtre avec le rail nav React.
- **P1 — Chat réel** : porter le streaming/sessions/markdown/tool-blocks en React (`components/agent/*` + `services/agentService` + hooks). **Critère** : parité avec l'app actuelle (chat fonctionnel).
- **P2 — Paramètres** : provider wizard → `Settings/sections` (LLM/RAG/Embeddings/Interface/Système) + **fetch live des modèles** (réutilise `list_models`).
- **P3 — Memory / KG / Projets / Stats** : vues réelles (cytoscape KG, recherche palais, stats).
- **P4 — Auth + licence** : `AuthContext` device-code + keyring + `PrivateRoute` compte + widget user (+ D3 licence FREE/PREMIUM côté aonyx-account).
- **P5 — Polish** : thème (ThemeToggle), **i18n FR/EN** (= B1, détection langue), CommandPalette, ConnectionLost, UI updater, TitleBar frameless.

## 9. Design system / thème

Tailwind config avec tokens Aonyx (réutiliser l'esthétique actuelle : fond noir, accent blanc, Saira) — palette `aonyx-*` / `primary-*` (mirror du `tailwind.config` d'aonyx-rag, valeurs recréées). `components/ui/` = primitives recréées (Badge, Card, Chip, Modal, EmptyState, PageHeader, StatCard, FormField, CommandPalette).

## 10. Risques / arbitrages

- Migration vanilla → React = **gros lift**, mais l'app est petite et **les commandes Rust restent** (le backend desktop est déjà fait).
- **HashRouter obligatoire** (origine `tauri://`, comme `file://` en Electron).
- **Clean-room strict** : on RECRÉE en s'inspirant de la structure/nommage d'aonyx-rag — **aucune copie** de code/donnée RAG.
- golden-layout **abandonné** ; panneaux déplaçables = option future (`react-resizable-panels`) dans Chat.
- Le travail vanilla en cours (rail nav non commité) sera écrasé par P0 — pas de valeur à le commiter.

## 11. Décisions P0 (actées 2026-06-04)

- **OQ-a ✅** TitleBar : **frameless + TitleBar custom** (window controls via window API Tauri), comme aonyx-rag.
- **OQ-b ✅** Thème : **clair + sombre** (ThemeToggle + variantes `dark:` Tailwind dès P0).
- **OQ-c ⚪ ouvert** Sections admin (Utilisateurs / Permissions) : pertinentes dans l'agent perso, ou réservées à aonyx-account web ? → MVP : placeholders, décision avant de les remplir.
- **OQ-d ✅** Renderer en **JSX** (aligné sur aonyx-rag).
- **OQ-e ✅** Migration sur **branche `desktop-v2`** ; `main` reste sur `902ea94` (golden-layout) comme fallback.
