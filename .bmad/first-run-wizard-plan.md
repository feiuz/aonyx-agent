# Plan — First-run setup wizard + bundled sidecar (Aonyx Agent desktop)

**Date** : 2026-06-21 · **ADR** : ADR-016 · **Statut** : décisions actées (sidecar embarqué + wizard complet)

## Objectif
Au 1er lancement, un onboarding poli façon Hermes Agent — mais adapté au **binaire Rust unique** (zéro Python). Trois écrans de choix (provider, RAG, embeddings) puis un écran **auto-bootstrap** (stepper + progression) qui prépare le moteur, télécharge le modèle d'embeddings, démarre l'agent.

## Décisions (ADR-016)
- Binaire `aonyx` **embarqué en sidecar** Tauri (`externalBin`, par triple cible), `--features api,rag`. `start_local` spawn le sidecar, plus de dépendance au PATH.
- Wizard **complet** : provider + RAG backend (ADR-008) + embeddings (ADR-009) + bootstrap.

## Non-objectifs
- Pas d'install de toolchain système (uv/Python/node/git) — on n'est pas Hermes.
- Pas de re-build à l'install (on livre le binaire compilé).
- Pas de refonte du backend agent Rust hors « prepare-embeddings » (progression download).

## Stories (effort S/M/L)
- **W1 — Sidecar** (M) : `tauri.conf.json` `bundle.externalBin` + staging du binaire par triple (`aonyx-<triple>(.exe)`), `start_local` via le sidecar (`tauri-plugin-shell`/process). Le dev continue de marcher via PATH. *Acc.* : app packagée lance l'agent sans `aonyx` sur le PATH.
- **W2 — Gating + shell** (S) : `setup_state` (Rust) + gate dans `App.jsx` (non configuré → Wizard). Coquille wizard (TitleBar frameless, stepper, branding loutre). *Acc.* : 1er lancement → wizard ; configuré → app.
- **W3 — Écrans de choix** (M) : Provider (réutilise `list_models`), RAG (local/MCP externe), Embeddings (local/provider). Persistance via `save_setup` → `config.toml` (+ `[rag]`). *Acc.* : les choix s'écrivent et se relisent.
- **W4 — Auto-bootstrap + progression** (L) : écran stepper (visuel validé) piloté par les vraies étapes — stage sidecar, détecter env, init palais, écrire config, **télécharger le modèle d'embeddings (progression)**, démarrer l'agent. Commande agent `aonyx … prepare-embeddings --progress` + commande Tauri qui stream la progression via `Channel`. *Acc.* : barre réelle pendant le download ; à la fin l'agent répond (`api_info`).
- **W5 — Polish/i18n/tests** (S) : strings FR/EN, annulation/erreurs, « relancer le wizard » depuis Settings, marqueur `setup_complete`. *Acc.* : OS FR → wizard FR ; annulation propre ; rejouable.

## Séquencement
W2 → W3 → W4 (UI testable en dev via PATH) ; **W1 (sidecar) en parallèle/avant le packaging** ; W5 en clôture.

## Risques
- `externalBin` : nommage par triple + signature/notarisation par OS — CI à étendre.
- Progression fastembed : `hf-hub` expose un callback de download → l'exposer en CLI puis le streamer.
- Cohérence dimension embeddings si changement ultérieur (ADR-009 : re-index).
