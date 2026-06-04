# BMAD — PRD + Plan : RAG documentaire in-app

**Project**: Aonyx Agent
**Date**: 2026-06-04
**Status**: Planned (proposé). Cible **v0.10.0**.
**Décisions**: ADR-008 (backend RAG au setup) + ADR-009 (embeddings au setup), étendent ADR-005.
**Severity**: 🟠 Important — concrétise la promesse cœur **G2** (« memory palace : hybrid search ») aujourd'hui livrée en **BM25 seul**, et corrige une sur-promesse du pitch (le site clame « BM25 + vectors + RRF » alors que les vecteurs sont des stubs).

---

## Problème (vérifié en source)

Le PRD V1 (G2) annonce un palais avec *hybrid search BM25 + vecteurs + RRF, splitter AST, cross-link* (ports de `rag_system/utils/*.py`). **La réalité du code** :

- ✅ `crates/aonyx-memory/src/chunks.rs` — store SQLite **FTS5 BM25** réel, multilingue (`unicode61 remove_diacritics 2`), `Chunk{project,source,content,kind,metadata}` + `search_bm25(project?,query,k)`.
- ⚠️ `Palace::hybrid_search` (palace.rs) = **BM25 seul** ; commentaire explicite *« V1.1 will fuse with fastembed-rs + HNSW via RRF »*.
- ❌ Stubs : `hybrid.rs` (10 l., TODO), `splitter.rs` (9 l.), `time_machine.rs` (6 l.), `cross_link.rs` (5 l.). Deps `fastembed-rs`/`hnsw_rs`/`tree-sitter` **absentes** (commentaire « join when their subsystems land »).
- ✅ Déjà en place : tool agent `memory_search` → `hybrid_search` ; **`auto_retrieve`** (ADR-006, cherche un tool `…rag_search`) ; client MCP (chemin RAG externe) ; `aonyx setup` + `Config` (config.rs) ; flux d'ingest TUI (doc/note/code).

Par ailleurs l'utilisateur **dogfoode** un RAG externe (bot Telegram → `aonyx-rag` MCP). On veut la **même capacité, native dans l'app**, génération via le **LLM configuré**.

> ⚠️ **Clean-room (ADR-001)** : on *porte le pattern* du RAG Python, **jamais les données** de l'utilisateur, et **aucun pipe** vers son corpus perso.

---

## Goals (de la feature)

| ID | Goal | Mesure |
|---|---|---|
| RG1 | Rendre la recherche **réellement hybride** (BM25 + vecteurs + RRF), pas BM25 seul. | M4 PRD : recall@10 ≥ 90 % sur l'eval set. |
| RG2 | RAG **utilisable in-app, offline, provider-agnostic** : ingest des docs → réponses citées via le LLM configuré. | `aonyx ingest` + réponses avec sources, sans clé cloud. |
| RG3 | **Choix au setup** : backend (local/externe) + embeddings (local/provider). | 2 prompts `aonyx setup` + champs `[rag]` persistés. |
| RG4 | **Zéro régression** : palais existants (BM25-only, sans vecteurs) continuent de marcher. | Fallback BM25 si embedder absent ; sessions/DB rétro-compatibles. |

## Non-goals (v0.10.0)

- **Pas** de réimplémentation du RAG externe complet (projets-as-a-service, time-machine serveur, FastAPI). On reste **palais-scoped**.
- **Pas** de re-ranking LLM (le RAG externe a un 14B ; trop lourd en local) — RRF suffit au MVP.
- **Pas** de HNSW au MVP (cosine brute-force ; HNSW = Phase 2, cf. ADR-005).
- **Pas** de splitter AST / boost temporel / KG-augmented retrieval au MVP (le *moat*, Phase 2).
- **Pas** de bundling du modèle d'embeddings dans le binaire (download au 1er run — garde M6 binary size).
- **Aucune** ingestion des données du RAG perso de l'utilisateur.

---

## Décisions (résumé — détail dans decisions.md)

- **ADR-008** — Backend RAG **choisi au setup** : `local` (palais built-in, tool `rag_search` local) **ou** `external` (MCP `__rag_search` déjà supporté). Même contrat dans les deux cas → `auto_retrieve` et le tool-calling marchent à l'identique.
- **ADR-009** — Embeddings **choisis au setup** : `local` (fastembed-rs, défaut, offline) **ou** `provider` (OpenAI/Ollama). Opérationnalise ADR-005. Motif : **Anthropic & claude-code n'ont pas d'API embeddings** → le local doit rester le défaut sûr.

---

## Architecture (types/traits clés)

