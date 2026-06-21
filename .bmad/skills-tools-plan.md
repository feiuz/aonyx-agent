# Plan — Parité Hermes : Outils · Skills · Messaging · RAG local

> Objectif (Damien) : rapprocher Aonyx Agent de Hermes sur 4 axes — (A) activer/
> désactiver les **outils**, (B) **catalogue de skills**, (C) **messaging multi-
> plateforme**, (D) **RAG local dans le menu** (projets, consultation, ingestion).
> Recherche faite 2026-06-21.

## Contexte

**Hermes Agent** (NousResearch, MIT, self-improving) expose **75 skills built-in /
22 catégories** + ~78 communautaires (repo `awesome-hermes-skills`), au format
**SKILL.md** — un *standard ouvert* cross-compatible (Claude Code, Cursor, Codex).

**Aonyx a déjà le moteur** (`crates/aonyx-skills`) : parse SKILL.md, active par
`trigger` (keywords / always_on / manual), auto-mine des skills récurrents, et le
runner porte un set `disabled_skills` live. **Mais 0 skill installé** et un schéma
de frontmatter différent.

Diff de format (même fichier, schémas distincts) :

| | Hermes | Aonyx |
|---|---|---|
| Frontmatter | `name, description, version, author, license, platforms, metadata.hermes.{tags, related_skills}` | `name, description, trigger.{keywords, manual, always_on}, project_matches` |
| Body | markdown + syntaxe `terminal()` + tableaux | markdown libre |

→ **Le body (les instructions) est portable ; le frontmatter doit être mappé.**

## A. Outils — activer / désactiver  *(petit, autonome)*

La policy existe déjà : `config.tools_allow` / `config.tools_deny` + `apply_tool_policy`
(appliqués au build du registre). Le registre a un disabled-set (`ToolRegistry::subset`).

- **Backend** — endpoint `POST /v1/tools/{name}/enabled {enabled}` qui flippe le
  disabled-set à chaud **et** persiste dans `tools_deny` (hot, pas de restart).
  Repli V1 si trop lourd : écriture `tools_deny` + prise en compte au prochain lancement.
- **Desktop** — toggle par outil dans la vue Compétences & outils → commande
  `set_tool_enabled(name, on)`. Les outils restent **tous ON par défaut**.
- **Effort** : S (live) / XS (config + restart).

## B. Skills — catalogue façon Hermes

**Décision pivot** : rendre Aonyx **compatible avec le standard ouvert** plutôt que
réécrire 75 skills à la main → on peut alors *consommer* les skills Hermes/communautaires.

- **B1 — Schéma SKILL.md superset (compat Hermes).** Étendre le parseur `aonyx_skills`
  pour accepter (optionnels) `version / author / license / platforms / metadata.tags`,
  et **dériver le `trigger.keywords`** depuis `tags` + `description` quand `trigger`
  est absent. Les skills Hermes deviennent chargeables quasi tels quels. *Effort M.*
- **B2 — Vue Skills (parité Hermes).** La vue « Compétences » existe (aujourd'hui vide).
  Ajouter : **onglets catégories**, **recherche**, **toggle on/off par skill** (persisté
  via `disabled_skills`). *Effort M.*
- **B3 — Seed d'un starter set** (~12 skills universels, body porté d'Hermes, frontmatter
  adapté, rangés par catégorie) :
  - *software-development* : `plan`, `systematic-debugging`, `test-driven-development`,
    `simplify-code`, `requesting-code-review`, `spike`
  - *github* : `github-pr-workflow`, `github-code-review`, `github-issues`
  - *creative* : `architecture-diagram`, `sketch`
  - *research* : `arxiv`
  → installés dans `~/.aonyx/skills/<catégorie>/<skill>/SKILL.md`. *Effort M-L.*
- **B4 — Import / marketplace** *(différé)* : installer un skill depuis un repo/URL
  (`awesome-hermes-skills`) vers `~/.aonyx/skills/`, avec validation. *Effort L.*

