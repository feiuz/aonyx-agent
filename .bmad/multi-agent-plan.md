# Plan — Multi-agent : architecte + agents custom (Aonyx)

**Date** : 2026-06-21 · **ADR** : ADR-017 · **Statut** : décisions actées (LLM décide via `dispatch_agent` · vue Agents dédiée)

## Concept
L'agent du chat **= l'architecte**. Il gagne un outil built-in `dispatch_agent(task, agent?)` et **décide lui-même** (LLM) de déléguer à un agent custom selon la tâche (code, recherche, review…). Les agents custom sont des fichiers `~/.aonyx/agents/*.AGENT.md`. Chaque sous-agent tourne **isolé** (whitelist d'outils + modèle propres) mais **partage le palais de mémoire du projet** — le différenciateur Aonyx vs Hermes : la délégation ne perd pas le contexte.

## Format `AGENT.md`
Frontmatter YAML + corps markdown (miroir des `SKILL.md` + subagents Claude Code) :
```
---
name: coder
description: Tâches d'écriture ou de modification de code   # "quand l'utiliser"
model: claude-sonnet-4-6        # optionnel ; sinon hérite du parent
provider: claude-code           # optionnel
tools: [fs_read, fs_write, fs_edit, bash, git_*]   # whitelist (vide = hérite)
---
Tu es un développeur senior. Écris du code idiomatique…   # system prompt
```

## Réutilise (vérifié dans le code)
`ToolHandler`+`ToolRegistry` · le système de **skills** (triggers, injection, loader fichier — `aonyx-skills`) · `Config` · `Palace` clonable (mémoire partagée) · l'**approval gate** · le streaming `TurnEvent`.

## Ajoute
- `AgentDefinition` (struct, proche de `Skill` + `model`/`provider`/whitelist stricte).
- Loader `~/.aonyx/agents/*.AGENT.md` (parallèle à `load_all_skills`).
- `SubAgentRunner` (remplit le stub `subagent.rs`) : réutilise `AgentRunner`, registry filtré à la whitelist, modèle propre, **Palace partagé**, hérite l'approval policy ; renvoie message final + trace.
- Outil built-in `dispatch_agent` (dans `aonyx-tools`) : `{task, agent?}` → résout l'agent → spawn `SubAgentRunner` → renvoie résultat + trace ; classé « destructif » (passe l'approval gate sauf auto-approve).
- Desktop : commandes Tauri CRUD (`agents_list/read/save/delete`) sur `~/.aonyx/agents/` + **vue Agents** (liste + éditeur).
- Chat : rendu du bloc « → agent » repliable (stream + résultat du sous-agent).

## Phases (effort S/M/L)
- **MA1 — Cœur backend** (L) : `AgentDefinition` + loader · `SubAgentRunner` · outil `dispatch_agent` · `aonyx agents list/run` · presets built-in (architect/coder/reviewer/researcher, embarqués comme les skills). *Acc.* : en CLI/TUI, l'agent délègue et renvoie le résultat du sous-agent.
- **MA2 — Vue Agents (desktop)** (M) : CRUD (liste + éditeur du mockup : nom, quand-utiliser, modèle, outils, prompt) via commandes Tauri. Nav « Agents ». *Acc.* : créer/éditer un agent écrit un `.AGENT.md` relu par l'agent.
- **MA3 — Orchestration dans le chat** (M) : l'API `serve` relaie les `TurnEvent` du sous-agent ; le chat affiche un bloc « → agent » repliable (stream + résultat). *Acc.* : déléguer dans le chat montre le travail du sous-agent.
- **MA4 — Polish** (M) : garde-fous (profondeur max, budget de tours/tokens), triggers/auto-routing optionnels, dispatch parallèle, i18n, presets éditables.

## Séquencement
MA1 (testable en CLI/TUI, sans desktop) → MA2 (config) → MA3 (rendu chat) → MA4 (garde-fous). MA1 est le jalon dur (archi agent).

## Risques
- **Récursion/coût** : un sous-agent qui dispatch → profondeur max (ex. 1–2) + budget de tours ; off par défaut, opt-in.
- **Isolation outils** : la whitelist doit être **appliquée au dispatch** (registry filtré), pas seulement déclarative (les skills ne l'enforcent pas aujourd'hui).
- **Petit modèle** : peut sur/sous-déléguer → prompt d'architecte clair + presets bien décrits.
- **Mémoire partagée** : les sous-agents écrivent dans le même palais → cohérence/concurrence à surveiller.
