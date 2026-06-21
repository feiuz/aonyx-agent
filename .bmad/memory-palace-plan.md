# Plan — Memory Palace par projet (RAG intégré · projet knowledge · diaries)

> Demande (Damien) : le memory palace doit fonctionner **en raccord avec le RAG
> intégré, par projets**. Dans chaque conversation on **choisit le projet RAG** ;
> le palais **crée ou retrouve tout seul** le palais de mémoire du projet. Il gère
> un **projet `knowledge` désigné (règles globales)** et les **diaries dans les
> conversations**. Réorganiser la page Memory Health en conséquence. 2026-06-22.

## L'idée, re-détaillée

1. **Un projet RAG = un répertoire de travail (workspace).** Chaque projet est un
   **dossier** que l'utilisateur définit ; son palais de mémoire vit à
   **`<dossier>/.aonyx/`** (chunks vectorisés, KG, diary). L'agent **opère dans ce
   dossier** (ses tools `fs_*` y sont rootés).
2. **Choix du projet directement sur le chat.** Chaque **conversation** est
   rattachée à un **répertoire**, choisi via un **sélecteur de dossier** (dialog
   natif) à la création du chat (ou dans le chat). Le palais est **auto-créé** à
   `<dossier>/.aonyx/` s'il n'existe pas, **retrouvé** sinon. Zéro fichier à gérer.
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

- `Palace::default_project_dir(root)` = **`<root>/.aonyx/`** : le modèle « un
  dossier = un palais » **existe déjà** (la CLI ouvre le palais du cwd). Les bases
  taguent aussi par projet (`Chunk.project`, `Session.project`).
- **Ce qui manque** : le **sidecar n'ouvre qu'UN palais** (celui de son cwd) → il
  faut un **registre de palais keyé par répertoire**, ouvert selon le dossier de la
  session. Le desktop crée les sessions **sans** répertoire ; pas de **sélecteur de
  dossier** ; `Palace::search` ne scope pas ; pas de projet `knowledge`/règles
  globales ; diaries non exposés.

## Concept technique cible

- **Répertoire rattaché à la session.** La conversation porte son **dossier**,
  choisi via un **dialog de sélection de dossier** (plugin Tauri) **dans le chat**.
  `session.project` = un slug dérivé du dossier ; le **chemin** est stocké.
- **Registre de palais.** Le sidecar ouvre/cache un `Palace` par **répertoire**
  (`<dir>/.aonyx/`) et utilise celui du dossier de la session au moment du tour.
  L'ingestion, le RAG et les diaries passent par ce palais.
- **Scoping** : `rag_search` / `memory_search` / auto-retrieve tournent sur le
  palais du dossier courant → pas de pollution entre projets.
- **Couche globale** : à chaque tour, injecter le top-k du projet **`knowledge`**
  (règles globales) **+** le top-k du projet courant. Règles globales aussi
  exposées comme un bloc éditable (≈ système prompt persistant).
- **Diary** : `diary_append(project, …)` au fil de la conversation (le tool memory
  existe) ; vue timeline par projet/conversation.

## Phases

- **H1 — Projet (= répertoire) sur le chat.** Sélecteur de **dossier** (dialog
  Tauri) **dans le chat** (header/composer de la conversation) ; le chemin + le slug
  sont posés sur la session ; le sidecar ouvre le palais `<dir>/.aonyx/` via un
  **registre par chemin**. ⚠️ *Le picker global du sidebar (v1) est à **déplacer
  sur le chat** et à passer du nom de projet au **dossier**.* *Effort M-L.*
- **H2 — Scoping RAG sur le palais du dossier.** Le runner / les tools (`rag_search`,
  `memory_search`, ingest) + l'auto-retrieve utilisent le **palais du répertoire de
  la session** (via le registre H1). `/v1/memory/*` accepte le dossier (ou le slug).
  *Effort M.*
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