## C. Messaging — passerelle multi-plateforme  *(en partie déjà là)*

**Déjà pré-implémenté** : adaptateurs **Telegram** + **Discord** (`crates/aonyx-adapters`)
+ wizards CLI (`aonyx setup telegram|discord` : token en keyring, chats autorisés) +
`aonyx serve telegram|discord`. Plus le serveur **OpenAI-compat** et l'**API REST/WS**.
Hermes expose ~16 canaux ; Aonyx en a 2 (+ serveurs).

- **C1 — Vue Messaging desktop** (façon Hermes) : liste des canaux à gauche, config à
  droite (token, IDs autorisés, home channel), badges *Disabled / Needs setup / Running*,
  **Save** → écrit la config + keyring. *Effort M.*
- **C2 — Start/stop de la passerelle** : commande desktop qui lance/arrête `aonyx serve
  <channel>` en sidecar (comme le sidecar API), statut live. *Effort M.*
- **C3 — Canaux additionnels** *(roadmap)* : Slack, Matrix, WhatsApp, Signal… chacun un
  adaptateur `AgentHandler` + wizard, à prioriser selon besoin. *Effort L / canal.*

## D. RAG local dans le menu  *(moteur déjà là, UI à faire)*

**Déjà là** : moteur RAG (embeddings bge-m3, chunk store SQLite + vecteurs, RRF, tool
`rag_search`, `aonyx ingest <dossier>`) + vues desktop Memory Health / KG / Projets.
**Manque** : une vraie **vue projets → documents** façon la capture RAG (consulter +
ingérer depuis l'app — aujourd'hui l'ingestion est **CLI-only**, pas d'endpoint HTTP).

- **D1 — Endpoint d'ingestion** : `POST /v1/memory/ingest {project, text|path, kind}` →
  chunk + embed dans le palais (le pipeline existe, juste pas exposé en HTTP). *Effort M.*
- **D2 — Vue Projets enrichie** : par projet → stats (docs/chunks, dim, modèle), **liste
  des documents ingérés** (consulter / éditer / supprimer un chunk), onglets *Texte /
  Fichiers / Documents*. *Effort M-L.*
- **D3 — Ingestion d'instructions + fichiers** : coller du texte (instructions) ou déposer
  des fichiers → ingérés dans le projet courant ; réutilise l'upload `+` du chat. *Effort M.*
- **D4 — Consultation / recherche** : recherche RAG dans un projet depuis la vue (le tool
  `rag_search` + `/v1/memory/search` existent déjà). *Effort S.*

## Séquencement proposé

1. **A** — toggle outils (quick win, visible tout de suite).
2. **B1 → B2 → B3** — skills : schéma superset, vue catégories/recherche/toggle, starter set.
3. **D1 → D2/D3** — RAG : endpoint d'ingestion, puis vue projets/documents + ingestion.
4. **C1 → C2** — messaging : vue de config des canaux, puis start/stop de la passerelle.
5. **Différés** : B4 (marketplace skills), C3 (canaux additionnels).

## Risques / garde-fous

- Beaucoup de skills Hermes supposent des **CLIs externes** (`gh`, `himalaya`, macOS…).
  Cibler les **universels** (dev, github, research, creative) ; marquer les prérequis ;
  adapter au toolset Aonyx (`bash`, `fs_*`, `web_*`, `git_*`).
- **Ne pas tout porter** : 22 catégories, plusieurs très spécifiques (apple, smart-home,
  social-media). Commencer petit, élargir à la demande.
- **Licences MIT** côté Hermes → portage OK en citant l'auteur dans le frontmatter.
- La syntaxe `terminal()` des bodies Hermes → réécrire en instructions neutres ou
  laisser (l'agent Aonyx exécute via ses propres tools, pas un runtime `terminal()`).

## Sources
- NousResearch/hermes-agent (catalogue + format SKILL.md), awesome-hermes-skills.