**Nouveau** `crates/aonyx-memory/src/embed.rs` :

```rust
#[async_trait]
pub trait Embedder: Send + Sync {
    fn model_id(&self) -> &str;   // ex. "bge-m3" — stocké pour la cohérence dim
    fn dim(&self) -> usize;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
```
- `LocalEmbedder` — `fastembed-rs` (ONNX), modèle multilingue, téléchargé au 1er run et caché sous `~/.aonyx/models/`.
- `ProviderEmbedder` — HTTP vers l'endpoint embeddings (OpenAI `/v1/embeddings`, Ollama `/api/embeddings`), réutilise la clé/base_url du provider.

**Stockage vecteurs** (extension de `chunks.rs`) — table parallèle, n'altère pas le FTS5 existant :
```sql
CREATE TABLE IF NOT EXISTS chunk_vectors (
  chunk_id TEXT PRIMARY KEY, model_id TEXT, dim INTEGER, vec BLOB
);
```

**Fusion** — `MemoryStore::hybrid_search(query, k, mode)` (ajout du `mode ∈ {bm25, semantic, hybrid}`) :
- `bm25` = actuel ; `semantic` = cosine top-k ; `hybrid` = **RRF k=60** sur l'union BM25 ∪ vecteurs.
- **Fallback gracieux** : si pas d'embedder configuré ou pas de vecteurs → `bm25` (comportement actuel).

**Tool built-in `rag_search`** (`aonyx-tools/src/memory.rs`) — adossé au palais, renvoie des **citations** :
```json
{ "query": "...", "results": [ { "project": "...", "source": "...", "content": "...", "score": 0.0 } ] }
```
Nom **exact `rag_search`** → matché par `auto_retrieve` (qui cherche `== "rag_search"` ou `…__rag_search`) ⇒ RAG local **sans serveur**, même contrat que le MCP externe. (`memory_search` actuel ne renvoie que `{content,score}` — on enrichit avec `project/source`.)

**Config** (`config.rs`) :
```toml
[rag]
backend    = "local"     # local | external      (ADR-008)
embeddings = "local"     # local | provider      (ADR-009)
embed_model = "bge-m3"   # modèle fastembed (si embeddings=local)
```
**Setup** : `aonyx setup` (CLI) + wizard desktop posent les 2 questions ; défauts `local`/`local`. Backend `external` ⇒ on ne registre pas le `rag_search` local, on s'appuie sur le MCP `__rag_search` déjà configuré.

---

## Épics → Stories

**Effort** : S ≤ ½ j · M ≈ 1–2 j · L ≈ 3 j+. Critères en Given/When/Then.

### Phase 0 — MVP (RAG local self-contained) → v0.10.0

| ID | Story | Effort |
|---|---|---|
| **R1** | Trait `Embedder` + `LocalEmbedder` (fastembed-rs, download+cache 1er run) | **M** |
| **R2** | Table `chunk_vectors` + `embed_and_store` à l'append quand un embedder est configuré (stocke `model_id`+`dim`) | **M** |
| **R3** | Recherche cosine top-k sur les vecteurs d'un projet | **S** |
| **R4** | `hybrid_search(query,k,mode)` + **fusion RRF k=60** + fallback BM25 | **S** |
| **R5** | Tool built-in **`rag_search`** (citations `project/source/content/score`) ; registré si `backend=local` | **S** |
| **R6** | CLI **`aonyx ingest <chemin>`** (fichiers/dossier, txt+md) → chunk → embed → store `kind:"doc"` | **M** |
| **R7** | Prompts setup + `Config[rag]` (backend, embeddings, embed_model) — ADR-008/009 | **S** |

**Critères d'acceptation (P0) :**
- R1 — *Given* aucun modèle en cache, *When* premier `embed(["bonjour","hello"])`, *Then* le modèle se télécharge une fois, est mis en cache, et renvoie 2 vecteurs de `dim()` cohérente ; runs suivants **offline**.
- R4 — *Given* un corpus ingéré avec vecteurs, *When* `hybrid_search(q,k,"hybrid")`, *Then* le top-k bat le BM25 seul sur l'eval (recall@10 ≥ 90 %, M4) ; *Given* `embeddings` off, *Then* renvoie le résultat BM25 sans erreur.
- R5 — *Given* `backend=local`, *When* l'agent appelle `rag_search`, *Then* il reçoit des résultats **avec source/projet** ; *And* `auto_retrieve` les pré-charge **sans MCP externe**.
- R6 — *Given* `aonyx ingest ./docs` (N fichiers), *When* terminé, *Then* N docs sont chunkés+indexés et `rag_search`/`memory_search` les retrouvent.
- R7 — *Given* `aonyx setup`, *When* l'utilisateur choisit backend/embeddings, *Then* c'est persisté dans `[rag]` ; défauts `local`/`local`.

