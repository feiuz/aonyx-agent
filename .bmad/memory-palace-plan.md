# Plan — Memory Palace par projet (RAG intégré · projet knowledge · diaries)

> Demande (Damien) : le memory palace doit fonctionner **en raccord avec le RAG
> intégré, par projets**. Dans chaque conversation on **choisit le projet RAG** ;
> le palais **crée ou retrouve tout seul** le palais de mémoire du projet. Il gère
> un **projet `knowledge` désigné (règles globales)** et les **diaries dans les
> conversations**. Réorganiser la page Memory Health en conséquence. 2026-06-22.

## L'idée, re-détaillée

1. **Un projet RAG = un palais de mémoire.** Chaque projet a sa mémoire : ses
   documents (chunks vectorisés), son knowledge-graph, son diary.
2. **Choix du projet par conversation.** À la création d'une conversation (ou
   dans la conversation), on **sélectionne ou crée** un projet RAG. Le palais
   **auto-gère** : nouveau projet → créé à la 1ʳᵉ écriture ; projet existant →
   retrouvé. L'utilisateur ne gère jamais de fichiers à la main.
3. **RAG scopé au projet.** Pendant la conversation, `rag_search` / `memory_search`
   et l'auto-retrieve **ne voient que la mémoire du projet choisi** → contexte
   pertinent, pas de pollution entre projets.
4. **Projet `knowledge` (règles globales).** Un projet **réservé** dont le contenu
   = les **règles/instructions globales**. Il est **toujours injecté** dans le
   contexte de **toute** conversation (par-dessus le projet courant) → persona +
   règles persistantes valables partout.
5. **Diaries dans les conversations.** Le diary (journal narratif daté de ce que
   l'agent fait/décide) est alimenté **au fil de la conversation** et rattaché au
   projet/à la conversation → on peut relire l'historique des décisions.

## État actuel (vérifié dans le code)

- Le palais (`aonyx-memory`) stocke chunks + diary + KG + sessions dans des bases
  SQLite, et **tout est déjà tagué par projet** : `Chunk.project`,
  `DiaryEntry.project`, `Session.project` ; `search_bm25(Some(project), …)` sait
  scoper. Donc « palais par projet » = **données taguées par projet** (un palais
  physique, des palais *virtuels* par projet) — l'auto-création est gratuite.
- **Ce qui manque** : le desktop crée les sessions **sans** projet ;
  `Palace::search` appelle `search_bm25(None, …)` → **cherche tous projets
  confondus** ; pas de projet `knowledge` ni d'injection globale ; pas de
  sélecteur de projet ni de gestion des diaries côté UI.

## Concept technique cible

- **Sélecteur de projet** rattaché à la **session** (`session.project`). La
  conversation porte son projet ; l'API `POST /v1/sessions` le prend déjà
  (`project`), le desktop doit l'exposer.
- **Scoping** : le runner / les tools `rag_search` & `memory_search` reçoivent le
  **projet de la session courante** → `hybrid_search` scopé. L'**ingestion** cible
  le projet courant (déjà le cas via `ingest_text(project, …)`).
- **Couche globale** : à chaque tour, injecter le top-k du projet **`knowledge`**
  (règles globales) **+** le top-k du projet courant. Règles globales aussi
  exposées comme un bloc éditable (≈ système prompt persistant).
- **Diary** : `diary_append(project, …)` au fil de la conversation (le tool memory
  existe) ; vue timeline par projet/conversation.

## Phases

- **H1 — Projet par conversation.** Sélecteur de projet à la création + dans la
  conversation ; `session.project` posé ; le desktop liste les projets (depuis les
  chunks/sessions distincts). *Effort M.*
- **H2 — Scoping RAG.** Passer le projet courant à `hybrid_search` / `rag_search` /
  auto-retrieve (runner + tool + endpoint `/v1/memory/search?project=`). *Effort M.*
- **H3 — Projet `knowledge` (règles globales).** Projet réservé ; injection de son
  contenu dans le contexte de chaque tour (par-dessus le projet courant) ; bloc
  d'édition des règles globales. *Effort M.*
- **H4 — Diaries.** `diary_append` au fil de la conversation + endpoint/vue timeline
  par projet ; option d'auto-journalisation (résumé de tour). *Effort M.*
- **H5 — Deux surfaces : « Palais de Mémoire » + « RAG ».** Renommer le nav
  *Memory Health* → **Palais de Mémoire** (la console mémoire du projet courant),
  et **ajouter un nav `RAG`** dédié à la **gestion des projets** + aux règles
  globales. Voir ci-dessous. *Effort M-L.*

## Deux surfaces : « Palais de Mémoire » + « RAG » (H5)

Dans le sidebar : **renommer `Memory Health` → « Palais de Mémoire »** et **ajouter
un item `RAG`**. Deux surfaces complémentaires.

**RAG (nouveau) — gestion des projets.** Le hub des projets de mémoire : liste des
projets (stats docs/chunks · dimension · modèle d'embedding), **créer / renommer /
supprimer**, définir le **projet actif**, et **édition des règles globales** (projet
réservé `knowledge`, protégé/non-supprimable). C'est là qu'on gère les palais.

**Palais de Mémoire (ex-Memory Health) — la console du projet.** La mémoire du
**projet courant** (sélectionné dans RAG / la conversation), façon la capture RAG :

```
┌ MÉMOIRE ──────────────────────────────────────────────┐
│ Projet : [ ▼ mon-projet ]  [+ Nouveau]   docs 123 · dim 384 · bge-m3 │
├───────────────────────────────────────────────────────┤
│ ⚙ Règles globales (projet `knowledge`)   [éditer]      │
├──────────────┬──────────────┬─────────────┬───────────┤
│  Ingérer     │  Documents   │  Recherche  │  Journal  │  (onglets)
├───────────────────────────────────────────────────────┤
│  …contenu de l'onglet, scopé au projet sélectionné…    │
└───────────────────────────────────────────────────────┘
```

- **Barre de projet** : sélecteur (liste des projets) + « Nouveau » + stats
  (docs/chunks, dimension, modèle d'embedding).
- **Règles globales** : encart d'édition du projet `knowledge` (toujours injecté).
- **Onglets scopés au projet** :
  - **Ingérer** — le panneau actuel (texte / fichier), cible le projet.
  - **Documents** — liste des documents ingérés (consulter / supprimer).
  - **Recherche** — recherche hybride **scopée au projet**.
  - **Journal** — le diary (timeline datée) du projet/des conversations.

## Risques / garde-fous

- **Migration** : les chunks déjà ingérés ont un projet (`src-tauri` vu en test) →
  prévoir un projet par défaut + un renommage/fusion de projet.
- **Fuite entre projets** : par défaut **scoper** ; ne jamais chercher tous projets
  sauf demande explicite. La couche `knowledge` est la seule transverse.
- **Création implicite** : un projet naît à la 1ʳᵉ ingestion/diary — éviter les
  projets fantômes (valider le nom, lister les non-vides).
- **`knowledge` réservé** : nom protégé, non supprimable, clairement signalé.
- **Cohérence embeddings** : si on change d'embedder, ré-indexer (model_id stocké).

## Dépendances

- S'appuie sur **D** (RAG ingest, livré) et le scoping projet déjà dans le schéma.
- Le sélecteur de projet (H1) recoupe **Workspace** dans `settings-org-plan.md`.
