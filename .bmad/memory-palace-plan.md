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
- **H5 — Deux surfaces.** **Palais de Mémoire** = vue **globale / stats transverse**
  (renommée depuis Memory Health, read-only) ; **RAG** (nouveau) = **console par
  projet** + gestion + règles globales. Réutilise l'existant (KG, diary, stats,
  sessions). Voir ci-dessous. *Effort M-L.*
- **H6 — Time-machine.** Plomber `as_of` à travers KG / diary / search (le module
  `time_machine` est un stub) + un curseur temporel dans le Palais. *Effort M.*

## Deux surfaces : « Palais de Mémoire » (global) + « RAG » (par projet) (H5)

Sidebar : **renommer `Memory Health` → « Palais de Mémoire »** et **ajouter un item
`RAG`**.

### Palais de Mémoire — vue globale / statistiques (read-only)

La **grande vision transverse** de toute la mémoire, **tous projets confondus** :

- **Stats globales** : nb de projets · total chunks/documents · entités + relations
  (KG) · entrées de diary · conversations + tours.
- **Diaries cross-projets** : timeline datée des décisions/actions, tous projets.
- **Explorateur KG** : le knowledge-graph global (réutilise la vue KnowledgeGraph).
- **Time-machine** : curseur `as_of` → l'état de la mémoire à une date passée.
- **Répartition par projet** : tableau `projet | docs | entités | diary | dernière activité`.

### RAG — console par projet (gestion + opérations)

Le **hub des projets** + la console du projet sélectionné :

- Liste / créer / renommer / supprimer un projet ; définir le **projet actif**.
- **Règles globales** : édition du projet réservé `knowledge` (toujours injecté, protégé).
- Par projet, en onglets : **Ingérer · Documents · Recherche (scopée) · Journal**.

## Améliorations avec l'existant (déjà codé — à brancher)

| Brique existante | État | À en faire |
|---|---|---|
| **Knowledge Graph** (`/v1/memory/kg/*` + vue KnowledgeGraph) | ✅ codé | Explorateur KG global (Palais) + compteurs ; KG par projet (RAG). |
| **Diary** (`/v1/memory/diary`) | ✅ codé | Timeline cross-projets (Palais) ; par projet (RAG). |
| **Sessions / turns** (`sessions.db`) | ✅ codé | Stats conversations (nb · tours), global + par projet. |
| **Stats + Dashboard** (vues) | ✅ codé | **Consolider** dans le Palais (3 vues qui se recoupent → 1). |
| **Recherche hybride** (`/v1/memory/search`) | ✅ codé | Globale (Palais) + scopée projet (RAG). |
| **`time_machine`** (`as_of`) | ⚠️ **stub (TODO)** | **Le plomber** (KG + diary + search) → curseur temporel. Le moat « memory-first » à finir. |

**Ce que tu oubliais sûrement** : le module **`time_machine`** (requêtes `as_of` —
voir la mémoire à une date passée) est **prévu mais non branché** ; c'est LA feature
mémoire-first à exposer dans le Palais. Idem le **KG timeline** et les **stats
sessions/tours**, déjà là mais pas agrégés.

**Endpoints à ajouter** pour alimenter le global : `GET /v1/memory/projects` (liste
+ stats par projet), `GET /v1/memory/stats` (agrégats globaux),
`GET /v1/memory/timeline?as_of=` (time-machine).

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