### Phase 1 — fast-follow

| ID | Story | Effort |
|---|---|---|
| R8 | `ProviderEmbedder` (OpenAI/Ollama embeddings) — option `embeddings=provider` | **M** |
| R9 | Détection mismatch `model_id`/`dim` → `aonyx memory reindex` (re-embed) | **S** |
| R10 | Desktop : "add documents" + panneau RAG (ingest + chat-over-corpus via `rag_search`) | **M** |
| R11 | `auto_retrieve` sur le TUI interactif (le fast-follow d'ADR-006) | **S** |

### Phase 2 — moat (différé)

| ID | Story | Effort |
|---|---|---|
| R12 | HNSW (`hnsw_rs`) en remplacement du cosine brute-force, au-delà de ~50k chunks (ADR-005) | **L** |
| R13 | Splitter AST tree-sitter (Rust/Py/JS/TS/Go) → chunks code cohérents (G2) | **M** |
| R14 | Retrieval augmenté KG + boost temporel + cross-link (les vrais différenciateurs) | **M** |

---

## Plan de test

- **Unitaires** : `Embedder` (dim stable, déterminisme), cosine (ordre attendu), **maths RRF** (fusion connue → rang attendu), shape `rag_search`, classifieur d'ingest (`ingest_kind_from_path`).
- **Intégration** : ingest → recall@10 sur l'eval set (**M4 ≥ 90 %**) ; fallback BM25 (embeddings off) ; **rétro-compat** (palais sans `chunk_vectors` → BM25 OK) ; switch d'embedder → reindex.
- **Gate** : `cargo test --workspace` + `cargo clippy --all-features -D warnings`.
- **Manuel** : `aonyx ingest` puis `serve`/TUI sur une question → réponse **citée** ; mode `external` → mêmes réponses via MCP.

## Séquencement / rollout

1. **Moteur** : R1 → R2 → R3 → R4 (l'hybride réel).
2. **Utilisable** : R5 (`rag_search`) + R6 (`ingest`).
3. **Choix** : R7 (setup). → **MVP shippable v0.10.0**.
4. Puis Phase 1 (provider embeddings + desktop + TUI), Phase 2 (moat).
- **Rollout sûr** : `backend`/`embeddings` défaut `local` ; embeddings *lazy* (download au 1er `ingest`, avec notice) ; binaire reste lean (modèle non bundlé, cf. M6).
- **Doc** : CHANGELOG `[0.10.0]` + maj `prd.md` (statut G2) + le site (rend vraie la promesse « vectors »).

## Risques

| Risque | Mitigation |
|---|---|
| Taille modèle 30–130 Mo | Download au 1er run (pas bundlé) ; notice ; cache `~/.aonyx/models`. |
| Cohérence dimension si changement d'embedder | Stocker `model_id`+`dim` par vecteur ; mismatch → `reindex` (R9) ; vecteurs périmés ignorés. |
| Multilingue (FR+EN) | Modèle multilingue (`bge-m3` / `multilingual-e5`) — à trancher (OQ1). |
| Scope creep vers le RAG externe complet | Non-goals explicites ; palais-scoped ; ADR-008 délègue l'externe au MCP. |
| Migration DB | `chunk_vectors` en table séparée, `CREATE … IF NOT EXISTS` ; FTS5 intact. |

## Open questions

- **OQ1 (eng/data)** — Modèle d'embeddings local par défaut : ADR-005 dit *MiniLM-L6-multilingual* ; `bge-m3`/`multilingual-e5-small` sont meilleurs en FR. Trancher (qualité vs taille). *Bloquant R1.*
- **OQ2 (eng)** — Plafond du cosine brute-force avant HNSW (nb de chunks) ? Définit le seuil R12. *Non bloquant.*
- **OQ3 (eng)** — Quels providers exposent des embeddings dans notre config (OpenAI ✓, Ollama ✓, LM-Studio ✓, OpenRouter ?) → périmètre R8. *Non bloquant.*
- **OQ4 (produit)** — Corpus = palais par-projet (actuel) ou notion de **collection** transverse ? *Non bloquant (défaut : par-projet).*
- **OQ5 (eng)** — Desktop : ingest via nouvel endpoint `POST /v1/memory/ingest` ou commande Tauri locale ? *Non bloquant (tranché en R10).*
