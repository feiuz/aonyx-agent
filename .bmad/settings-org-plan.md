# Plan — Organisation des Paramètres façon Hermes

> Demande (Damien) : organiser le sous-menu Paramètres comme Hermes Agent
> (groupé, avec séparateurs). Capture de référence 2026-06-22.

## Structure cible (Hermes)

```
Préférences        Intégrations        Système
─────────────      ─────────────       ─────────
Model              Providers           About
Chat               Gateway
Appearance         Tools & Keys
Workspace          MCP
Safety             Archived Chats
Memory & Context
Voice
Advanced
Notifications
```

## Mapping vers Aonyx (existant → Hermes)

| Section Hermes | Aonyx | État |
|---|---|---|
| **Model** | dans `provider` (Fournisseur) | à **scinder** (Modèle ≠ Fournisseur) |
| **Chat** | — | **à créer** (réglages chat : placeholder, raccourcis, streaming) |
| **Appearance** | thème + langue (dans le sidebar) | **à créer** (déplacer ici : thème, langue, police) |
| **Workspace** | `projects` (Projets) | **renommer** Espace de travail (projet par défaut, dossiers) |
| **Safety** | `permissions` + politique d'approbation | **enrichir** (Deny/Ask/Allow, allowlist outils) |
| **Memory & Context** | `kg` + Mémoire (sidebar) | **regrouper** (palais, RAG, fenêtre contexte) |
| **Voice** | dictée (dans le composer) | **à créer** (langue STT, on/off) |
| **Advanced** | — | **à créer** (`max_iterations`, `max_dispatches`, auto-retrieve) |
| **Notifications** | — | **à créer** (à coupler au cron + updates) |
| **Providers** | `provider` (clés LLM) | **garder** |
| **Gateway** | `messaging` | **renommer** Passerelle |
| **Tools & Keys** | Skills & Tools (sidebar) + clés | **regrouper** (outils + clés API) |
| **MCP** | `mcp` | **garder** |
| **Archived Chats** | — | **à créer** (conversations archivées) |
| **About** | version (bottom bar) | **à créer** (version, liens, licence) |

**Sections Aonyx hors structure Hermes** (Dashboard / Statistiques / Utilisateurs) :
les déplacer dans un groupe **« Admin »** distinct (ou les sortir des Paramètres),
elles relèvent du compte/admin, pas des préférences.

## Phases

- **F0 — Regroupement de l'existant** *(rapide, frontend)*. Passer `SettingsHub`
  d'une liste plate à un **sous-menu groupé avec séparateurs** ; mapper les
  sections existantes aux libellés Hermes (Fournisseur→Providers, kg→Memory &
  Context, messaging→Gateway, projects→Workspace, permissions→Safety, mcp→MCP) ;
  ajouter **About** (version/liens). Admin (dashboard/stats/users) en 3ᵉ groupe.
  *Effort S.*
- **F1 — Apparence + Voix** : déplacer thème/langue/police (Appearance) et les
  réglages de dictée (Voice) depuis le sidebar/composer vers des sections dédiées.
  *Effort M.*
- **F2 — Advanced + Chat** : exposer `max_iterations`, `max_dispatches`,
  auto-retrieve, et les réglages chat (placeholder, raccourcis). *Effort M.*
- **F3 — Safety enrichi** : Deny / Ask / Allow + allowlist d'outils (s'appuie sur
  l'approbation interactive déjà livrée). *Effort M.*
- **F4 — Archived Chats + Notifications** : archive de conversations + centre de
  notifications (couplé au **cron** — voir `cron-plan.md`). *Effort M-L.*

## Séquencement

**F0** (regroupement visible tout de suite) → **F1** (Apparence/Voix) → **F2**
(Advanced/Chat) → **F3** (Safety) → **F4** (Archived/Notifications).

## Risques / notes

- Ne pas **dupliquer** les surfaces : thème/langue sont aujourd'hui dans le
  sidebar ; les déplacer vers Appearance, garder un raccourci.
- Certaines sections Hermes (Notifications, Archived) dépendent d'autres chantiers
  (cron, store de conversations) → les livrer après leurs prérequis.
- Garder l'i18n FR/EN pour chaque nouveau libellé.
