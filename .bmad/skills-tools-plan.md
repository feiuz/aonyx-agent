# Plan — Outils activables + Catalogue de skills façon Hermes

> Objectif (Damien) : (1) pouvoir activer/désactiver les **outils** dans la vue
> Compétences & outils ; (2) **répliquer le catalogue de skills d'Hermes Agent**
> sur Aonyx. Recherche faite 2026-06-21.

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

## Séquencement proposé

1. **A** — toggle outils (quick win, visible tout de suite).
2. **B1** — schéma superset (débloque tout le reste).
3. **B2** — vue Skills : catégories + recherche + toggle.
4. **B3** — seed starter set.
5. **B4** — import/marketplace (plus tard).

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
