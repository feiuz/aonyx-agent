# Plan — Système Cron (tâches planifiées) façon Hermes

> Demande (Damien) : ajouter un système cron comme Hermes Agent (le « Cron » de
> la bottom bar). Recherche faite 2026-06-22.

## Contexte

**Hermes** : scheduler built-in, **syntaxe cron + one-shot**. Surface modèle = un
**tool `cronjob`** à actions (`create / list / update / pause / resume / run /
remove`) → l'agent planifie lui-même. La gateway **tick toutes les 60 s**,
exécute les jobs dus dans des **sessions agent isolées** (avec injection de skills
+ delivery cross-plateforme), et propose un mode **`no_agent`** pour les jobs sans
LLM (watchdogs, alertes disque/mémoire, heartbeats).

**Aonyx** : **aucun scheduler** — l'agent tourne à la demande (chat / serve). Mais
le sidecar `aonyx serve api` est **déjà un process long-running** → hôte naturel
du tick. On a déjà : `AgentRunner` (sessions isolées via `run_subagent`), le store
SQLite (sessions.db / palace), l'approbation interactive, et les canaux messagerie
(Telegram / Discord) pour la delivery.

## Architecture proposée (miroir Hermes)

- **Store `cronjobs`** (SQLite, comme `sessions.db`) : `id, name, schedule`
  (`cron` expr **ou** `at` one-shot ISO), `prompt`, `project`, `agent` (sous-agent
  optionnel), `delivery` (`none|diary|telegram|discord`), `no_agent`, `enabled`,
  `last_run`, `next_run`, `last_result`, `created_at`.
- **Scheduler loop** : tâche `tokio` lancée dans `aonyx serve` → **tick 60 s** →
  jobs où `enabled && next_run <= now` → exécute chacun dans une **session agent
  isolée** (`AgentRunner`, comme `run_subagent`) → **delivery** → recalcule
  `next_run` (crate `cron` : `Schedule::after(now)`).
- **Tool `cronjob`** (built-in, model-facing) : les actions ci-dessus, adossées au
  store → « tous les matins, résume mes emails » fait créer un job par l'agent.
- **Mode `no_agent`** : job exécutant une **action fixe** (un tool) sans tour LLM
  — alertes / heartbeats.
- **Delivery** : V1 → **diary + une liste de notifications** que le desktop affiche ;
  cross-plateforme (Telegram / Discord) **quand la gateway tourne**.

## Phases

- **E1 — Store + parsing + scheduler loop.** `crate aonyx-cron` (ou dans
  `aonyx-memory`) : table `cronjobs`, crate `cron` pour le next-occurrence, tick
  60 s dans `serve`, exécution en session isolée, recalc `next_run`, écriture
  `last_run/last_result`. *Effort M-L.*
- **E2 — Tool `cronjob`.** `ToolHandler` built-in à actions, adossé au store (comme
  `dispatch_agent`) → l'agent crée / liste / met en pause les jobs. *Effort M.*
- **E3 — API CRUD + delivery.** `GET/POST/PATCH/DELETE /v1/cron`, `POST
  /v1/cron/:id/run|pause|resume` ; delivery diary + endpoint notifications. *Effort M.*
- **E4 — Vue Cron desktop.** CRUD (cron expr **ou** langage naturel délégué à
  l'agent), statut (prochain / dernier run, dernier résultat, pause), +
  **indicateur bottom-bar « Cron »** (nb actifs · prochain run). *Effort M.*
- **E5 — no_agent + langage naturel.** Jobs sans LLM + création NL (l'agent
  traduit en cron via le tool). *Effort S-M.*

## Séquencement

**E1** (le cœur) → **E2** (tool, débloque l'usage agentique) → **E3** (API) →
**E4** (vue + bottom bar) → **E5** (raffinements).

## Risques / garde-fous

- **Sécurité (critique).** Un job tourne **sans humain présent** mais peut appeler
  des tools destructifs. L'approbation interactive est inutilisable hors-ligne →
  les jobs cron tournent en **`DenyDestructive` par défaut**, ou avec une
  **allowlist explicite par job**. Jamais de `fs_write`/`bash`/`git push` auto
  sans opt-in clair et journalisé.
- **Récurrence runaway** : cap de fréquence minimale (p. ex. ≥ 1 min) + cap du
  nombre de jobs ; un one-shot se désactive après exécution.
- **Persistance du process** : le tick n'a lieu que si `aonyx serve` tourne (le
  sidecar desktop). App fermée = pas de tick → à documenter ; un vrai daemon /
  service système (autostart) = **hors V1**.
- **Fuseaux** : stocker en **UTC**, afficher en local.
- **Delivery** : sans gateway messagerie active, livrer en **diary + notifications**
  (la vue Cron + un badge).

## Surface du tool (E2)

```
cronjob {
  action: "create" | "list" | "update" | "pause" | "resume" | "run" | "remove",
  id?, name?, schedule?,            // "0 9 * * *" (cron) ou "2026-07-01T09:00:00Z" (one-shot)
  prompt?, project?, delivery?,     // none | diary | telegram | discord
  no_agent?                         // job sans tour LLM
}
```

## Sources
- Hermes cron : docs `user-guide/features/cron`, `developer-guide/cron-internals`,
  `cron/scheduler.py` (NousResearch/hermes-agent).
