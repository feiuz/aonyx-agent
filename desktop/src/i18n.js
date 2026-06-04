// Tiny dependency-free i18n. Flat key dictionaries + a detector.
const fr = {
  "nav.dashboard": "Dashboard",
  "nav.chat": "Chat",
  "nav.projects": "Projets",
  "nav.stats": "Statistiques",
  "nav.memory": "Memory Health",
  "nav.kg": "Knowledge Graph",
  "nav.users": "Utilisateurs",
  "nav.permissions": "Permissions",
  "nav.mcp": "MCP",
  "nav.settings": "Paramètres",

  "auth.signin": "Se connecter",
  "auth.signout": "Se déconnecter",
  "sidebar.collapse": "Réduire",
  "sidebar.expand": "Étendre",
  "theme.toLight": "Thème clair",
  "theme.toDark": "Thème sombre",
  "update.label": "MAJ",

  "chat.conversations": "Conversations",
  "chat.new": "Nouveau",
  "chat.placeholder": "Message Aonyx…  (Entrée pour envoyer · Maj+Entrée = nouvelle ligne)",
  "chat.empty.ready": "Pose ta question — je stream la réponse et j'appelle mes outils.",
  "chat.empty.configure": "Configure le provider dans Paramètres.",
  "chat.none": "Aucune conversation.",
  "chat.turn": "tour",
  "chat.turns": "tours",
  "chat.you": "vous",
  "status.offline": "hors ligne",
  "status.connecting": "connexion…",

  "stub.dashboard": "Vue d'ensemble — à venir.",
  "stub.construction": "Section en construction.",
  "stub.users": "Multi-utilisateurs — réservé à aonyx-account (à confirmer).",
  "stub.permissions": "Réservé à aonyx-account (à confirmer).",
  "stub.mcp": "Gestion des serveurs MCP — à venir.",
  "kg.soon": "Bientôt",
  "kg.note": "La visualisation du graphe (cytoscape) nécessite un endpoint KG côté agent — à brancher après le MVP.",

  "memory.subtitle": "Recherche hybride dans le palais de mémoire (BM25 + vecteurs · RRF)",
  "memory.search": "Cherche dans la mémoire…",
  "memory.connect": "Connecte-toi (Paramètres) pour chercher",
  "memory.hint": "Lance une recherche pour voir les passages les plus proches.",
  "memory.none": "Aucun résultat.",
  "memory.searching": "recherche…",

  "stats.conversations": "Conversations",
  "stats.turns": "Tours",
  "stats.provider": "Fournisseur",
  "stats.model": "Modèle",
  "common.connect": "Connecte-toi (Paramètres).",
  "projects.subtitle": "Projets déduits des conversations",
  "projects.none": "Aucun projet — démarre une conversation dans Chat.",
  "projects.count": "conv.",

  "settings.providerSection": "Fournisseur LLM",
  "settings.provider": "Fournisseur",
  "settings.model": "Modèle",
  "settings.connectionSection": "Connexion",
  "settings.save": "Enregistrer & connecter",
};

const en = {
  "nav.dashboard": "Dashboard",
  "nav.chat": "Chat",
  "nav.projects": "Projects",
  "nav.stats": "Statistics",
  "nav.memory": "Memory Health",
  "nav.kg": "Knowledge Graph",
  "nav.users": "Users",
  "nav.permissions": "Permissions",
  "nav.mcp": "MCP",
  "nav.settings": "Settings",

  "auth.signin": "Sign in",
  "auth.signout": "Sign out",
  "sidebar.collapse": "Collapse",
  "sidebar.expand": "Expand",
  "theme.toLight": "Light theme",
  "theme.toDark": "Dark theme",
  "update.label": "Update",

  "chat.conversations": "Conversations",
  "chat.new": "New",
  "chat.placeholder": "Message Aonyx…  (Enter to send · Shift+Enter for a newline)",
  "chat.empty.ready": "Ask away — I stream the reply and call my tools.",
  "chat.empty.configure": "Configure the provider in Settings.",
  "chat.none": "No conversations.",
  "chat.turn": "turn",
  "chat.turns": "turns",
  "chat.you": "you",
  "status.offline": "offline",
  "status.connecting": "connecting…",

  "stub.dashboard": "Overview — coming soon.",
  "stub.construction": "Section under construction.",
  "stub.users": "Multi-user — handled by aonyx-account (TBC).",
  "stub.permissions": "Handled by aonyx-account (TBC).",
  "stub.mcp": "MCP server management — coming soon.",
  "kg.soon": "Soon",
  "kg.note": "The graph view (cytoscape) needs a KG endpoint on the agent — wired after the MVP.",

  "memory.subtitle": "Hybrid memory-palace search (BM25 + vectors · RRF)",
  "memory.search": "Search the memory…",
  "memory.connect": "Sign in (Settings) to search",
  "memory.hint": "Run a search to see the closest passages.",
  "memory.none": "No results.",
  "memory.searching": "searching…",

  "stats.conversations": "Conversations",
  "stats.turns": "Turns",
  "stats.provider": "Provider",
  "stats.model": "Model",
  "common.connect": "Sign in (Settings).",
  "projects.subtitle": "Projects inferred from conversations",
  "projects.none": "No projects — start a conversation in Chat.",
  "projects.count": "conv.",

  "settings.providerSection": "LLM Provider",
  "settings.provider": "Provider",
  "settings.model": "Model",
  "settings.connectionSection": "Connection",
  "settings.save": "Save & connect",
};

export const DICTS = { fr, en };

export function detectLang() {
  const saved = localStorage.getItem("aonyx.lang");
  if (saved === "fr" || saved === "en") return saved;
  const nav = (typeof navigator !== "undefined" && navigator.language ? navigator.language : "en").toLowerCase();
  return nav.startsWith("fr") ? "fr" : "en";
}
