# Plan — Catalogue d'agents pré-installés (agents-as-tools)

> Demande (Damien) : présenter les agents comme un **catalogue** (façon outils /
> skills), avec une **grande liste pré-installée** en plus des customs. 2026-06-22.

## Contexte

Aujourd'hui : 3 presets (`coder`, `reviewer`, `researcher`) via
`agents.rs::builtin()` + les agents custom (`~/.aonyx/agents/*.AGENT.md`). La vue
Agents montre juste « Nouvel agent » + la liste custom. L'architecte délègue via
le tool **`dispatch_agent`**, qui **énumère déjà tous les agents** (presets +
custom) — donc ajouter des presets suffit à les rendre délégables.

`AgentDefinition { id, name, description, model?, provider?, tools[], enabled,
max_iterations?, body }` existe. Les presets sont des `AGENT.md` const (comme les
skills builtin via `include_str!`).

## Architecture

- **G1 — Vue catalogue** : remplacer la liste plate par des **cartes par
  catégorie + recherche + toggle on/off** (comme `SkillsTools`), section
  **Pré-installés** (catalogue) + section **Mes agents** (custom) + « Nouvel agent ».
- **G2 — Seed** : ~34 agents builtin (`AGENT.md` + persona body), catégorisés.
  Étendre `AgentDefinition` avec `category` + `tags` (superset, comme les skills).
- **G3 — Toggle par agent** : activer/désactiver (le `dispatch_agent` schema ne
  liste alors que les activés → prompt plus court + sélection). Mirror du toggle
  skills/outils (set partagé). + cap de délégations déjà en place (MA4).

## La grande liste (~34 agents · 7 catégories)

### Engineering
| id | nom | rôle | outils clés |
|---|---|---|---|
| `coder` | Coder | écrit du code idiomatique, features, fixes | fs_*, bash, git_* |
| `reviewer` | Reviewer | relit diffs/PR, trouve bugs réels + sécu | fs_read, fs_grep, git_diff, git_show |
| `refactorer` | Refactorer | nettoie/simplifie sans changer le comportement | fs_read, fs_write, fs_edit, fs_grep |
| `debugger` | Debugger | root-cause en 4 phases puis corrige | fs_read, fs_grep, git_log, git_diff, bash |
| `tester` | Tester | écrit des tests (TDD RED-GREEN-REFACTOR) | fs_read, fs_write, fs_edit, bash |
| `architect` | Architect | conçoit la structure modules/système | fs_read, fs_grep, fs_glob, rag_search |
| `security-auditor` | Security Auditor | repère vulnérabilités, secrets, injections | fs_read, fs_grep, fs_glob, git_diff |
| `perf-optimizer` | Performance Optimizer | profile + optimise les points chauds | fs_read, fs_grep, bash |
| `migrator` | Migrator | porte/migre (framework, langage, version) | fs_*, git_* |
| `api-designer` | API Designer | conçoit REST/GraphQL, contrats, schémas | fs_read, fs_grep, fs_write |

### Research
| id | nom | rôle | outils clés |
|---|---|---|---|
| `researcher` | Researcher | enquête sujet/codebase, cite ses sources | web_search, web_fetch, rag_search, memory_search |
| `data-analyst` | Data Analyst | analyse données, stats, tendances | fs_read, fs_glob, bash, memory_search |
| `summarizer` | Summarizer | condense du contenu long en synthèse | fs_read, web_fetch |
| `fact-checker` | Fact Checker | vérifie des affirmations contre des sources | web_search, web_fetch, rag_search |
| `market-analyst` | Market Analyst | analyse concurrents / marché | web_search, web_fetch |

### Writing
| id | nom | rôle | outils clés |
|---|---|---|---|
| `doc-writer` | Documentation Writer | écrit/maj la doc technique | fs_read, fs_write, fs_edit, fs_glob |
| `technical-writer` | Technical Writer | guides, tutoriels, how-to | fs_read, fs_write, fs_edit, web_fetch |
| `copywriter` | Copywriter | copy marketing, landing, posts | fs_read, web_fetch |
| `editor` | Editor | relit, resserre, corrige la prose | fs_read, fs_edit |
| `changelog-writer` | Changelog Writer | notes de version depuis l'historique git | git_log, fs_read, fs_write |

### DevOps
| id | nom | rôle | outils clés |
|---|---|---|---|
| `devops` | DevOps | CI/CD, déploiement, infra-as-code | bash, fs_*, git_* |
| `incident-responder` | Incident Responder | triage incident, hypothèses, mitigation | bash, fs_read, fs_grep, git_log, memory_search |
| `dockerizer` | Dockerizer | conteneurise une app (Dockerfile, compose) | fs_read, fs_write, bash |
| `sre` | SRE | fiabilité, monitoring, post-mortems | bash, fs_read, memory_search |

### Product
| id | nom | rôle | outils clés |
|---|---|---|---|
| `planner` | Planner | écrit un plan actionnable (pas d'exécution) | fs_read, fs_grep, fs_glob, rag_search, fs_write |
| `product-manager` | Product Manager | PRD, specs, user stories | fs_read, web_search, fs_write |
| `estimator` | Estimator | estime l'effort (S/M/L), découpe | fs_read, fs_grep, fs_glob |
| `triager` | Triager | trie/labellise/priorise les issues | fs_read, fs_grep, memory_search |

### Data / ML
| id | nom | rôle | outils clés |
|---|---|---|---|
| `ml-engineer` | ML Engineer | entraînement/éval de modèles, pipelines | fs_read, fs_write, bash |
| `prompt-engineer` | Prompt Engineer | conçoit/optimise des prompts | fs_read, rag_search |
| `sql-expert` | SQL Expert | écrit/optimise du SQL, schémas | fs_read, bash |

### Cross-cutting
| id | nom | rôle | outils clés |
|---|---|---|---|
| `translator` | Translator | traduit en préservant le sens/ton | fs_read, fs_write |
| `accessibility-auditor` | Accessibility Auditor | audit a11y (WCAG) | fs_read, fs_grep, web_fetch |
| `i18n-specialist` | i18n Specialist | internationalisation, extraction de chaînes | fs_read, fs_grep, fs_write, fs_edit |
| `onboarding-buddy` | Onboarding Buddy | explique le codebase à un nouvel arrivant | fs_read, fs_grep, fs_glob, rag_search |

## Phases / séquencement

1. **G1** — vue catalogue (cartes, catégories, recherche, toggle) + section custom.
2. **G2** — étendre `AgentDefinition` (category/tags) + seed des ~34 `AGENT.md`.
3. **G3** — toggle par agent (set partagé, comme outils/skills) + n'exposer que les
   activés dans le schema `dispatch_agent`.

## Risques / garde-fous

- **Bruit pour le LLM** : `dispatch_agent` liste tous les agents dans sa description
  → prompt long si 34 agents. **Solution** : n'exposer que les **activés** (toggle),
  défaut = un sous-ensemble (les 8-10 cœur), le reste activable.
- **Sécurité** : sous-agents en **DenyDestructive** (déjà le cas) ; les écritures
  passent par l'approbation interactive.
- **Qualité des persona** : bodies concis mais réels (pas de coquilles vides) —
  porter l'esprit des presets existants.
